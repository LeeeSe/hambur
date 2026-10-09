//! Exa search provider: a plain JSON search API, registered in the same seam as the
//! DeepSeek provider so the deployment can switch backends without touching the tools.
//!
//! Unlike the DeepSeek provider this one costs no model turn: it returns citable results
//! directly, and the snippet comes from the page text Exa returns for each hit.

use serde::Deserialize;
use serde_json::json;
use std::time::Duration;

use crate::cancel::{CancelToken, race_cancel};
use crate::seam::{ProviderId, SearchProviderSpec};
use crate::types::{
    WEB_ABORTED, WEB_PROVIDER_CREDENTIAL_MISSING, WEB_PROVIDER_ERROR, WebError, WebSearchRequest,
    WebSearchResult, WebSearchSource,
};

/// Stable id this provider registers under.
pub const EXA_PROVIDER_ID: &str = "exa";
/// Default Exa API endpoint; `/search` is appended.
pub const EXA_DEFAULT_BASE_URL: &str = "https://api.exa.ai";
/// Default characters of page text requested per hit (the snippet source).
pub const EXA_DEFAULT_TEXT_CHARS: usize = 1200;
/// Provider-side ceiling on requested results, independent of the seam's bound.
pub const EXA_MAX_RESULTS: usize = 25;

const USER_AGENT: &str = "hambur/0.1 (+web-search-exa)";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Deserialize)]
struct ExaResult {
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default, rename = "publishedDate")]
    published_date: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ExaResponse {
    #[serde(default)]
    results: Vec<ExaResult>,
    #[serde(default)]
    error: Option<String>,
}

