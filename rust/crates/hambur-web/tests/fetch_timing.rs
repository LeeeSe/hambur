//! Timing probe for the local HTTP fetch provider, kept out of the hermetic suite.
//!
//! Run it to compare Hambur's `web_fetch` path against a plain keep-alive `reqwest` client on
//! the same machine:
//!
//! ```text
//! cargo test -p hambur-web --test fetch_timing -- --ignored --nocapture
//! ```
//!
//! `HAMBUR_FETCH_TIMING_URLS` (comma separated) overrides the probe targets.

use std::time::{Duration, Instant};

use hambur_web::fetch_http::FetchPolicy;
use hambur_web::{CancelToken, FetchProviderSpec, HttpFetchProvider, WebFetchRequest};

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(future)
}

fn targets() -> Vec<String> {
    let raw = std::env::var("HAMBUR_FETCH_TIMING_URLS").unwrap_or_default();
    let parsed: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .collect();
    if parsed.is_empty() {
        return vec![
            "https://example.com/".to_string(),
            "https://docs.rs/crate/adb-kit/latest".to_string(),
            "https://doc.rust-lang.org/std/process/struct.Command.html".to_string(),
        ];
    }
    parsed
}

/// DNS work the provider does explicitly before every hop, measured on its own.
async fn dns_ms(host: &str) -> u128 {
    let started = Instant::now();
    let resolved = tokio::net::lookup_host((host, 443)).await;
    match resolved {
        Ok(addresses) => {
            let count = addresses.count();
            println!(
                "    dns {host}: {} ms ({count} addresses)",
                started.elapsed().as_millis()
            );
        }
        Err(error) => println!(
            "    dns {host}: failed after {} ms ({error})",
            started.elapsed().as_millis()
        ),
    }
    started.elapsed().as_millis()
}

/// Character count of a fetched body, whatever representation it took.
fn body_len(body: &hambur_web::WebFetchBody) -> usize {
    match body {
        hambur_web::WebFetchBody::Html(text) | hambur_web::WebFetchBody::Text(text) => {
            text.chars().count()
        }
    }
}

#[test]
#[ignore = "hits the public internet"]
fn local_fetch_provider_timings() {
    let urls = targets();
    let provider = HttpFetchProvider::new(FetchPolicy {
        timeout: Duration::from_secs(20),
        ..FetchPolicy::default()
    });

    block_on(async {
        // One warm-up so a cold TLS session ticket cache does not dominate the first sample.
        let warm = provider
            .fetch(
                &WebFetchRequest {
                    url: urls[0].clone(),
                },
                &CancelToken::never(),
            )
            .await;
        println!("warm-up: {:?}\n", warm.map(|result| result.status_code));

        for url in &urls {
            let host = reqwest::Url::parse(url)
                .ok()
                .and_then(|parsed| parsed.host_str().map(str::to_string))
                .unwrap_or_default();
            println!("target {url}");
            let _ = dns_ms(&host).await;

            for attempt in 1..=3 {
                let started = Instant::now();
                let outcome = provider
                    .fetch(&WebFetchRequest { url: url.clone() }, &CancelToken::never())
                    .await;
                let elapsed = started.elapsed();
                match outcome {
                    Ok(result) => println!(
                        "  hambur web_fetch #{attempt}: {} ms (status {}, {} bytes, truncated {})",
                        elapsed.as_millis(),
                        result.status_code,
                        body_len(&result.body),
                        result.truncated
                    ),
                    Err(error) => println!(
                        "  hambur web_fetch #{attempt}: {} ms (error {})",
                        elapsed.as_millis(),
                        error
                    ),
                }
            }
        }
    });
}

#[test]
#[ignore = "hits the public internet"]
fn render_after_fetch_timings() {
    let urls = targets();
    let provider = HttpFetchProvider::new(FetchPolicy {
        timeout: Duration::from_secs(20),
        ..FetchPolicy::default()
    });

    block_on(async {
        for url in &urls {
            let started = Instant::now();
            let outcome = provider
                .fetch(&WebFetchRequest { url: url.clone() }, &CancelToken::never())
                .await;
            let fetched = started.elapsed();
            let result = match outcome {
                Ok(result) => result,
                Err(error) => {
                    println!(
                        "{url}: fetch failed after {} ms ({error})",
                        fetched.as_millis()
                    );
                    continue;
                }
            };
            let rendered_started = Instant::now();
            let markdown = hambur_web::format_fetch_output(&result, 48_000);
            let rendered = rendered_started.elapsed();
            println!(
                "{}: fetch {} ms, html->markdown {} ms ({} chars out)",
                url,
                fetched.as_millis(),
                rendered.as_millis(),
                markdown.chars().count()
            );
        }
    });
}

#[test]
#[ignore = "hits the public internet"]
fn bare_reqwest_baseline_timings() {
    let urls = targets();
    block_on(async {
        // One shared client with a connection pool, which is what a normal HTTP client does.
        let shared = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("client");

        for url in &urls {
            println!("target {url}");
            for attempt in 1..=3 {
                let started = Instant::now();
                let outcome = shared.get(url).send().await;
                let elapsed = started.elapsed();
                match outcome {
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        println!(
                            "  shared-client reqwest #{attempt}: {} ms (status {}, {} bytes)",
                            elapsed.as_millis(),
                            status,
                            body.len()
                        );
                    }
                    Err(error) => println!(
                        "  shared-client reqwest #{attempt}: {} ms (error {error})",
                        elapsed.as_millis()
                    ),
                }
            }
        }

        for url in &urls {
            println!("target {url}");
            for attempt in 1..=3 {
                // A fresh client per request mirrors what the provider does on every hop.
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(20))
                    .build()
                    .expect("client");
                let started = Instant::now();
                let outcome = client.get(url).send().await;
                let elapsed = started.elapsed();
                match outcome {
                    Ok(response) => {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        println!(
                            "  fresh-client reqwest #{attempt}: {} ms (status {}, {} bytes)",
                            elapsed.as_millis(),
                            status,
                            body.len()
                        );
                    }
                    Err(error) => println!(
                        "  fresh-client reqwest #{attempt}: {} ms (error {error})",
                        elapsed.as_millis()
                    ),
                }
            }
        }
    });
}
