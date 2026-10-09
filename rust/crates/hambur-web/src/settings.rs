//! Product configuration for the web tools.
//!
//! Both sections are stored as JSON strings in the app settings table — the same shape the
//! settings UI edits — and every field falls back to the constant defaults above, so a
//! partially written or legacy value still produces a usable deployment.

use serde::{Deserialize, Serialize};

use crate::fetch_http::FetchPolicy;
use crate::search_deepseek::{
    DEEPSEEK_DEFAULT_API_VERSION, DEEPSEEK_DEFAULT_BASE_URL, DEEPSEEK_DEFAULT_MAX_TOKENS,
    DEEPSEEK_DEFAULT_MAX_USES, DEEPSEEK_DEFAULT_MODEL,
};
use crate::search_exa::{EXA_DEFAULT_BASE_URL, EXA_DEFAULT_TEXT_CHARS, EXA_MAX_RESULTS};

/// App-settings key holding [`WebSearchSettings`].
pub const WEB_SEARCH_SETTINGS_KEY: &str = "web_search_settings";
/// App-settings key holding [`WebFetchSettings`].
pub const WEB_FETCH_SETTINGS_KEY: &str = "web_fetch_settings";

/// Provider id meaning "pick the only usable one".
pub const PROVIDER_AUTO: &str = "auto";
/// Provider id meaning "register nothing": the tools report a configuration error.
pub const PROVIDER_OFF: &str = "off";

/// Settings for the `web_search` tool and its provider seam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebSearchSettings {
    /// `auto`, `off`, or a registered provider id (`deepseek-official`, `exa`).
    pub provider: String,
    /// Anthropic-compatible Messages base for the DeepSeek provider; `/messages` is appended.
    pub endpoint: String,
    /// Model used only for the auxiliary DeepSeek search turn.
    pub model: String,
    pub api_version: String,
    pub max_tokens: u32,
    /// Native `web_search` server-tool uses per DeepSeek request.
    pub max_uses: u32,
    /// Upper bound on sources returned per query.
    pub max_results: usize,
    /// Upper bound on queries accepted in one `web_search` call.
    pub max_queries: usize,
    /// Credential reference for the DeepSeek search key; empty reuses the official
    /// DeepSeek conversation provider's credential when there is one.
    pub api_key_ref: String,
    /// Exa search API base; `/search` is appended.
    pub exa_endpoint: String,
    /// Credential reference for the Exa key.
    pub exa_api_key_ref: String,
    /// Characters of page text requested per Exa hit (the snippet source).
    pub exa_snippet_chars: usize,
}

impl Default for WebSearchSettings {
    fn default() -> Self {
        Self {
            provider: PROVIDER_AUTO.to_string(),
            endpoint: DEEPSEEK_DEFAULT_BASE_URL.to_string(),
            model: DEEPSEEK_DEFAULT_MODEL.to_string(),
            api_version: DEEPSEEK_DEFAULT_API_VERSION.to_string(),
            max_tokens: DEEPSEEK_DEFAULT_MAX_TOKENS,
            max_uses: DEEPSEEK_DEFAULT_MAX_USES,
            max_results: 8,
            max_queries: 4,
            api_key_ref: String::new(),
            exa_endpoint: EXA_DEFAULT_BASE_URL.to_string(),
            exa_api_key_ref: "android-secret://web_search/exa".to_string(),
            exa_snippet_chars: EXA_DEFAULT_TEXT_CHARS,
        }
    }
}

impl WebSearchSettings {
    /// Parse a stored settings value, tolerating absent, partial, or quoted JSON.
    pub fn from_json(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::default();
        }
        let candidate =
            serde_json::from_str::<String>(trimmed).unwrap_or_else(|_| trimmed.to_string());
        serde_json::from_str::<Self>(&candidate)
            .unwrap_or_default()
            .sanitized()
    }

    /// Clamp every value into a range the providers can honor.
    pub fn sanitized(mut self) -> Self {
        let defaults = Self::default();
        self.provider = self.provider.trim().to_string();
        if self.provider.is_empty() {
            self.provider = defaults.provider;
        }
        self.endpoint = self.endpoint.trim().trim_end_matches('/').to_string();
        if self.endpoint.is_empty() {
            self.endpoint = defaults.endpoint;
        }
        self.model = self.model.trim().to_string();
        if self.model.is_empty() {
            self.model = defaults.model;
        }
        self.api_version = self.api_version.trim().to_string();
        if self.api_version.is_empty() {
            self.api_version = defaults.api_version;
        }
        self.max_tokens = self.max_tokens.clamp(256, 32_000);
        self.max_uses = self.max_uses.clamp(1, 20);
        self.max_results = self.max_results.clamp(1, EXA_MAX_RESULTS);
        self.max_queries = self.max_queries.clamp(1, 8);
        self.exa_endpoint = self.exa_endpoint.trim().trim_end_matches('/').to_string();
        if self.exa_endpoint.is_empty() {
            self.exa_endpoint = defaults.exa_endpoint;
        }
        self.exa_snippet_chars = self.exa_snippet_chars.clamp(120, 8_000);
        self
    }

    /// The provider id to pin, or `None` for auto-selection.
    pub fn configured_provider(&self) -> Option<String> {
        let provider = self.provider.trim();
        if provider.is_empty() || provider == PROVIDER_AUTO || provider == PROVIDER_OFF {
            return None;
        }
        Some(provider.to_string())
    }

    pub fn disabled(&self) -> bool {
        self.provider.trim() == PROVIDER_OFF
    }
}

