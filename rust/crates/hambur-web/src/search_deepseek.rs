//! DeepSeek search through an Anthropic-compatible Messages call with the native
//! `web_search_20250305` server tool — the same wire protocol DeepSeek Harness uses in
//! `@deepseek-ai/dsh-web-search-deepseek`.
//!
//! Each search costs a model turn but returns structured result blocks; the absence of
//! those blocks is an error rather than a prose-scraping fallback. The endpoint is
//! deliberately independent of the conversation model's endpoint so search can be pointed
//! at a different (for example domestic) host without changing chat.

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
pub const DEEPSEEK_PROVIDER_ID: &str = "deepseek-official";
/// Default auxiliary-search endpoint, including `/v1`; `/messages` is appended.
pub const DEEPSEEK_DEFAULT_BASE_URL: &str = "https://api.deepseek.com/anthropic/v1";
/// Default Anthropic-format model name used only for the auxiliary search turn.
pub const DEEPSEEK_DEFAULT_MODEL: &str = "deepseek-chat";
/// Default `anthropic-version` header value.
pub const DEEPSEEK_DEFAULT_API_VERSION: &str = "2023-06-01";
/// Default upper bound on generated tokens for the Messages request.
pub const DEEPSEEK_DEFAULT_MAX_TOKENS: u32 = 4096;
/// Default maximum `web_search` server-tool uses per request.
pub const DEEPSEEK_DEFAULT_MAX_USES: u32 = 5;

const USER_AGENT: &str = "hambur/0.1 (+web-search-deepseek)";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// A `web_search_result` item inside a `web_search_tool_result` block.
#[derive(Debug, Clone, Deserialize)]
struct ResultItem {
    #[serde(default, rename = "type")]
    item_type: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    page_age: Option<String>,
}

/// The `web_search_tool_result_error` object the wire protocol may put in `content`
/// instead of a result array (for example `max_uses_exceeded`).
#[derive(Debug, Clone, Deserialize)]
struct ResultError {
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

/// `web_search_tool_result.content` is either the list of results or a single error object.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ToolResultContent {
    Results(Vec<ResultItem>),
    Error(ResultError),
}

/// One citation location inside a `text` block (the snippet source).
#[derive(Debug, Clone, Deserialize)]
struct Citation {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    cited_text: Option<String>,
}

/// Any content block; only `web_search_tool_result` and `text` are consumed.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult {
        #[serde(default)]
        content: Option<ToolResultContent>,
    },
    #[serde(rename = "text")]
    Text {
        #[serde(default)]
        citations: Vec<Citation>,
    },
    #[serde(other)]
    Other,
}

/// DeepSeek's Anthropic Messages response envelope.
#[derive(Debug, Clone, Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<ContentBlock>,
}

/// Build a `url → cited_text` map from every `text` block's `citations[]`.
///
/// This is the snippet source: `web_search_result` items carry `url`/`title`/`page_age`
/// but typically no inline snippet — the excerpt lives in a separate `text` block's
/// citation, keyed by URL (first occurrence wins).
fn citation_snippets(blocks: &[ContentBlock]) -> std::collections::HashMap<String, String> {
    let mut snippets = std::collections::HashMap::new();
    for block in blocks {
        let ContentBlock::Text { citations } = block else {
            continue;
        };
        for citation in citations {
            let Some(url) = citation
                .url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let Some(text) = citation
                .cited_text
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            snippets
                .entry(url.to_string())
                .or_insert_with(|| collapse_whitespace(text));
        }
    }
    snippets
}

/// Map a DeepSeek Anthropic Messages response to a normalized search result. Walks
/// `web_search_tool_result` blocks for citable items, joins each to its citation excerpt
/// as `snippet`, and dedupes by URL (a `max_uses > 1` request can surface the same URL
/// across searches).
pub fn map_anthropic_response(response: &str) -> Result<WebSearchResult, WebError> {
    let parsed: AnthropicResponse = serde_json::from_str(response).map_err(|error| {
        WebError::new(
            WEB_PROVIDER_ERROR,
            format!("DeepSeek returned an unprocessable response body: {error}"),
        )
    })?;
    let result_blocks: Vec<&ContentBlock> = parsed
        .content
        .iter()
        .filter(|block| matches!(block, ContentBlock::WebSearchToolResult { .. }))
        .collect();
    if result_blocks.is_empty() {
        return Err(WebError::new(
            WEB_PROVIDER_ERROR,
            "DeepSeek returned no web_search_tool_result blocks; the request may not have triggered native web search",
        ));
    }

    let snippets = citation_snippets(&parsed.content);
    let mut seen = std::collections::HashSet::new();
    let mut sources = Vec::new();
    let mut provider_error: Option<String> = None;
    for block in result_blocks {
        let ContentBlock::WebSearchToolResult { content } = block else {
            continue;
        };
        match content {
            Some(ToolResultContent::Results(items)) => {
                for item in items {
                    if item.item_type != "web_search_result" {
                        continue;
                    }
                    let url = item.url.trim();
                    if url.is_empty() || !seen.insert(url.to_string()) {
                        continue;
                    }
                    sources.push(WebSearchSource {
                        url: url.to_string(),
                        title: non_empty(item.title.as_deref()),
                        snippet: snippets.get(url).cloned().filter(|value| !value.is_empty()),
                        published_at: non_empty(item.page_age.as_deref()),
                    });
                }
            }
            Some(ToolResultContent::Error(error)) => {
                // Keep the provider's own code/message instead of masking it as a parse
                // failure; still prefer real results if the same response carried any.
                provider_error.get_or_insert_with(|| describe_result_error(error));
            }
            None => {}
        }
    }

    if sources.is_empty()
        && let Some(detail) = provider_error
    {
        return Err(WebError::new(
            WEB_PROVIDER_ERROR,
            format!("DeepSeek web search failed: {detail}"),
        ));
    }

    Ok(WebSearchResult {
        content: None,
        sources,
        truncated: false,
    })
}

