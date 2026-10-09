//! Vocabulary for the web capability seam.
//!
//! Search and fetch deliberately share one seam so provider selection, cancellation,
//! errors, and product configuration have one owner, while keeping separate request and
//! result types. The shapes mirror DeepSeek Harness' `ctx.web` seam so a provider ported
//! from DSH needs only the transport swapped.

use serde::{Deserialize, Serialize};
use std::fmt;

/// No usable provider is registered (and none was configured).
pub const WEB_PROVIDER_UNAVAILABLE: &str = "WEB_PROVIDER_UNAVAILABLE";
/// A provider id was configured but nothing registered under it.
pub const WEB_PROVIDER_CONFIGURED_MISSING: &str = "WEB_PROVIDER_CONFIGURED_MISSING";
/// A provider id was configured, is registered, but reports itself unusable.
pub const WEB_PROVIDER_CONFIGURED_UNAVAILABLE: &str = "WEB_PROVIDER_CONFIGURED_UNAVAILABLE";
/// Several usable providers are registered and none was configured.
pub const WEB_PROVIDER_AMBIGUOUS: &str = "WEB_PROVIDER_AMBIGUOUS";
/// The provider failed after dispatch began.
pub const WEB_PROVIDER_ERROR: &str = "WEB_PROVIDER_ERROR";
/// The provider has no credential for the operation.
pub const WEB_PROVIDER_CREDENTIAL_MISSING: &str = "WEB_PROVIDER_CREDENTIAL_MISSING";
/// The caller cancelled the operation.
pub const WEB_ABORTED: &str = "WEB_ABORTED";
/// The tool arguments do not satisfy the model-facing schema.
pub const WEB_INVALID_ARGUMENTS: &str = "WEB_INVALID_ARGUMENTS";
/// The URL is not a retrievable http(s) URL.
pub const WEB_INVALID_URL: &str = "WEB_INVALID_URL";
/// The URL points at a host the local fetch policy refuses to reach.
pub const WEB_BLOCKED_URL: &str = "WEB_BLOCKED_URL";
/// The redirect chain exceeded its bound.
pub const WEB_TOO_MANY_REDIRECTS: &str = "WEB_TOO_MANY_REDIRECTS";
/// The resource is larger than the configured body cap.
pub const WEB_TOO_LARGE: &str = "WEB_TOO_LARGE";
/// The retrieval exceeded the configured budget.
pub const WEB_TIMEOUT: &str = "WEB_TIMEOUT";
/// The response content type is not one this build can represent.
pub const WEB_UNSUPPORTED_CONTENT_TYPE: &str = "WEB_UNSUPPORTED_CONTENT_TYPE";

/// Typed web error with a machine-routable, open-string `code`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebError {
    code: &'static str,
    message: String,
}

impl WebError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// The stable cancellation error for an aborted operation.
    pub fn aborted() -> Self {
        Self::new(WEB_ABORTED, "web operation aborted")
    }
}

impl fmt::Display for WebError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} [{}]", self.message, self.code)
    }
}

impl std::error::Error for WebError {}

/// What one search-capable backend is asked to search. Each request carries one query.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchRequest {
    pub query: String,
    /// Upper bound on returned sources; the seam truncates to it.
    pub max_results: Option<usize>,
}

/// One citable source. A source always has a URL; the other fields are optional because
/// not every provider returns them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchSource {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<String>,
}

/// Normalized search outcome.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// Optional provider-generated answer text, search context, or summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Citable sources, already truncated to the request's `max_results`.
    pub sources: Vec<WebSearchSource>,
    /// True when the seam dropped sources to honor `max_results`.
    pub truncated: bool,
}

/// What one fetch-capable backend is asked to retrieve.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebFetchRequest {
    pub url: String,
}

/// The decoded body of a fetched resource. A closed union: the provider decodes the kind
/// and the tool layer renders it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebFetchBody {
    Html(String),
    Text(String),
}

impl WebFetchBody {
    pub fn content(&self) -> &str {
        match self {
            Self::Html(content) | Self::Text(content) => content,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Html(_) => "html",
            Self::Text(_) => "text",
        }
    }
}

/// Normalized fetch outcome. A successful retrieval of a non-2xx response is a result,
/// not an error: the status code is part of the fetched resource state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebFetchResult {
    /// The final URL after allowed redirects.
    pub url: String,
    pub status_code: u16,
    pub body: WebFetchBody,
    /// The response's content type header, when it carried one.
    pub content_type: String,
    /// True when the provider capped the decoded body.
    pub truncated: bool,
}
