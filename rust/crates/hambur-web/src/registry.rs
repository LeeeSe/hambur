//! Provider registration: turns parsed settings plus resolved credentials into a
//! [`WebRuntime`], exactly like a DSH deployment registers `ctx.web` providers.
//!
//! A provider whose credential did not resolve is simply not registered, so the seam's
//! selection rules stay honest: "one usable provider" never counts a keyless backend.

use crate::cancel::CancelToken;
use crate::fetch_http::HttpFetchProvider;
use crate::seam::{ProviderId, SearchProviderSpec, WebRuntime};
use crate::search_deepseek::{DEEPSEEK_PROVIDER_ID, DeepSeekSearchProvider};
use crate::search_exa::{EXA_PROVIDER_ID, ExaSearchProvider};
use crate::settings::{WebFetchSettings, WebSearchSettings};
use crate::types::{
    WEB_PROVIDER_CREDENTIAL_MISSING, WEB_PROVIDER_UNAVAILABLE, WebError, WebSearchRequest,
    WebSearchResult,
};

/// Credentials the host resolved for one web operation.
///
/// Deliberately not `Debug`: every field is a raw API key, so an accidental `{:?}` in a
/// log line or error message would print the secrets verbatim.
#[derive(Clone, Default)]
pub struct WebCredentials {
    /// DeepSeek API key used only for the auxiliary search call.
    pub deepseek_api_key: Option<String>,
    /// Exa API key.
    pub exa_api_key: Option<String>,
}

impl WebCredentials {
    fn deepseek(&self) -> Option<&str> {
        self.deepseek_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }

    fn exa(&self) -> Option<&str> {
        self.exa_api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
    }
}

/// Every search backend this build can serve, in registration order.
#[derive(Clone)]
pub enum SearchBackend {
    DeepSeek(DeepSeekSearchProvider),
    Exa(ExaSearchProvider),
}

impl ProviderId for SearchBackend {
    fn id(&self) -> &'static str {
        match self {
            Self::DeepSeek(_) => DEEPSEEK_PROVIDER_ID,
            Self::Exa(_) => EXA_PROVIDER_ID,
        }
    }

    fn available(&self) -> bool {
        match self {
            Self::DeepSeek(provider) => provider.available(),
            Self::Exa(provider) => provider.available(),
        }
    }
}

impl SearchProviderSpec for SearchBackend {
    async fn search(
        &self,
        request: &WebSearchRequest,
        cancel: &CancelToken,
    ) -> Result<WebSearchResult, WebError> {
        match self {
            Self::DeepSeek(provider) => provider.search(request, cancel).await,
            Self::Exa(provider) => provider.search(request, cancel).await,
        }
    }
}

/// Build the runtime for one operation from settings and resolved credentials.
pub fn build_runtime(
    search: &WebSearchSettings,
    fetch: &WebFetchSettings,
    credentials: &WebCredentials,
) -> WebRuntime<SearchBackend, HttpFetchProvider> {
    let mut search_providers = Vec::new();
    if !search.disabled() {
        if let Some(api_key) = credentials.deepseek() {
            search_providers.push(SearchBackend::DeepSeek(DeepSeekSearchProvider::new(
                &search.endpoint,
                api_key,
                &search.model,
                &search.api_version,
                search.max_tokens,
                search.max_uses,
            )));
        }
        if let Some(api_key) = credentials.exa() {
            search_providers.push(SearchBackend::Exa(ExaSearchProvider::new(
                &search.exa_endpoint,
                api_key,
                search.exa_snippet_chars,
                search.max_results,
            )));
        }
    }

    let fetch_providers = if fetch.enabled {
        vec![HttpFetchProvider::new(fetch.policy())]
    } else {
        Vec::new()
    };

    WebRuntime::new(search_providers, fetch_providers)
        .with_search_provider(search.configured_provider())
        .with_fetch_provider(None)
}

/// Explain why no search provider could be built, with the exact fix for the user.
pub fn search_setup_error(search: &WebSearchSettings, credentials: &WebCredentials) -> WebError {
    if search.disabled() {
        return WebError::new(
            WEB_PROVIDER_UNAVAILABLE,
            "web search is turned off; set Provider to \"auto\" or a backend in Settings > Tools > Web search",
        );
    }
    if credentials.deepseek().is_none() && credentials.exa().is_none() {
        return WebError::new(
            WEB_PROVIDER_CREDENTIAL_MISSING,
            "web search needs a credential: add an Exa API key in Settings > Tools > Web search, or configure a DeepSeek provider (the search call reuses its key, or set an explicit key reference)",
        );
    }
    WebError::new(
        WEB_PROVIDER_UNAVAILABLE,
        "no usable web search provider is registered; check Settings > Tools > Web search",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::WebFetchRequest;

    #[test]
    fn only_keyed_providers_register() {
        let search = WebSearchSettings::default();
        let fetch = WebFetchSettings::default();

        let empty = build_runtime(&search, &fetch, &WebCredentials::default());
        assert!(empty.search_provider_ids().is_empty());

        let deepseek_only = build_runtime(
            &search,
            &fetch,
            &WebCredentials {
                deepseek_api_key: Some("sk-test".to_string()),
                exa_api_key: None,
            },
        );
        assert_eq!(
            deepseek_only.search_provider_ids(),
            vec![DEEPSEEK_PROVIDER_ID]
        );

        let both = build_runtime(
            &search,
            &fetch,
            &WebCredentials {
                deepseek_api_key: Some("sk-test".to_string()),
                exa_api_key: Some("exa-test".to_string()),
            },
        );
        assert_eq!(
            both.search_provider_ids(),
            vec![DEEPSEEK_PROVIDER_ID, EXA_PROVIDER_ID]
        );
    }

    #[test]
    fn turning_search_off_registers_nothing() {
        let search = WebSearchSettings::from_json(r#"{"provider":"off"}"#);
        let runtime = build_runtime(
            &search,
            &WebFetchSettings::default(),
            &WebCredentials {
                deepseek_api_key: Some("sk-test".to_string()),
                exa_api_key: None,
            },
        );
        assert!(runtime.search_provider_ids().is_empty());
        assert!(
            search_setup_error(&search, &WebCredentials::default())
                .message()
                .contains("turned off")
        );
    }

    #[test]
    fn a_disabled_fetch_tool_registers_no_fetch_provider() {
        let fetch = WebFetchSettings::from_json(r#"{"enabled":false}"#);
        let runtime = build_runtime(
            &WebSearchSettings::default(),
            &fetch,
            &WebCredentials {
                deepseek_api_key: Some("sk-test".to_string()),
                exa_api_key: None,
            },
        );
        let error = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(runtime.fetch(
                &WebFetchRequest {
                    url: "https://example.com".to_string(),
                },
                &CancelToken::never(),
            ))
            .expect_err("no provider");
        assert_eq!(error.code(), WEB_PROVIDER_UNAVAILABLE);
    }
}
