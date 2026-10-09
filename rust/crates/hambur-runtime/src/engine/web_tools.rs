//! Runtime-hosted web tools: `web_search` and `web_fetch`.
//!
//! Both go through the `hambur-web` seam, which owns provider registration, selection, and
//! the result bound, so these functions only do what the DSH tool layer does: validate
//! arguments, resolve credentials, run the queries, and render the text a model reads.
//!
//! Secrets never enter the settings table: the search key is resolved from an explicit
//! credential reference, else reused from the configured official DeepSeek provider, else
//! from `$DEEPSEEK_API_KEY` in the launching environment.

use std::sync::Arc;

use hambur_web::{
    CancelToken, DEEPSEEK_PROVIDER_ID, EXA_PROVIDER_ID, WEB_FETCH_MAX_URLS, WEB_FETCH_SETTINGS_KEY,
    WEB_PROVIDER_ERROR, WEB_PROVIDER_UNAVAILABLE, WEB_SEARCH_SETTINGS_KEY, WebCredentials,
    WebError, WebFetchRequest, WebFetchSettings, WebSearchRequest, WebSearchResult,
    WebSearchSettings, build_runtime, format_fetch_output, format_search_output,
    merge_search_results, parse_fetch_urls, parse_search_queries, search_setup_error,
};
use tokio::task::JoinSet;

use crate::*;

/// Owner id attached to credential lookups made on behalf of the web tools.
const WEB_SEARCH_OWNER_ID: &str = "web_search";

/// Where `web_search` looks when nothing else supplies a DeepSeek search credential.
const DEEPSEEK_SEARCH_SECRET_REF: &str = "env://DEEPSEEK_API_KEY";

impl RuntimeEngine {
    /// Run one runtime-hosted web tool and normalize its result.
    pub(crate) async fn resolve_web_tool_result(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        if let Err(error) = self
            .tools
            .schemas()
            .validate_arguments(&invocation.name, arguments)
        {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                error.to_string(),
            );
        }

        let search_settings = self.web_search_settings();
        let fetch_settings = self.web_fetch_settings();
        let raw = match invocation.name.as_str() {
            "web_search" => {
                self.web_search_raw(invocation, arguments, &search_settings, &fetch_settings)
                    .await
            }
            "web_fetch" => {
                self.web_fetch_raw(invocation, arguments, &search_settings, &fetch_settings)
                    .await
            }
            other => web_raw(
                invocation,
                true,
                format!("unknown web tool: {other}"),
                "Unknown web tool".to_string(),
                "InvalidCommand",
                "trusted",
            ),
        };

