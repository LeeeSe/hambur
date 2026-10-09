//! The web access capability seam: provider registries and provider-selecting execution
//! for search and fetch.
//!
//! Selection happens at execution time and never depends on registration order:
//! - a configured id that is registered and usable → that provider;
//! - a configured id that is not registered → `WEB_PROVIDER_CONFIGURED_MISSING`;
//! - a configured id registered but unusable → `WEB_PROVIDER_CONFIGURED_UNAVAILABLE`;
//! - no id configured, exactly one usable provider → that provider;
//! - no id configured, several usable providers → `WEB_PROVIDER_AMBIGUOUS`;
//! - no id configured, none usable → `WEB_PROVIDER_UNAVAILABLE`.
//!
//! The seam also owns `max_results`: a provider that over-returns is truncated here so the
//! bound holds no matter which backend served the search.

use std::future::Future;

use crate::cancel::CancelToken;
use crate::types::{
    WEB_PROVIDER_AMBIGUOUS, WEB_PROVIDER_CONFIGURED_MISSING, WEB_PROVIDER_CONFIGURED_UNAVAILABLE,
    WEB_PROVIDER_UNAVAILABLE, WebError, WebFetchRequest, WebFetchResult, WebSearchRequest,
    WebSearchResult,
};

/// Identity plus cheap local usability for one registered provider.
pub trait ProviderId {
    fn id(&self) -> &'static str;
    /// Cheap local usability check; must not make network calls.
    fn available(&self) -> bool;
}

/// A search-capable backend.
pub trait SearchProviderSpec: ProviderId + Send + Sync {
    fn search(
        &self,
        request: &WebSearchRequest,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<WebSearchResult, WebError>> + Send;
}

/// A fetch-capable backend.
pub trait FetchProviderSpec: ProviderId + Send + Sync {
    fn fetch(
        &self,
        request: &WebFetchRequest,
        cancel: &CancelToken,
    ) -> impl Future<Output = Result<WebFetchResult, WebError>> + Send;
}

/// Resolve one provider from the registry using the seam's documented rules.
pub fn select_provider<'a, P: ProviderId>(
    providers: &'a [P],
    configured: Option<&str>,
    capability: &str,
) -> Result<&'a P, WebError> {
    if let Some(configured) = configured.map(str::trim).filter(|id| !id.is_empty()) {
        let provider = providers
            .iter()
            .find(|provider| provider.id() == configured)
            .ok_or_else(|| {
                WebError::new(
                    WEB_PROVIDER_CONFIGURED_MISSING,
                    format!(
                        "configured web {capability} provider \"{configured}\" is not registered"
                    ),
                )
            })?;
        if !provider.available() {
            return Err(WebError::new(
                WEB_PROVIDER_CONFIGURED_UNAVAILABLE,
                format!(
                    "configured web {capability} provider \"{configured}\" is registered but unusable (missing endpoint or credential)"
                ),
            ));
        }
        return Ok(provider);
    }

    let mut usable = providers.iter().filter(|provider| provider.available());
    let first = usable.next().ok_or_else(|| {
        WebError::new(
            WEB_PROVIDER_UNAVAILABLE,
            format!("no usable web {capability} provider is configured or reachable"),
        )
    })?;
    if let Some(second) = usable.next() {
        return Err(WebError::new(
            WEB_PROVIDER_AMBIGUOUS,
            format!(
                "several web {capability} providers are usable (\"{}\", \"{}\"); configure one explicitly",
                first.id(),
                second.id()
            ),
        ));
    }
    Ok(first)
}

/// The web access runtime: one registry per capability plus the configured selection.
pub struct WebRuntime<S, F> {
    search_providers: Vec<S>,
    fetch_providers: Vec<F>,
    search_provider_id: Option<String>,
    fetch_provider_id: Option<String>,
}

