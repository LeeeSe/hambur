//! The model-facing half of the web tools: argument validation, the multi-query merge, and
//! the text the model reads.
//!
//! These functions mirror DeepSeek Harness' `@deepseek-ai/dsh-tool-web` renderers so a
//! prompt tuned against DSH keeps working here.

use serde_json::Value;

use crate::html::html_to_markdown;
use crate::types::{
    WEB_INVALID_ARGUMENTS, WebError, WebFetchBody, WebFetchResult, WebSearchResult, WebSearchSource,
};

/// Footer appended when a fetch render hits the deployment's output cap.
pub const TRUNCATION_FOOTER: &str =
    "\n\n(Content truncated. Fetch a more specific URL or section for the full text.)";

/// Upper bound on URLs accepted in one `web_fetch` call.
pub const WEB_FETCH_MAX_URLS: usize = 5;

/// Validate the `web_fetch` arguments: one `url` string and/or a `urls` array, 1..=`max_urls`
/// distinct absolute URLs in first-occurrence order.
///
/// `url` is the DeepSeek Harness shape and `urls` the multi-page shape this app promises;
/// accepting both keeps single-page calls short without losing concurrent fetches.
pub fn parse_fetch_urls(arguments: &Value, max_urls: usize) -> Result<Vec<String>, WebError> {
    let mut urls: Vec<String> = Vec::new();
    match arguments.get("url") {
        Some(Value::String(value)) => urls.push(value.trim().to_string()),
        Some(_) => {
            return Err(WebError::new(
                WEB_INVALID_ARGUMENTS,
                "\"url\" must be a string",
            ));
        }
        None => {}
    }
    match arguments.get("urls") {
        Some(Value::Array(values)) => {
            for value in values {
                let url = value.as_str().ok_or_else(|| {
                    WebError::new(
                        WEB_INVALID_ARGUMENTS,
                        "every entry of \"urls\" must be a string",
                    )
                })?;
                urls.push(url.trim().to_string());
            }
        }
        Some(_) => {
            return Err(WebError::new(
                WEB_INVALID_ARGUMENTS,
                "\"urls\" must be an array of strings",
            ));
        }
        None => {}
    }

    if urls.is_empty() {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            "pass \"url\" or a non-empty \"urls\" array",
        ));
    }
    if urls.iter().any(|url| url.is_empty()) {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            "every URL must be a non-empty string",
        ));
    }

    let mut seen = std::collections::HashSet::new();
    let urls: Vec<String> = urls
        .into_iter()
        .filter(|url| seen.insert(url.clone()))
        .collect();
    if urls.len() > max_urls {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            format!("at most {max_urls} URLs may be fetched in one call"),
        ));
    }
    Ok(urls)
}

/// Validate the `web_search` arguments: one or more non-blank queries within the bound.
///
/// `queries` is the canonical shape; a bare `query` string is still accepted so a session
/// whose tool schema predates the multi-query form keeps working. Duplicates are collapsed
/// after the bound check, in first-occurrence order.
pub fn parse_search_queries(
    arguments: &Value,
    max_queries: usize,
) -> Result<Vec<String>, WebError> {
    let mut queries: Vec<String> = Vec::new();
    match arguments.get("queries") {
        Some(Value::Array(values)) => {
            for value in values {
                let query = value.as_str().ok_or_else(|| {
                    WebError::new(
                        WEB_INVALID_ARGUMENTS,
                        "every entry of \"queries\" must be a string",
                    )
                })?;
                queries.push(query.trim().to_string());
            }
        }
        Some(Value::String(value)) => queries.push(value.trim().to_string()),
        Some(_) => {
            return Err(WebError::new(
                WEB_INVALID_ARGUMENTS,
                "\"queries\" must be an array of strings",
            ));
        }
        None => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string();
            queries.push(query);
        }
    }

    if queries.is_empty() {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            "queries must contain at least one query",
        ));
    }
    if queries.len() > max_queries {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            format!("queries must contain at most {max_queries} queries"),
        ));
    }
    if queries.iter().any(|query| query.is_empty()) {
        return Err(WebError::new(
            WEB_INVALID_ARGUMENTS,
            "each query must be a non-empty string",
        ));
    }

    let mut seen = std::collections::HashSet::new();
    Ok(queries
        .into_iter()
        .filter(|query| seen.insert(query.clone()))
        .collect())
}