/// Render a `web_search_tool_result_error` object for the user.
fn describe_result_error(error: &ResultError) -> String {
    match (
        non_empty(error.error_code.as_deref()),
        non_empty(error.message.as_deref()),
    ) {
        (Some(code), Some(message)) => format!("{code}: {message}"),
        (Some(code), None) => code,
        (None, Some(message)) => message,
        (None, None) => "the provider returned an unspecified web search error".to_string(),
    }
}

/// Extract the provider's message from an error envelope (best effort; fields vary).
pub fn error_message(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if let Some(message) = value.get("error") {
        if let Some(message) = message.as_str() {
            return Some(message.to_string());
        }
        if let Some(message) = message.get("message").and_then(serde_json::Value::as_str) {
            return Some(message.to_string());
        }
    }
    value
        .get("message")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The DeepSeek-backed search provider.
#[derive(Clone)]
pub struct DeepSeekSearchProvider {
    client: reqwest::Client,
    /// Fully resolved Messages endpoint (`.../messages`).
    endpoint: String,
    api_key: String,
    model: String,
    api_version: String,
    max_tokens: u32,
    max_uses: u32,
}

impl DeepSeekSearchProvider {
    /// Build a provider for one operation from already-resolved options.
    ///
    /// `base_url` may include a trailing slash; `/messages` is appended once.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        api_version: &str,
        max_tokens: u32,
        max_uses: u32,
    ) -> Self {
        let base = base_url.trim().trim_end_matches('/');
        let endpoint = format!("{base}/messages");
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
            model: model.trim().to_string(),
            api_version: api_version.trim().to_string(),
            max_tokens,
            max_uses,
        }
    }

    /// The endpoint this provider will dispatch to (for error messages and diagnostics).
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    fn body(&self, query: &str) -> serde_json::Value {
        json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "text",
                    "text": format!("Perform a web search for the query: {query}")
                }]
            }],
            "tools": [{
                "type": "web_search_20250305",
                "name": "web_search",
                "max_uses": self.max_uses
            }]
        })
    }
}

impl ProviderId for DeepSeekSearchProvider {
    fn id(&self) -> &'static str {
        DEEPSEEK_PROVIDER_ID
    }

    fn available(&self) -> bool {
        !self.api_key.is_empty()
            && !self.model.is_empty()
            && reqwest::Url::parse(&self.endpoint).is_ok()
            && self.max_tokens > 0
            && self.max_uses > 0
    }
}

impl SearchProviderSpec for DeepSeekSearchProvider {
    async fn search(
        &self,
        request: &WebSearchRequest,
        cancel: &CancelToken,
    ) -> Result<WebSearchResult, WebError> {
        if self.api_key.is_empty() {
            return Err(WebError::new(
                WEB_PROVIDER_CREDENTIAL_MISSING,
                "DeepSeek search has no API key; store one in Settings > Tools > Web search, or reuse the DeepSeek provider credential",
            ));
        }

        let response = race_cancel(
            self.client
                .post(&self.endpoint)
                .header("x-api-key", &self.api_key)
                .header("authorization", format!("Bearer {}", self.api_key))
                .header("anthropic-version", &self.api_version)
                .header("content-type", "application/json")
                .header("accept", "application/json")
                .body(serde_json::to_string(&self.body(&request.query)).unwrap_or_default())
                .send(),
            cancel,
        )
        .await
        .map_err(|error| self.dispatch_error(error, "DeepSeek search request failed"))?;

        let response = response.map_err(|error| {
            self.endpoint_error(format!("DeepSeek search request failed: {error}"))
        })?;
        let status = response.status();
        let body = race_cancel(response.text(), cancel)
            .await
            .map_err(|error| self.dispatch_error(error, "DeepSeek search response failed"))?
            .map_err(|error| {
                self.endpoint_error(format!("DeepSeek search response failed: {error}"))
            })?;

        if !status.is_success() {
            let detail = error_message(&body)
                .map(|message| format!(": {message}"))
                .unwrap_or_default();
            return Err(self.endpoint_error(format!(
                "DeepSeek API error (HTTP {}){detail}",
                status.as_u16()
            )));
        }

        map_anthropic_response(&body).map_err(|error| self.endpoint_error(error.to_string()))
    }
}