        self.tools
            .normalize_raw_with_session(raw, Some(&invocation.session_id))
            .unwrap_or_else(|error| {
                ToolResult::failed(
                    &invocation.tool_call_id,
                    &invocation.name,
                    error.to_string(),
                )
            })
    }

    /// Stored `web_search_settings`, defaulted and clamped.
    fn web_search_settings(&self) -> WebSearchSettings {
        WebSearchSettings::from_json(&self.web_setting_value(WEB_SEARCH_SETTINGS_KEY))
    }

    /// Stored `web_fetch_settings`, defaulted and clamped.
    fn web_fetch_settings(&self) -> WebFetchSettings {
        WebFetchSettings::from_json(&self.web_setting_value(WEB_FETCH_SETTINGS_KEY))
    }

    fn web_setting_value(&self, key: &str) -> String {
        self.database
            .app_setting(key)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// The turn's cancel flag, so an in-flight HTTP call stops with the turn.
    fn web_cancel_flag(&self, session_id: &str) -> Arc<AtomicBool> {
        self.active_turn_for_session(session_id)
            .map(|active| active.cancel)
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)))
    }

    /// Resolve the credentials the seam's providers need, with per-provider reasons on
    /// failure so a missing key is reported as such rather than as "no provider".
    async fn web_credentials(
        &self,
        session_id: &str,
        turn_id: &str,
        search: &WebSearchSettings,
        cancel: &Arc<AtomicBool>,
    ) -> (WebCredentials, Vec<String>) {
        let mut failures = Vec::new();
        if search.disabled() {
            return (WebCredentials::default(), failures);
        }

        // Resolve only the credential the pinned provider needs; under `auto` both are
        // resolved so the seam can register whichever is usable. Otherwise a deployment
        // pinned to DeepSeek still pays an Exa `ResolveSecret` round-trip on every call.
        let pinned = search.configured_provider();
        let wants_deepseek = pinned.as_deref() != Some(EXA_PROVIDER_ID);
        let wants_exa = pinned.as_deref() != Some(DEEPSEEK_PROVIDER_ID);

        let deepseek_api_key = if wants_deepseek {
            match self
                .resolve_deepseek_search_key(session_id, turn_id, search, cancel)
                .await
            {
                Ok(key) => Some(key),
                Err(error) => {
                    failures.push(format!("DeepSeek search credential: {error}"));
                    None
                }
            }
        } else {
            None
        };

        let exa_api_key = if wants_exa {
            let secret_ref = search.exa_api_key_ref.trim();
            if secret_ref.is_empty() {
                None
            } else {
                match self
                    .resolve_secret_ref(
                        session_id,
                        turn_id,
                        secret_ref,
                        WEB_SEARCH_OWNER_ID,
                        cancel,
                    )
                    .await
                {
                    Ok(key) => Some(key),
                    Err(error) => {
                        failures.push(format!("Exa search credential: {error}"));
                        None
                    }
                }
            }
        } else {
            None
        };

        (
            WebCredentials {
                deepseek_api_key,
                exa_api_key,
            },
            failures,
        )
    }

    /// Resolve the DeepSeek search credential: an explicit reference, else the official
    /// DeepSeek provider's own credential, else `$DEEPSEEK_API_KEY`.
    ///
    /// The reuse path is what makes search work out of the box for someone who already
    /// chats with official DeepSeek — DeepSeek Harness reuses `DEEPSEEK_API_KEY` the same way.
    async fn resolve_deepseek_search_key(
        &self,
        session_id: &str,
        turn_id: &str,
        search: &WebSearchSettings,
        cancel: &Arc<AtomicBool>,
    ) -> HamburResult<String> {
        let explicit = search.api_key_ref.trim();
        if !explicit.is_empty() {
            return self
                .resolve_secret_ref(session_id, turn_id, explicit, WEB_SEARCH_OWNER_ID, cancel)
                .await;
        }

        // An official DeepSeek provider is the intended credential source once one exists, so
        // its failure is reported as such instead of falling through to a confusing
        // "DEEPSEEK_API_KEY is not set".
        if let Ok(snapshot) = self.database.settings_snapshot()
            && let Some(provider) = snapshot.providers.iter().find(|provider| {
                provider.enabled && hambur_llm::is_official_deepseek_url(&provider.base_url)
            })
            && let Ok(record) = self.database.provider_by_id(&provider.id)
            && !record.secret_ref.trim().is_empty()
        {
            return self
                .resolve_secret_ref(
                    session_id,
                    turn_id,
                    record.secret_ref.trim(),
                    &record.id,
                    cancel,
                )
                .await
                .map_err(|error| {
                    HamburError::ProviderUnavailable(format!(
                        "the official DeepSeek provider credential could not be resolved ({error}); store it again, or set an explicit key reference for web search"
                    ))
                });
        }

        self.resolve_secret_ref(
            session_id,
            turn_id,
            DEEPSEEK_SEARCH_SECRET_REF,
            WEB_SEARCH_OWNER_ID,
            cancel,
        )
        .await
    }

    /// `web_search`: run 1..=max_queries queries concurrently and merge their sources.
    async fn web_search_raw(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
        search: &WebSearchSettings,
        fetch: &WebFetchSettings,
    ) -> RawToolOutput {
        let queries = match parse_search_queries(arguments, search.max_queries) {
            Ok(queries) => queries,
            Err(error) => return web_error_raw(invocation, &error),
        };

        let cancel_flag = self.web_cancel_flag(&invocation.session_id);
        if cancel_flag.load(Ordering::SeqCst) {
            return web_error_raw(invocation, &WebError::aborted());
        }
        let cancel = CancelToken::new(cancel_flag.clone());
        let (credentials, credential_failures) = self
            .web_credentials(
                &invocation.session_id,
                &invocation.turn_id,
                search,
                &cancel_flag,
            )
            .await;
        let runtime = Arc::new(build_runtime(search, fetch, &credentials));

        if runtime.search_provider_ids().is_empty() {
            let mut error = search_setup_error(search, &credentials);
            if !credential_failures.is_empty() {
                error = WebError::new(
                    error.code(),
                    format!("{} — {}", error.message(), credential_failures.join("; ")),
                );
            }
            return web_error_raw(invocation, &error);
        }

        let mut tasks = JoinSet::new();
        for (index, query) in queries.iter().enumerate() {
            let runtime = runtime.clone();
            let cancel = cancel.clone();
            let request = WebSearchRequest {
                query: query.clone(),
                max_results: Some(search.max_results),
            };
            tasks.spawn(async move { (index, runtime.search(&request, &cancel).await) });
        }

        let mut outcomes: Vec<(usize, Result<WebSearchResult, WebError>)> = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => outcomes.push((
                    usize::MAX,
                    Err(WebError::new(
                        WEB_PROVIDER_ERROR,
                        format!("web search task failed: {error}"),
                    )),
                )),
            }
        }
        // Query order decides the merge's round-robin order, so sort the completion order back.
        outcomes.sort_by_key(|(index, _)| *index);

        let mut results = Vec::new();
        for (_, outcome) in outcomes {
            match outcome {
                Ok(result) => results.push(result),
                // One failed query fails the call, matching DSH's `firstFailure` behavior.
                Err(error) => return web_error_raw(invocation, &error),
            }
        }
        if results.is_empty() {
            return web_error_raw(
                invocation,
                &WebError::new(WEB_PROVIDER_UNAVAILABLE, "no web search results"),
            );
        }

        let merged = merge_search_results(results, search.max_results);
        let summary = search_summary(&queries, &merged);
        web_raw(
            invocation,
            false,
            format_search_output(&merged),
            summary,
            "Completed",
            "untrusted",
        )
    }

    /// `web_fetch`: retrieve one URL, or up to [`WEB_FETCH_MAX_URLS`] of them concurrently,
    /// and render each as Markdown.
    async fn web_fetch_raw(
        &self,
        invocation: &ToolInvocation,
        arguments: &Value,
        search: &WebSearchSettings,
        fetch: &WebFetchSettings,
    ) -> RawToolOutput {
        let urls = match parse_fetch_urls(arguments, WEB_FETCH_MAX_URLS) {
            Ok(urls) => urls,
            Err(error) => return web_error_raw(invocation, &error),
        };
        if !fetch.enabled {
            return web_error_raw(
                invocation,
                &WebError::new(
                    WEB_PROVIDER_UNAVAILABLE,
                    "web_fetch is turned off; enable it in Settings > Tools > Web fetch",
                ),
            );
        }

        let cancel_flag = self.web_cancel_flag(&invocation.session_id);
        if cancel_flag.load(Ordering::SeqCst) {
            return web_error_raw(invocation, &WebError::aborted());
        }
        let cancel = CancelToken::new(cancel_flag);
        // Fetching needs no credential, so the seam registers only the fetch provider.
        let runtime = Arc::new(build_runtime(search, fetch, &WebCredentials::default()));

        let mut tasks = JoinSet::new();
        for (index, url) in urls.iter().enumerate() {
            let runtime = runtime.clone();
            let cancel = cancel.clone();
            let request = WebFetchRequest { url: url.clone() };
            tasks.spawn(async move { (index, runtime.fetch(&request, &cancel).await) });
        }

        let mut outcomes = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            match joined {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => outcomes.push((
                    usize::MAX,
                    Err(WebError::new(
                        WEB_PROVIDER_ERROR,
                        format!("web fetch task failed: {error}"),
                    )),
                )),
            }
        }
        outcomes.sort_by_key(|(index, _)| *index);

        // Split the output budget across the requested pages so one long page cannot starve
        // the others.
        let per_url_chars = (fetch.max_chars / urls.len()).max(1_500);
        let mut sections = Vec::new();
        let mut failures = Vec::new();
        let mut succeeded = 0_usize;
        for (index, outcome) in &outcomes {
            let url = urls.get(*index).cloned().unwrap_or_default();
            match outcome {
                Ok(result) => {
                    succeeded += 1;
                    sections.push(format_fetch_output(result, per_url_chars));
                }
                Err(error) => {
                    // A `JoinError` (index `usize::MAX`) has no URL: label it as the task
                    // failure it is instead of rendering an empty target.
                    let target = if url.is_empty() {
                        "fetch task"
                    } else {
                        url.as_str()
                    };
                    failures.push(format!("{target}: {error}"));
                    sections.push(format!("Fetched {target} failed\n\n{error}"));
                }
            }
        }

        if succeeded == 0 {
            // Keep the first failure's stable code, but report every URL's diagnostic so a
            // multi-URL call says which pages failed and why.
            let first = outcomes
                .iter()
                .find_map(|(_, outcome)| outcome.as_ref().err())
                .cloned();
            let detail = if failures.is_empty() {
                "no URL was fetched".to_string()
            } else {
                failures.join("; ")
            };
            let error = match first {
                Some(error) => WebError::new(error.code(), detail),
                None => WebError::new(WEB_PROVIDER_UNAVAILABLE, detail),
            };
            return web_error_raw(invocation, &error);
        }

        let summary = if failures.is_empty() {
            if urls.len() == 1 {
                format!("Fetched {}", urls[0])
            } else {
                format!("Fetched {} pages", urls.len())
            }
        } else {
            format!("Fetched {succeeded}/{} pages", urls.len())
        };
        // A non-2xx response is a result, not an error: the rendered header carries the
        // status so the model can react to it.
        let status = if failures.is_empty() {
            "Completed"
        } else {
            "Partial"
        };
        web_raw(
            invocation,
            false,
            sections.join("\n\n---\n\n"),
            summary,
            status,
            "untrusted",
        )
    }
}