/// Merge per-query results into one deduplicated, round-robin, capped result.
///
/// Round-robin matters: taking each query's best hit before any query's second hit keeps
/// every phrasing represented when the cap bites.
pub fn merge_search_results(results: Vec<WebSearchResult>, max_results: usize) -> WebSearchResult {
    let source_ranks = results
        .iter()
        .map(|result| result.sources.len())
        .max()
        .unwrap_or(0);
    let mut seen = std::collections::HashSet::new();
    let mut sources: Vec<WebSearchSource> = Vec::new();
    let mut dropped_source = false;
    'outer: for rank in 0..source_ranks {
        for result in &results {
            let Some(source) = result.sources.get(rank) else {
                continue;
            };
            if !seen.insert(source.url.clone()) {
                continue;
            }
            if sources.len() == max_results {
                dropped_source = true;
                break 'outer;
            }
            sources.push(source.clone());
        }
    }

    let contents: Vec<String> = results
        .iter()
        .filter_map(|result| {
            result
                .content
                .as_deref()
                .filter(|content| !content.is_empty())
        })
        .map(str::to_string)
        .collect();

    WebSearchResult {
        content: if contents.is_empty() {
            None
        } else {
            Some(contents.join("\n\n"))
        },
        sources,
        truncated: results.iter().any(|result| result.truncated) || dropped_source,
    }
}

/// Format a merged search result as one model-facing text block.
pub fn format_search_output(result: &WebSearchResult) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(content) = result
        .content
        .as_deref()
        .filter(|content| !content.is_empty())
    {
        parts.push(content.to_string());
    }
    if !result.sources.is_empty() {
        let lines: Vec<String> = result
            .sources
            .iter()
            .map(|source| {
                let label = source_label(source);
                let mut meta: Vec<String> = Vec::new();
                if let Some(snippet) = source.snippet.as_deref().filter(|value| !value.is_empty()) {
                    meta.push(snippet.to_string());
                }
                if let Some(published) = source
                    .published_at
                    .as_deref()
                    .filter(|value| !value.is_empty())
                {
                    meta.push(format!("({published})"));
                }
                let suffix = if meta.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", meta.join(" "))
                };
                format!("- [{}]({}){}", label, source.url, suffix)
            })
            .collect();
        parts.push(format!("Sources:\n{}", lines.join("\n")));
    } else if result.content.is_none() {
        parts.push("No results found.".to_string());
    }
    if result.truncated {
        parts.push(format!(
            "(Showing the first {} sources. Refine the query for more.)",
            result.sources.len()
        ));
    }
    parts.push("Cite the relevant URLs above as markdown links in your answer.".to_string());
    parts.join("\n\n")
}

/// The display label for one source: its title, else its hostname, else the URL.
pub fn source_label(source: &WebSearchSource) -> String {
    if let Some(title) = source
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        return title.to_string();
    }
    reqwest::Url::parse(&source.url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| source.url.clone())
}