/// Map an Exa search response to a normalized search result.
pub fn map_exa_response(body: &str, snippet_chars: usize) -> Result<WebSearchResult, WebError> {
    let parsed: ExaResponse = serde_json::from_str(body).map_err(|error| {
        WebError::new(
            WEB_PROVIDER_ERROR,
            format!("Exa returned an unprocessable response body: {error}"),
        )
    })?;
    if let Some(error) = parsed.error.as_deref().filter(|error| !error.is_empty()) {
        return Err(WebError::new(
            WEB_PROVIDER_ERROR,
            format!("Exa error: {error}"),
        ));
    }

    let mut seen = std::collections::HashSet::new();
    let mut sources = Vec::new();
    for result in parsed.results {
        let url = result.url.trim();
        if url.is_empty() || !seen.insert(url.to_string()) {
            continue;
        }
        sources.push(WebSearchSource {
            url: url.to_string(),
            title: non_empty(result.title.as_deref()),
            snippet: result
                .text
                .as_deref()
                .map(|text| excerpt(text, snippet_chars))
                .filter(|text| !text.is_empty()),
            published_at: non_empty(result.published_date.as_deref()),
        });
    }

    Ok(WebSearchResult {
        content: None,
        sources,
        truncated: false,
    })
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Collapse whitespace and cut a page excerpt to `limit` characters.
fn excerpt(value: &str, limit: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let mut cut = String::new();
    for character in collapsed.chars().take(limit.saturating_sub(1)) {
        cut.push(character);
    }
    cut.push('…');
    cut
}

/// The Exa-backed search provider.
#[derive(Clone)]
pub struct ExaSearchProvider {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    snippet_chars: usize,
    /// Provider-side result request size for the next operation.
    request_results: usize,
}

impl ExaSearchProvider {
    pub fn new(
        base_url: &str,
        api_key: &str,
        snippet_chars: usize,
        request_results: usize,
    ) -> Self {
        let base = base_url.trim().trim_end_matches('/');
        let endpoint = format!("{base}/search");
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(15))
            .user_agent(USER_AGENT)
            .build()
            .unwrap_or_default();
        Self {
            client,
            endpoint,
            api_key: api_key.trim().to_string(),
            snippet_chars: snippet_chars.max(80),
            request_results: request_results.clamp(1, EXA_MAX_RESULTS),
        }
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl ProviderId for ExaSearchProvider {
    fn id(&self) -> &'static str {
        EXA_PROVIDER_ID
    }

    fn available(&self) -> bool {
        !self.api_key.is_empty() && reqwest::Url::parse(&self.endpoint).is_ok()
    }
}

impl SearchProviderSpec for ExaSearchProvider {
    async fn search(
        &self,
        request: &WebSearchRequest,
        cancel: &CancelToken,
    ) -> Result<WebSearchResult, WebError> {
        if self.api_key.is_empty() {
            return Err(WebError::new(
                WEB_PROVIDER_CREDENTIAL_MISSING,
                "Exa search has no API key; store one in Settings > Tools > Web search",
            ));
        }

        let wanted = request
            .max_results
            .unwrap_or(self.request_results)
            .min(EXA_MAX_RESULTS)
            .max(1);
        let payload = json!({
            "query": request.query,
            "numResults": wanted,
            "contents": {"text": {"maxCharacters": self.snippet_chars}}
        });

        let response = race_cancel(
            self.client
                .post(&self.endpoint)
                .header("x-api-key", &self.api_key)
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .body(serde_json::to_string(&payload).unwrap_or_default())
                .send(),
            cancel,
        )
        .await
        .map_err(|error| self.dispatch_error(error, "Exa search request failed"))?;

        let response = response
            .map_err(|error| self.endpoint_error(format!("Exa search request failed: {error}")))?;
        let status = response.status();
        let body = race_cancel(response.text(), cancel)
            .await
            .map_err(|error| self.dispatch_error(error, "Exa search response failed"))?
            .map_err(|error| self.endpoint_error(format!("Exa search response failed: {error}")))?;

        if !status.is_success() {
            return Err(self.endpoint_error(format!(
                "Exa API error (HTTP {}): {}",
                status.as_u16(),
                body.chars().take(400).collect::<String>()
            )));
        }

        map_exa_response(&body, self.snippet_chars)
            .map_err(|error| self.endpoint_error(error.to_string()))
    }
}

impl ExaSearchProvider {
    /// A dispatch or response-body failure. An aborted turn keeps `WEB_ABORTED` rather
    /// than being re-tagged as a provider error, matching the HTTP fetch provider.
    fn dispatch_error(&self, error: WebError, context: &str) -> WebError {
        if error.code() == WEB_ABORTED {
            error
        } else {
            self.endpoint_error(format!("{context}: {error}"))
        }
    }

    fn endpoint_error(&self, message: impl std::fmt::Display) -> WebError {
        WebError::new(
            WEB_PROVIDER_ERROR,
            format!(
                "{message}\n\nThe web search request used endpoint {}; change it in Settings > Tools > Web search.",
                self.endpoint
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_results_and_trims_snippets() {
        let body = json!({
            "requestId": "req_1",
            "results": [
                {"title": "Alpha", "url": "https://a.example/x", "publishedDate": "2026-01-02", "text": "one   two three"},
                {"url": "https://a.example/x", "text": "duplicate"},
                {"title": "", "url": "https://b.example/y", "text": "abcdefghijklmnop"}
            ]
        })
        .to_string();

        let result = map_exa_response(&body, 8).expect("mapped");
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.sources[0].snippet.as_deref(), Some("one two…"));
        assert_eq!(
            result.sources[0].published_at.as_deref(),
            Some("2026-01-02")
        );
        assert!(result.sources[1].title.is_none());
        assert_eq!(result.sources[1].snippet.as_deref(), Some("abcdefg…"));
    }

    #[test]
    fn surfaces_provider_errors() {
        let error = map_exa_response(r#"{"error":"invalid api key"}"#, 100).expect_err("error");
        assert_eq!(error.code(), WEB_PROVIDER_ERROR);
        assert!(error.message().contains("invalid api key"));
    }

    #[test]
    fn keeps_the_abort_code_on_a_cancelled_dispatch() {
        let provider = ExaSearchProvider::new(EXA_DEFAULT_BASE_URL, "exa-test", 1200, 5);
        let error = provider.dispatch_error(WebError::aborted(), "Exa search request failed");
        assert_eq!(error.code(), WEB_ABORTED);
    }
}