/// A short, UI-facing summary of a merged search.
fn search_summary(queries: &[String], result: &WebSearchResult) -> String {
    let source_word = if result.sources.len() == 1 {
        "source"
    } else {
        "sources"
    };
    let label = if queries.len() == 1 {
        format!("\"{}\"", queries[0])
    } else {
        format!("{} queries", queries.len())
    };
    if result.sources.is_empty() {
        format!("No results for {label}")
    } else {
        format!("{} {source_word} for {label}", result.sources.len())
    }
}

/// Build one web tool output. `trust_level` is `untrusted` exactly when the content came
/// from the network, so the normalizer wraps fetched pages and search results but leaves
/// this app's own error text unwrapped.
fn web_raw(
    invocation: &ToolInvocation,
    is_error: bool,
    content: String,
    summary: String,
    status: impl Into<String>,
    trust_level: &str,
) -> RawToolOutput {
    RawToolOutput {
        tool_call_id: invocation.tool_call_id.clone(),
        tool_name: invocation.name.clone(),
        is_error,
        content,
        summary,
        trust_level: trust_level.to_string(),
        command_or_url: invocation.arguments_json.clone(),
        status: status.into(),
    }
}

/// Build a failed web tool output that names the seam's error code, so the model can tell a
/// missing credential from a blocked URL or a provider outage.
fn web_error_raw(invocation: &ToolInvocation, error: &WebError) -> RawToolOutput {
    web_raw(
        invocation,
        true,
        format!("{error}"),
        format!("Web tool failed ({})", error.code()),
        error.code(),
        "trusted",
    )
}