/// Format a fetch result as one model-facing text block, bounded as a whole.
pub fn format_fetch_output(result: &WebFetchResult, max_chars: usize) -> String {
    let header = format!("Fetched {} (HTTP {})\n\n", result.url, result.status_code);
    let body = match &result.body {
        WebFetchBody::Html(content) => html_to_markdown(content),
        WebFetchBody::Text(content) => content.clone(),
    };
    let prefix = format!("{header}{body}");
    let truncated = result.truncated || prefix.chars().count() > max_chars;

    if !truncated {
        return prefix;
    }
    let footer_len = TRUNCATION_FOOTER.chars().count();
    if max_chars <= footer_len {
        return prefix.chars().take(max_chars).collect();
    }
    let mut text: String = prefix.chars().take(max_chars - footer_len).collect();
    text.push_str(TRUNCATION_FOOTER);
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn result(urls: &[&str]) -> WebSearchResult {
        WebSearchResult {
            content: None,
            sources: urls
                .iter()
                .map(|url| WebSearchSource {
                    url: (*url).to_string(),
                    title: Some(format!("Title {url}")),
                    ..Default::default()
                })
                .collect(),
            truncated: false,
        }
    }

    #[test]
    fn accepts_multi_query_and_legacy_single_query() {
        let queries = parse_search_queries(&json!({"queries": ["a", " b ", "a"]}), 4).expect("ok");
        assert_eq!(queries, vec!["a".to_string(), "b".to_string()]);

        let legacy = parse_search_queries(&json!({"query": "solo"}), 4).expect("ok");
        assert_eq!(legacy, vec!["solo".to_string()]);
    }

    #[test]
    fn rejects_blank_oversized_and_empty_query_lists() {
        assert_eq!(
            parse_search_queries(&json!({"queries": []}), 4)
                .expect_err("empty")
                .code(),
            WEB_INVALID_ARGUMENTS
        );
        assert!(parse_search_queries(&json!({"queries": ["a", "b", "c"]}), 2).is_err());
        assert!(parse_search_queries(&json!({"queries": ["a", "  "]}), 4).is_err());
        assert!(parse_search_queries(&json!({"queries": [1]}), 4).is_err());
    }

    #[test]
    fn merges_round_robin_and_caps() {
        let merged = merge_search_results(
            vec![
                result(&["https://a.example/1", "https://a.example/2"]),
                result(&["https://b.example/1", "https://b.example/2"]),
            ],
            3,
        );
        let urls: Vec<&str> = merged.sources.iter().map(|s| s.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://a.example/1",
                "https://b.example/1",
                "https://a.example/2"
            ]
        );
        assert!(merged.truncated);
    }

    #[test]
    fn accepts_one_url_or_a_bounded_list() {
        let single = parse_fetch_urls(&json!({"url": "https://a.example/x"}), 5).expect("ok");
        assert_eq!(single, vec!["https://a.example/x".to_string()]);

        let many = parse_fetch_urls(
            &json!({"urls": ["https://a.example/1", "https://b.example/2", "https://a.example/1"]}),
            5,
        )
        .expect("ok");
        assert_eq!(
            many,
            vec![
                "https://a.example/1".to_string(),
                "https://b.example/2".to_string()
            ]
        );

        let both = parse_fetch_urls(
            &json!({"url": "https://a.example/x", "urls": ["https://b.example/y"]}),
            5,
        )
        .expect("ok");
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn rejects_empty_or_oversized_fetch_url_lists() {
        assert_eq!(
            parse_fetch_urls(&json!({}), 5).expect_err("empty").code(),
            WEB_INVALID_ARGUMENTS
        );
        assert!(parse_fetch_urls(&json!({"url": "  "}), 5).is_err());
        assert!(parse_fetch_urls(&json!({"urls": [1]}), 5).is_err());
        assert!(parse_fetch_urls(&json!({"url": 7}), 5).is_err());
        assert!(
            parse_fetch_urls(
                &json!({"urls": ["https://a/1", "https://a/2", "https://a/3"]}),
                2
            )
            .is_err()
        );
    }

    #[test]
    fn formats_sources_with_snippets_and_dates() {
        let mut source = result(&["https://a.example/x"]).sources.remove(0);
        source.title = None;
        source.snippet = Some("an excerpt".to_string());
        source.published_at = Some("2026-01-02".to_string());
        let formatted = format_search_output(&WebSearchResult {
            content: None,
            sources: vec![source],
            truncated: false,
        });
        assert!(
            formatted.contains("- [a.example](https://a.example/x) — an excerpt (2026-01-02)"),
            "{formatted}"
        );
        assert!(formatted.contains("Cite the relevant URLs"));
    }

    #[test]
    fn renders_fetch_output_and_truncates_at_the_cap() {
        let result = WebFetchResult {
            url: "https://a.example/x".to_string(),
            status_code: 200,
            body: WebFetchBody::Html("<h1>Hi</h1><p>there</p>".to_string()),
            content_type: "text/html".to_string(),
            truncated: false,
        };
        let rendered = format_fetch_output(&result, 5_000);
        assert!(
            rendered.starts_with("Fetched https://a.example/x (HTTP 200)"),
            "{rendered}"
        );
        assert!(rendered.contains("# Hi"), "{rendered}");

        // A body longer than the cap keeps the cap, the head of the content, and the footer.
        let long = WebFetchResult {
            body: WebFetchBody::Text("x".repeat(5_000)),
            ..result.clone()
        };
        let footer_chars = TRUNCATION_FOOTER.chars().count();
        let cap = footer_chars + 120;
        let capped = format_fetch_output(&long, cap);
        assert_eq!(capped.chars().count(), cap);
        assert!(capped.ends_with(TRUNCATION_FOOTER), "{capped}");
        assert!(
            capped.starts_with("Fetched https://a.example/x (HTTP 200)"),
            "{capped}"
        );
    }
}
