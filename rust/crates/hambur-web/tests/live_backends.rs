//! Live checks against the real backends. Ignored by default so the offline suite stays
//! hermetic; run them with a credential in the environment:
//!
//! ```text
//! HAMBUR_WEB_LIVE_KEY=sk-… cargo test -p hambur-web --test live_backends -- --ignored --nocapture
//! ```
//!
//! `HAMBUR_WEB_LIVE_KEY` is the DeepSeek key used for the auxiliary search call;
//! `HAMBUR_WEB_LIVE_MODEL` optionally overrides the model name.

use hambur_web::fetch_http::FetchPolicy;
use hambur_web::{
    CancelToken, DEEPSEEK_DEFAULT_API_VERSION, DEEPSEEK_DEFAULT_BASE_URL,
    DEEPSEEK_DEFAULT_MAX_TOKENS, DEEPSEEK_DEFAULT_MODEL, DeepSeekSearchProvider, FetchProviderSpec,
    HttpFetchProvider, ProviderId, SearchProviderSpec, WEB_BLOCKED_URL, WebFetchRequest,
    WebFetchResult, WebSearchRequest,
};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

fn live_key() -> Option<String> {
    std::env::var("HAMBUR_WEB_LIVE_KEY")
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn live_model() -> String {
    std::env::var("HAMBUR_WEB_LIVE_MODEL")
        .ok()
        .filter(|model| !model.trim().is_empty())
        .unwrap_or_else(|| DEEPSEEK_DEFAULT_MODEL.to_string())
}

#[test]
#[ignore = "hits the live DeepSeek API; set HAMBUR_WEB_LIVE_KEY to run"]
fn deepseek_search_returns_citable_sources() {
    let Some(api_key) = live_key() else {
        eprintln!("HAMBUR_WEB_LIVE_KEY is not set; skipping");
        return;
    };
    let provider = DeepSeekSearchProvider::new(
        DEEPSEEK_DEFAULT_BASE_URL,
        &api_key,
        &live_model(),
        DEEPSEEK_DEFAULT_API_VERSION,
        DEEPSEEK_DEFAULT_MAX_TOKENS,
        3,
    );
    assert!(provider.available());

    let result = block_on(provider.search(
        &WebSearchRequest {
            query: "Rust 2024 edition release".to_string(),
            max_results: Some(5),
        },
        &CancelToken::never(),
    ))
    .expect("live search");

    println!("sources: {}", result.sources.len());
    for source in result.sources.iter().take(3) {
        println!(
            "- {} | {} | {:?}",
            source.title.as_deref().unwrap_or("<no title>"),
            source.url,
            source.published_at
        );
    }
    assert!(
        !result.sources.is_empty(),
        "live search returned no sources"
    );
    assert!(
        result
            .sources
            .iter()
            .all(|source| source.url.starts_with("http")),
        "every source must carry a URL"
    );
}

#[test]
#[ignore = "hits the public internet"]
fn http_fetch_renders_a_public_page_as_markdown() {
    let url = std::env::var("HAMBUR_WEB_LIVE_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| "https://example.com/".to_string());
    let provider = HttpFetchProvider::new(FetchPolicy {
        timeout: std::time::Duration::from_secs(20),
        ..FetchPolicy::default()
    });
    let result: WebFetchResult =
        block_on(provider.fetch(&WebFetchRequest { url: url.clone() }, &CancelToken::never()))
            .expect("live fetch");

    println!(
        "status: {} content-type: {}",
        result.status_code, result.content_type
    );
    assert_eq!(result.status_code, 200);

    let markdown = hambur_web::format_fetch_output(&result, 4_000);
    println!("--- markdown ---\n{markdown}\n--- end ---");
    assert!(!markdown.trim().is_empty(), "the render must not be empty");
    assert!(
        !markdown.contains("<style")
            && !markdown.contains("<script")
            && !markdown.contains("color-scheme"),
        "markup and styles must not survive the render: {markdown}"
    );
}

#[test]
fn the_local_fetch_policy_refuses_private_hosts_without_the_network() {
    let provider = HttpFetchProvider::new(FetchPolicy::default());
    let error = block_on(provider.fetch(
        &WebFetchRequest {
            url: "http://127.0.0.1:8080/".to_string(),
        },
        &CancelToken::never(),
    ))
    .expect_err("blocked");
    assert_eq!(error.code(), WEB_BLOCKED_URL);
}