impl<S, F> WebRuntime<S, F>
where
    S: SearchProviderSpec,
    F: FetchProviderSpec,
{
    pub fn new(search_providers: Vec<S>, fetch_providers: Vec<F>) -> Self {
        Self {
            search_providers,
            fetch_providers,
            search_provider_id: None,
            fetch_provider_id: None,
        }
    }

    /// Pin which search provider wins; `None` or an empty value auto-selects.
    pub fn with_search_provider(mut self, provider_id: Option<String>) -> Self {
        self.search_provider_id = provider_id.filter(|id| !id.trim().is_empty());
        self
    }

    /// Pin which fetch provider wins; `None` or an empty value auto-selects.
    pub fn with_fetch_provider(mut self, provider_id: Option<String>) -> Self {
        self.fetch_provider_id = provider_id.filter(|id| !id.trim().is_empty());
        self
    }

    pub fn search_provider_id(&self) -> Option<&str> {
        self.search_provider_id.as_deref()
    }

    pub fn fetch_provider_id(&self) -> Option<&str> {
        self.fetch_provider_id.as_deref()
    }

    /// Ids of the registered search providers, in registration order.
    pub fn search_provider_ids(&self) -> Vec<&'static str> {
        self.search_providers.iter().map(ProviderId::id).collect()
    }

    /// Run one search through the selected provider, enforcing `request.max_results`.
    pub async fn search(
        &self,
        request: &WebSearchRequest,
        cancel: &CancelToken,
    ) -> Result<WebSearchResult, WebError> {
        let provider = select_provider(
            &self.search_providers,
            self.search_provider_id.as_deref(),
            "search",
        )?;
        let mut result = provider.search(request, cancel).await?;
        if let Some(max_results) = request.max_results
            && result.sources.len() > max_results
        {
            result.sources.truncate(max_results);
            result.truncated = true;
        }
        Ok(result)
    }

    /// Retrieve one URL through the selected provider. A non-2xx response is a result.
    pub async fn fetch(
        &self,
        request: &WebFetchRequest,
        cancel: &CancelToken,
    ) -> Result<WebFetchResult, WebError> {
        let provider = select_provider(
            &self.fetch_providers,
            self.fetch_provider_id.as_deref(),
            "fetch",
        )?;
        provider.fetch(request, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WebFetchBody, WebSearchSource};

    struct FakeSearch {
        id: &'static str,
        usable: bool,
    }

    impl ProviderId for FakeSearch {
        fn id(&self) -> &'static str {
            self.id
        }
        fn available(&self) -> bool {
            self.usable
        }
    }

    impl SearchProviderSpec for FakeSearch {
        async fn search(
            &self,
            _request: &WebSearchRequest,
            _cancel: &CancelToken,
        ) -> Result<WebSearchResult, WebError> {
            Ok(WebSearchResult {
                content: None,
                sources: (0..3)
                    .map(|index| WebSearchSource {
                        url: format!("https://example.com/{index}"),
                        ..Default::default()
                    })
                    .collect(),
                truncated: false,
            })
        }
    }

    struct FakeFetch;

    impl ProviderId for FakeFetch {
        fn id(&self) -> &'static str {
            "fake-fetch"
        }
        fn available(&self) -> bool {
            true
        }
    }

    impl FetchProviderSpec for FakeFetch {
        async fn fetch(
            &self,
            request: &WebFetchRequest,
            _cancel: &CancelToken,
        ) -> Result<WebFetchResult, WebError> {
            Ok(WebFetchResult {
                url: request.url.clone(),
                status_code: 200,
                body: WebFetchBody::Text("ok".to_string()),
                content_type: "text/plain".to_string(),
                truncated: false,
            })
        }
    }

    fn runtime(
        providers: Vec<FakeSearch>,
        configured: Option<&str>,
    ) -> WebRuntime<FakeSearch, FakeFetch> {
        WebRuntime::new(providers, vec![FakeFetch])
            .with_search_provider(configured.map(str::to_string))
    }

    #[test]
    fn auto_selects_the_only_usable_provider() {
        crate::test_support::block_on(async {
            let runtime = runtime(
                vec![FakeSearch {
                    id: "one",
                    usable: true,
                }],
                None,
            );
            let result = runtime
                .search(&WebSearchRequest::default(), &CancelToken::never())
                .await
                .expect("search");
            assert_eq!(result.sources.len(), 3);
        });
    }

    #[test]
    fn refuses_to_guess_between_two_usable_providers() {
        crate::test_support::block_on(async {
            let runtime = runtime(
                vec![
                    FakeSearch {
                        id: "one",
                        usable: true,
                    },
                    FakeSearch {
                        id: "two",
                        usable: true,
                    },
                ],
                None,
            );
            let error = runtime
                .search(&WebSearchRequest::default(), &CancelToken::never())
                .await
                .expect_err("ambiguous");
            assert_eq!(error.code(), WEB_PROVIDER_AMBIGUOUS);
        });
    }

    #[test]
    fn configured_provider_must_exist_and_be_usable() {
        crate::test_support::block_on(async {
            let missing = runtime(vec![], Some("nope"));
            let error = missing
                .search(&WebSearchRequest::default(), &CancelToken::never())
                .await
                .expect_err("missing");
            assert_eq!(error.code(), WEB_PROVIDER_CONFIGURED_MISSING);

            let unusable = runtime(
                vec![FakeSearch {
                    id: "one",
                    usable: false,
                }],
                Some("one"),
            );
            let error = unusable
                .search(&WebSearchRequest::default(), &CancelToken::never())
                .await
                .expect_err("unusable");
            assert_eq!(error.code(), WEB_PROVIDER_CONFIGURED_UNAVAILABLE);
        });
    }

    #[test]
    fn the_seam_truncates_over_returning_providers() {
        crate::test_support::block_on(async {
            let runtime = runtime(
                vec![FakeSearch {
                    id: "one",
                    usable: true,
                }],
                None,
            );
            let result = runtime
                .search(
                    &WebSearchRequest {
                        query: "q".to_string(),
                        max_results: Some(2),
                    },
                    &CancelToken::never(),
                )
                .await
                .expect("search");
            assert_eq!(result.sources.len(), 2);
            assert!(result.truncated);
        });
    }
}