/// Settings for the `web_fetch` tool and its local HTTP provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WebFetchSettings {
    /// Whether `web_fetch` is exposed at all.
    pub enabled: bool,
    /// Upper bound on decoded body bytes kept from one response.
    pub max_bytes: usize,
    /// Per-request budget in milliseconds.
    pub timeout_ms: u64,
    pub max_redirects: usize,
    /// Whether loopback and private-range hosts may be reached.
    pub allow_private_hosts: bool,
    /// Cap on the markdown handed to the model for one fetch.
    pub max_chars: usize,
}

impl Default for WebFetchSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_bytes: 2 * 1024 * 1024,
            timeout_ms: 20_000,
            max_redirects: 4,
            allow_private_hosts: false,
            max_chars: 48_000,
        }
    }
}

impl WebFetchSettings {
    /// Parse a stored settings value, tolerating absent, partial, or quoted JSON.
    pub fn from_json(raw: &str) -> Self {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Self::default();
        }
        let candidate =
            serde_json::from_str::<String>(trimmed).unwrap_or_else(|_| trimmed.to_string());
        serde_json::from_str::<Self>(&candidate)
            .unwrap_or_default()
            .sanitized()
    }

    pub fn sanitized(mut self) -> Self {
        self.max_bytes = self.max_bytes.clamp(8 * 1024, 16 * 1024 * 1024);
        self.timeout_ms = self.timeout_ms.clamp(1_000, 120_000);
        self.max_redirects = self.max_redirects.min(10);
        self.max_chars = self.max_chars.clamp(1_000, 400_000);
        self
    }

    /// Project these settings into the fetch provider's local policy.
    pub fn policy(&self) -> FetchPolicy {
        FetchPolicy {
            max_bytes: self.max_bytes,
            timeout: std::time::Duration::from_millis(self.timeout_ms),
            max_redirects: self.max_redirects,
            allow_private_hosts: self.allow_private_hosts,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_broken_values_fall_back_to_defaults() {
        assert_eq!(
            WebSearchSettings::from_json(""),
            WebSearchSettings::default()
        );
        assert_eq!(
            WebSearchSettings::from_json("not json"),
            WebSearchSettings::default()
        );
        // Values written through the settings UI are JSON strings.
        let quoted = serde_json::to_string(&serde_json::json!({"maxResults": 3}).to_string())
            .expect("quoted");
        let parsed = WebSearchSettings::from_json(&quoted);
        assert_eq!(parsed.max_results, 3);
        assert_eq!(parsed.provider, PROVIDER_AUTO);
    }

    #[test]
    fn clamps_hostile_values() {
        let parsed = WebSearchSettings::from_json(
            r#"{"maxResults":9000,"maxQueries":0,"maxTokens":1,"endpoint":"https://x.example/v1/"}"#,
        );
        assert_eq!(parsed.max_results, EXA_MAX_RESULTS);
        assert_eq!(parsed.max_queries, 1);
        assert_eq!(parsed.max_tokens, 256);
        assert_eq!(parsed.endpoint, "https://x.example/v1");
    }

    #[test]
    fn provider_selection_reads_the_sentinel_values() {
        assert_eq!(WebSearchSettings::default().configured_provider(), None);
        assert!(WebSearchSettings::default().disabled() == false);
        let off = WebSearchSettings::from_json(r#"{"provider":"off"}"#);
        assert!(off.disabled());
        assert_eq!(off.configured_provider(), None);
        let pinned = WebSearchSettings::from_json(r#"{"provider":"exa"}"#);
        assert_eq!(pinned.configured_provider().as_deref(), Some("exa"));
    }

    #[test]
    fn fetch_policy_follows_settings() {
        let policy = WebFetchSettings::from_json(r#"{"timeoutMs":5000,"maxRedirects":1}"#).policy();
        assert_eq!(policy.timeout, std::time::Duration::from_millis(5_000));
        assert_eq!(policy.max_redirects, 1);
        assert!(!policy.allow_private_hosts);
    }
}
