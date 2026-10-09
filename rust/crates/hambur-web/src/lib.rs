//! Web access for Hambur: a provider seam for search and fetch, the backends this build
//! ships, and the model-facing rendering the runtime tools use.
//!
//! The shape mirrors DeepSeek Harness' `ctx.web` capability seam (`@deepseek-ai/dsh-web`)
//! together with its DeepSeek/HTTP providers and `dsh-tool-web` renderers, so behavior and
//! prompts stay close to DSH while the transport is plain Rust `reqwest`.
//!
//! Layering:
//! - [`seam`] owns provider registration, selection rules, and the `max_results` bound;
//! - [`search_deepseek`] and [`search_exa`] are search backends;
//! - [`fetch_http`] is the anonymous public HTTP(S) fetch backend, with SSRF screening;
//! - [`render`] turns seam results into the text a model reads;
//! - [`settings`] maps the app's stored JSON settings onto all of the above.

pub mod cancel;
pub mod fetch_http;
pub mod html;
pub mod registry;
pub mod render;
pub mod seam;
pub mod search_deepseek;
pub mod search_exa;
pub mod settings;
pub mod types;

pub use cancel::{CancelToken, race_cancel};
pub use fetch_http::{FetchPolicy, HttpFetchProvider};
pub use registry::{SearchBackend, WebCredentials, build_runtime, search_setup_error};
pub use render::{
    TRUNCATION_FOOTER, WEB_FETCH_MAX_URLS, format_fetch_output, format_search_output,
    merge_search_results, parse_fetch_urls, parse_search_queries, source_label,
};
pub use seam::{FetchProviderSpec, ProviderId, SearchProviderSpec, WebRuntime, select_provider};
pub use search_deepseek::{
    DEEPSEEK_DEFAULT_API_VERSION, DEEPSEEK_DEFAULT_BASE_URL, DEEPSEEK_DEFAULT_MAX_TOKENS,
    DEEPSEEK_DEFAULT_MAX_USES, DEEPSEEK_DEFAULT_MODEL, DEEPSEEK_PROVIDER_ID,
    DeepSeekSearchProvider,
};
pub use search_exa::{EXA_DEFAULT_BASE_URL, EXA_PROVIDER_ID, ExaSearchProvider};
pub use settings::{
    PROVIDER_AUTO, PROVIDER_OFF, WEB_FETCH_SETTINGS_KEY, WEB_SEARCH_SETTINGS_KEY, WebFetchSettings,
    WebSearchSettings,
};
pub use types::*;

/// Test-only helpers. The crate deliberately avoids tokio's `macros` feature, so async
/// tests drive their own runtime instead of using `#[tokio::test]`.
#[cfg(test)]
pub(crate) mod test_support {
    use std::future::Future;

    /// Run one future to completion on a private single-threaded runtime.
    pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime")
            .block_on(future)
    }
}