impl DeepSeekSearchProvider {
    /// A dispatch or response-body failure. An aborted turn keeps `WEB_ABORTED` rather
    /// than being re-tagged as a provider error, matching the HTTP fetch provider.
    fn dispatch_error(&self, error: WebError, context: &str) -> WebError {
        if error.code() == WEB_ABORTED {
            error
        } else {
            self.endpoint_error(format!("{context}: {error}"))
        }
    }

    /// Add endpoint recovery guidance to failures after dispatch begins.
    fn endpoint_error(&self, message: impl std::fmt::Display) -> WebError {
        WebError::new(
            WEB_PROVIDER_ERROR,
            format!(
                "{message}\n\nThe web search request used endpoint {}. Search endpoint configuration is separate from chat; change it in Settings > Tools > Web search.",
                self.endpoint
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_result_blocks_with_citation_snippets() {
        let body = json!({
            "content": [
                {"type": "text", "text": "thinking", "citations": [
                    {"type": "web_search_result_location", "url": "https://a.example/x", "cited_text": "Alpha   snippet"},
                    {"type": "web_search_result_location", "url": "https://a.example/x", "cited_text": "later duplicate"}
                ]},
                {"type": "web_search_tool_result", "content": [
                    {"type": "web_search_result", "url": "https://a.example/x", "title": "Alpha", "page_age": "2026-01-02"},
                    {"type": "web_search_result", "url": "https://b.example/y", "title": null},
                    {"type": "web_search_result", "url": "https://a.example/x", "title": "Alpha duplicate"}
                ]}
            ]
        })
        .to_string();

        let result = map_anthropic_response(&body).expect("mapped");
        assert_eq!(result.sources.len(), 2);
        assert_eq!(result.sources[0].url, "https://a.example/x");
        assert_eq!(result.sources[0].title.as_deref(), Some("Alpha"));
        assert_eq!(result.sources[0].snippet.as_deref(), Some("Alpha snippet"));
        assert_eq!(
            result.sources[0].published_at.as_deref(),
            Some("2026-01-02")
        );
        assert_eq!(result.sources[1].url, "https://b.example/y");
        assert!(result.sources[1].title.is_none());
        assert!(!result.truncated);
    }

    #[test]
    fn a_response_without_result_blocks_is_an_error() {
        let body = json!({"content": [{"type": "text", "text": "no search happened"}]}).to_string();
        let error = map_anthropic_response(&body).expect_err("error");
        assert_eq!(error.code(), WEB_PROVIDER_ERROR);
    }

    #[test]
    fn unavailable_without_an_api_key() {
        let provider = DeepSeekSearchProvider::new(
            DEEPSEEK_DEFAULT_BASE_URL,
            "",
            DEEPSEEK_DEFAULT_MODEL,
            DEEPSEEK_DEFAULT_API_VERSION,
            DEEPSEEK_DEFAULT_MAX_TOKENS,
            DEEPSEEK_DEFAULT_MAX_USES,
        );
        assert!(!provider.available());
        assert_eq!(
            provider.endpoint(),
            "https://api.deepseek.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn surfaces_the_provider_error_object() {
        // The wire protocol may put a single error object in `content` instead of a
        // result array; that must surface as a provider error, not a parse failure.
        let body = json!({
            "content": [{
                "type": "web_search_tool_result",
                "content": {
                    "type": "web_search_tool_result_error",
                    "error_code": "max_uses_exceeded",
                    "message": "too many searches"
                }
            }]
        })
        .to_string();

        let error = map_anthropic_response(&body).expect_err("provider error");
        assert_eq!(error.code(), WEB_PROVIDER_ERROR);
        assert!(error.message().contains("max_uses_exceeded"), "{error}");
        assert!(error.message().contains("too many searches"), "{error}");
    }

    #[test]
    fn tolerates_a_malformed_result_item() {
        let body = json!({
            "content": [{
                "type": "web_search_tool_result",
                "content": [
                    {"type": "web_search_result"},
                    {"type": "web_search_result", "url": "https://a.example/x", "title": "Alpha"}
                ]
            }]
        })
        .to_string();

        let result = map_anthropic_response(&body).expect("mapped");
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].url, "https://a.example/x");
    }

    #[test]
    fn keeps_the_abort_code_on_a_cancelled_dispatch() {
        let provider = DeepSeekSearchProvider::new(
            DEEPSEEK_DEFAULT_BASE_URL,
            "sk-test",
            DEEPSEEK_DEFAULT_MODEL,
            DEEPSEEK_DEFAULT_API_VERSION,
            DEEPSEEK_DEFAULT_MAX_TOKENS,
            DEEPSEEK_DEFAULT_MAX_USES,
        );
        let error = provider.dispatch_error(WebError::aborted(), "DeepSeek search request failed");
        assert_eq!(error.code(), WEB_ABORTED);
    }

    #[test]
    fn reads_error_envelopes() {
        assert_eq!(
            error_message(r#"{"error":{"message":"bad key"}}"#).as_deref(),
            Some("bad key")
        );
        assert_eq!(
            error_message(r#"{"error":"bad key"}"#).as_deref(),
            Some("bad key")
        );
        assert_eq!(error_message("not json"), None);
    }
}
