//! Anonymous public HTTP(S) fetch provider for the web seam.
//!
//! Safety properties, in the order they are enforced:
//! 1. only `http`/`https`, no embedded credentials;
//! 2. the host is resolved once, every resolved address must be public unless the local
//!    policy opts into private hosts, and the connection is then pinned to those
//!    addresses so a rebinding DNS answer cannot swap in an internal target;
//! 3. redirects are followed manually, each hop re-validated like a fresh request;
//! 4. the body is streamed and capped; a declared content type must be representable,
//!    while a missing one is treated as text (many pages omit the header).
//!
//! Every network step is bounded, and none of them can outlive the caller's budget. The
//! screening lookup needs its own deadline because `reqwest`'s request timeout cannot see a
//! resolver that has stopped answering; the handshake is additionally capped at
//! [`CONNECT_TIMEOUT_CEILING`] so a slow target cannot hold a slot for the whole budget.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use crate::cancel::{CancelToken, race_cancel};
use crate::seam::{FetchProviderSpec, ProviderId};
use crate::types::{
    WEB_ABORTED, WEB_BLOCKED_URL, WEB_INVALID_URL, WEB_PROVIDER_ERROR, WEB_TIMEOUT,
    WEB_TOO_MANY_REDIRECTS, WEB_UNSUPPORTED_CONTENT_TYPE, WebError, WebFetchBody, WebFetchRequest,
    WebFetchResult,
};

/// Stable id this provider registers under.
pub const HTTP_PROVIDER_ID: &str = "http";

/// Upper bound on the screening DNS lookup, so a resolver that never answers cannot hang a
/// fetch past its own budget. Connect and body phases are bounded by `policy.timeout`.
pub const DNS_TIMEOUT_CEILING: Duration = Duration::from_secs(5);

/// Upper bound on the TCP/TLS handshake, applied on top of the per-request budget. The
/// effective deadline is `min(policy.timeout, this)`.
const CONNECT_TIMEOUT_CEILING: Duration = Duration::from_secs(10);

/// Upper bound on cached pinned clients. A fetch operation touches at most a handful of
/// hosts; beyond this the cache is reset rather than grown without bound.
const CLIENT_CACHE_LIMIT: usize = 16;

const USER_AGENT: &str = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Hambur/0.1 Safari/537.36";

/// Local retrieval policy for the anonymous fetch provider.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    /// Upper bound on decoded body bytes kept from one response.
    pub max_bytes: usize,
    /// Per-request budget covering connect, headers, and body streaming.
    pub timeout: Duration,
    /// Maximum redirect hops followed before failing.
    pub max_redirects: usize,
    /// Whether hosts inside private, loopback, or link-local ranges may be reached.
    pub allow_private_hosts: bool,
}

impl Default for FetchPolicy {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024,
            timeout: Duration::from_secs(20),
            max_redirects: 4,
            allow_private_hosts: false,
        }
    }
}

impl FetchPolicy {
    /// Deadline for the screening DNS lookup: bounded on its own, and never allowed to
    /// outlive the request budget the caller configured.
    pub fn dns_timeout(&self) -> Duration {
        self.timeout.min(DNS_TIMEOUT_CEILING)
    }

    /// Deadline for the TCP/TLS handshake, inside the request budget.
    pub fn connect_timeout(&self) -> Duration {
        self.timeout.min(CONNECT_TIMEOUT_CEILING)
    }
}

/// True when an address is routable on the public internet.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_public_ipv4(ipv4),
        IpAddr::V6(ipv6) => {
            if let Some(mapped) = ipv6.to_ipv4_mapped() {
                return is_public_ipv4(mapped);
            }
            let segments = ipv6.segments();
            let documentation = segments[0] == 0x2001 && segments[1] == 0x0db8; // 2001:db8::/32
            let teredo = segments[0] == 0x2001 && segments[1] == 0x0000; // 2001::/32 embeds IPv4
            let six_to_four = segments[0] == 0x2002; // 2002::/16 embeds IPv4
            let nat64 = segments[0] == 0x0064 && segments[1] == 0xff9b; // 64:ff9b::/96 and ::/48
            !(ipv6.is_unspecified()
                || ipv6.is_loopback()
                || ipv6.is_multicast()
                || segments[0] & 0xfe00 == 0xfc00 // fc00::/7 unique local
                || segments[0] & 0xffc0 == 0xfe80 // fe80::/10 link local
                || documentation
                || teredo
                || six_to_four
                || nat64)
        }
    }
}

fn is_public_ipv4(ipv4: Ipv4Addr) -> bool {
    let [first, second, _, _] = ipv4.octets();
    !(ipv4.is_unspecified()
        || ipv4.is_loopback()
        || ipv4.is_private()
        || ipv4.is_link_local()
        || ipv4.is_broadcast()
        || ipv4.is_documentation()
        || ipv4.is_multicast()
        || first == 0
        || (first == 100 && (64..128).contains(&second)) // 100.64.0.0/10 carrier NAT
        || (first == 192 && second == 0) // 192.0.0.0/24 protocol assignments
        || (first == 198 && (second == 18 || second == 19)) // 198.18.0.0/15 benchmarking
        || first >= 240) // 240.0.0.0/4 reserved, includes the broadcast address
}

/// The anonymous public HTTP(S) fetch provider.
#[derive(Clone)]
pub struct HttpFetchProvider {
    policy: FetchPolicy,
    /// Pinned clients keyed by host, port, and screened addresses, so redirect hops and
    /// repeated URLs reuse one connection pool. Shared across clones (the runtime clones
    /// the provider per URL) without outliving the operation that built it.
    clients: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, reqwest::Client>>>,
}

impl Default for HttpFetchProvider {
    fn default() -> Self {
        Self::new(FetchPolicy::default())
    }
}

impl HttpFetchProvider {
    pub fn new(policy: FetchPolicy) -> Self {
        Self {
            policy,
            clients: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn policy(&self) -> &FetchPolicy {
        &self.policy
    }

    /// Screen one URL and return the socket addresses the connection may use.
    async fn screen(
        &self,
        url: &reqwest::Url,
        cancel: &CancelToken,
    ) -> Result<Vec<SocketAddr>, WebError> {
        let host = url
            .host_str()
            .ok_or_else(|| WebError::new(WEB_INVALID_URL, "URL has no host"))?;
        let port = url.port_or_known_default().ok_or_else(|| {
            WebError::new(
                WEB_INVALID_URL,
                format!("URL scheme \"{}\" has no default port", url.scheme()),
            )
        })?;

        let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
            vec![SocketAddr::new(ip, port)]
        } else {
            self.resolve(host, port, cancel).await?
        };

        if addresses.is_empty() {
            return Err(WebError::new(
                WEB_PROVIDER_ERROR,
                format!("host \"{host}\" resolved to no addresses"),
            ));
        }
        if !self.policy.allow_private_hosts
            && let Some(blocked) = addresses.iter().find(|address| !is_public_ip(address.ip()))
        {
            return Err(WebError::new(
                WEB_BLOCKED_URL,
                format!(
                    "refusing to fetch {url}: host \"{host}\" resolves to the non-public address {} (enable \"Allow private hosts\" in Settings > Tools > Web fetch to override)",
                    blocked.ip()
                ),
            ));
        }
        Ok(addresses)
    }

    /// Resolve one host under the DNS deadline and the turn's cancel flag.
    ///
    /// `resolve_to_addrs` pins the answer, but the lookup itself happens here, so this is the
    /// step that has to be bounded: without a deadline a resolver that stops answering hangs
    /// the whole tool call, and without the cancel token an aborted turn keeps waiting on it.
    async fn resolve(
        &self,
        host: &str,
        port: u16,
        cancel: &CancelToken,
    ) -> Result<Vec<SocketAddr>, WebError> {
        let budget = self.policy.dns_timeout();
        let failed = |error: WebError| {
            if error.code() == WEB_ABORTED {
                // An aborted turn keeps its own error rather than being reported as a lookup
                // failure.
                error
            } else {
                WebError::new(
                    WEB_PROVIDER_ERROR,
                    format!("could not resolve host \"{host}\": {error}"),
                )
            }
        };
        let deadline_elapsed = || {
            WebError::new(
                WEB_TIMEOUT,
                format!(
                    "DNS lookup for \"{host}\" exceeded {} ms",
                    budget.as_millis()
                ),
            )
        };

        let lookup = race_cancel(tokio::net::lookup_host((host, port)), cancel);
        let addresses = tokio::time::timeout(budget, lookup)
            .await
            .map_err(|_| deadline_elapsed())?
            .map_err(failed)?
            .map_err(|error| {
                WebError::new(
                    WEB_PROVIDER_ERROR,
                    format!("could not resolve host \"{host}\": {error}"),
                )
            })?;
        Ok(addresses.collect())
    }

    /// Build (or reuse) a client whose DNS answer for this host is pinned to the screened
    /// addresses. Reuse keeps the connection pool across redirect hops and across the URLs
    /// of one call instead of re-handshaking on every request.
    fn client_for(
        &self,
        url: &reqwest::Url,
        addresses: &[SocketAddr],
    ) -> Result<reqwest::Client, WebError> {
        let host = url
            .host_str()
            .ok_or_else(|| WebError::new(WEB_INVALID_URL, "URL has no host"))?;
        let port = url.port_or_known_default().unwrap_or_default();
        let key = client_cache_key(host, port, addresses);
        if let Some(client) = self
            .clients
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
            .cloned()
        {
            return Ok(client);
        }

        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .user_agent(USER_AGENT)
            .timeout(self.policy.timeout)
            .connect_timeout(self.policy.connect_timeout());
        if host.parse::<IpAddr>().is_err() {
            builder = builder.resolve_to_addrs(host, addresses);
        }
        let client = builder
            .build()
            .map_err(|error| WebError::new(WEB_PROVIDER_ERROR, format!("http client: {error}")))?;

        let mut clients = self
            .clients
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if clients.len() >= CLIENT_CACHE_LIMIT {
            clients.clear();
        }
        clients.insert(key, client.clone());
        Ok(client)
    }

    async fn get_once(
        &self,
        url: &reqwest::Url,
        cancel: &CancelToken,
    ) -> Result<reqwest::Response, WebError> {
        let addresses = self.screen(url, cancel).await?;
        let client = self.client_for(url, &addresses)?;
        let outcome = race_cancel(client.get(url.clone()).send(), cancel).await;
        let response = outcome?.map_err(|error| map_reqwest_error(url, error))?;
        Ok(response)
    }
}

impl ProviderId for HttpFetchProvider {
    fn id(&self) -> &'static str {
        HTTP_PROVIDER_ID
    }

    fn available(&self) -> bool {
        self.policy.max_bytes > 0 && self.policy.timeout > Duration::ZERO
    }
}

impl FetchProviderSpec for HttpFetchProvider {
    async fn fetch(
        &self,
        request: &WebFetchRequest,
        cancel: &CancelToken,
    ) -> Result<WebFetchResult, WebError> {
        let mut current = validate_url(&request.url)?;

        for _hop in 0..=self.policy.max_redirects {
            let response = self.get_once(&current, cancel).await?;
            let status = response.status();

            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        WebError::new(
                            WEB_PROVIDER_ERROR,
                            format!("redirect from {current} carried no usable Location header"),
                        )
                    })?;
                let next = current.join(location).map_err(|error| {
                    WebError::new(
                        WEB_INVALID_URL,
                        format!("redirect target \"{location}\" is not a valid URL: {error}"),
                    )
                })?;
                // A hop must not silently downgrade a validated https request to
                // cleartext http.
                if current.scheme() == "https" && next.scheme() == "http" {
                    return Err(WebError::new(
                        WEB_BLOCKED_URL,
                        format!("refusing redirect from {current} to cleartext {next}"),
                    ));
                }
                drop(response);
                current = validate_url(next.as_str())?;
                continue;
            }

            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            // `classify_content_type` treats a missing header as text, so this only fires
            // for a content type that is present but not representable as text.
            let kind = classify_content_type(&content_type).ok_or_else(|| {
                WebError::new(
                    WEB_UNSUPPORTED_CONTENT_TYPE,
                    format!(
                        "content type \"{}\" cannot be represented as text; fetch an HTML or text resource instead",
                        content_type.trim()
                    ),
                )
            })?;

            let (bytes, truncated) = self.read_body(response, cancel).await?;
            let text = decode_body(&bytes);
            let body = match kind {
                BodyKind::Html => WebFetchBody::Html(text),
                BodyKind::Text => WebFetchBody::Text(text),
            };
            return Ok(WebFetchResult {
                url: current.to_string(),
                status_code: status.as_u16(),
                body,
                content_type,
                truncated,
            });
        }

        Err(WebError::new(
            WEB_TOO_MANY_REDIRECTS,
            format!(
                "more than {} redirects while fetching {}",
                self.policy.max_redirects, request.url
            ),
        ))
    }
}

impl HttpFetchProvider {
    /// Stream the body, keeping at most `max_bytes` and reporting whether it was cut.
    async fn read_body(
        &self,
        mut response: reqwest::Response,
        cancel: &CancelToken,
    ) -> Result<(Vec<u8>, bool), WebError> {
        let url = response.url().clone();
        let mut buffer: Vec<u8> = Vec::new();
        let mut truncated = false;
        loop {
            let outcome = race_cancel(response.chunk(), cancel).await;
            let chunk = outcome?.map_err(|error| map_reqwest_error(&url, error))?;
            let Some(chunk) = chunk else {
                break;
            };
            if buffer.len() + chunk.len() > self.policy.max_bytes {
                let room = self.policy.max_bytes.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..room]);
                truncated = true;
                break;
            }
            buffer.extend_from_slice(&chunk);
        }
        Ok((buffer, truncated))
    }
}

/// Validate the scheme and credential shape of one fetch URL.
pub fn validate_url(url: &str) -> Result<reqwest::Url, WebError> {
    let trimmed = url.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|error| {
        WebError::new(
            WEB_INVALID_URL,
            format!("\"{trimmed}\" is not a valid absolute URL: {error}"),
        )
    })?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(WebError::new(
                WEB_INVALID_URL,
                format!("scheme \"{other}\" is not fetchable; use http or https"),
            ));
        }
    }
    if parsed.host_str().is_none() {
        return Err(WebError::new(WEB_INVALID_URL, "URL has no host"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(WebError::new(
            WEB_INVALID_URL,
            "URLs carrying credentials are refused; fetch the public URL instead",
        ));
    }
    Ok(parsed)
}

/// How a response body is represented after retrieval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Html,
    Text,
}

/// Classify a response content type; `None` means this build cannot represent it.
pub fn classify_content_type(content_type: &str) -> Option<BodyKind> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if essence.is_empty() {
        return Some(BodyKind::Text);
    }
    if essence == "text/html" || essence == "application/xhtml+xml" {
        return Some(BodyKind::Html);
    }
    const TEXTUAL: &[&str] = &[
        "application/json",
        "application/xml",
        "application/javascript",
        "application/x-javascript",
        "application/ecmascript",
        "application/x-ndjson",
        "application/yaml",
        "application/x-yaml",
        "application/rss+xml",
        "application/atom+xml",
        "application/ld+json",
    ];
    if essence.starts_with("text/")
        || TEXTUAL.contains(&essence.as_str())
        || essence.ends_with("+json")
        || essence.ends_with("+xml")
    {
        return Some(BodyKind::Text);
    }
    None
}

/// Decode a body as UTF-8, replacing invalid sequences and dropping a BOM.
fn decode_body(bytes: &[u8]) -> String {
    let without_bom = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    String::from_utf8_lossy(without_bom).to_string()
}

/// Cache key for a pinned client: host, port, and the screened addresses it may use.
///
/// The addresses are sorted so a differently ordered DNS answer still hits the same
/// client, while a genuinely different pin gets its own entry.
fn client_cache_key(host: &str, port: u16, addresses: &[SocketAddr]) -> String {
    let mut addresses = addresses.to_vec();
    addresses.sort_unstable();
    let addresses = addresses
        .iter()
        .map(SocketAddr::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("{host}:{port}|{addresses}")
}

/// Map a transport failure onto the seam's error vocabulary.
fn map_reqwest_error(url: impl std::fmt::Display, error: reqwest::Error) -> WebError {
    if error.is_timeout() {
        return WebError::new(
            WEB_TIMEOUT,
            format!("timed out while fetching {url}: {error}"),
        );
    }
    WebError::new(
        WEB_PROVIDER_ERROR,
        format!("request to {url} failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_non_public_addresses() {
        for blocked in [
            "127.0.0.1",
            "10.0.0.5",
            "172.16.4.4",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "198.18.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "2001::1",
            "2002::1",
            "64:ff9b::1",
        ] {
            let ip: IpAddr = blocked.parse().expect("ip");
            assert!(!is_public_ip(ip), "{blocked} should be blocked");
        }
        for allowed in ["1.1.1.1", "8.8.8.8", "2606:4700::1111"] {
            let ip: IpAddr = allowed.parse().expect("ip");
            assert!(is_public_ip(ip), "{allowed} should be allowed");
        }
    }

    #[test]
    fn client_cache_key_ignores_address_order() {
        let first = SocketAddr::new(IpAddr::from([1, 1, 1, 1]), 443);
        let second = SocketAddr::new(IpAddr::from([8, 8, 8, 8]), 443);
        assert_eq!(
            client_cache_key("example.com", 443, &[first, second]),
            client_cache_key("example.com", 443, &[second, first])
        );
    }

    #[test]
    fn reuses_a_pinned_client_for_the_same_host_and_addresses() {
        let provider = HttpFetchProvider::default();
        let url = validate_url("https://example.com/").expect("url");
        let addresses = vec![SocketAddr::new(IpAddr::from([93, 184, 216, 34]), 443)];
        let _first = provider.client_for(&url, &addresses).expect("client");
        let _second = provider.client_for(&url, &addresses).expect("client");
        assert_eq!(
            provider.clients.lock().expect("lock").len(),
            1,
            "the same host/pin must reuse one pooled client"
        );

        let other = vec![SocketAddr::new(IpAddr::from([93, 184, 216, 35]), 443)];
        let _third = provider.client_for(&url, &other).expect("client");
        assert_eq!(
            provider.clients.lock().expect("lock").len(),
            2,
            "a different pin must get its own client"
        );
    }

    #[test]
    fn dns_and_connect_deadlines_stay_inside_the_request_budget() {
        // A generous request budget still cannot lift the DNS or handshake ceilings.
        let patient = FetchPolicy {
            timeout: Duration::from_secs(120),
            ..FetchPolicy::default()
        };
        assert_eq!(patient.dns_timeout(), DNS_TIMEOUT_CEILING);
        assert_eq!(patient.connect_timeout(), CONNECT_TIMEOUT_CEILING);

        // A tight budget pulls both deadlines down with it instead of outliving it.
        let tight = FetchPolicy {
            timeout: Duration::from_millis(1_500),
            ..FetchPolicy::default()
        };
        assert_eq!(tight.dns_timeout(), Duration::from_millis(1_500));
        assert_eq!(tight.connect_timeout(), Duration::from_millis(1_500));
    }

    #[test]
    fn a_hostname_lookup_under_a_tiny_deadline_fails_instead_of_hanging() {
        let provider = HttpFetchProvider::new(FetchPolicy {
            timeout: Duration::from_millis(1),
            ..FetchPolicy::default()
        });
        let url = validate_url("https://hambur-web-probe.invalid/").expect("url");
        crate::test_support::block_on(async {
            let started = std::time::Instant::now();
            let error = provider
                .screen(&url, &CancelToken::never())
                .await
                .expect_err("no deadline can resolve this in 1 ms");
            assert!(
                error.code() == WEB_TIMEOUT || error.code() == WEB_PROVIDER_ERROR,
                "unexpected code {}",
                error.code()
            );
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "screening outlived its deadline: {:?}",
                started.elapsed()
            );
        });
    }

    #[test]
    fn screening_stops_immediately_once_the_turn_is_cancelled() {
        let provider = HttpFetchProvider::default();
        let url = validate_url("https://example.com/").expect("url");
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let cancel = CancelToken::new(flag);
        crate::test_support::block_on(async {
            let started = std::time::Instant::now();
            let error = provider.screen(&url, &cancel).await.expect_err("cancelled");
            assert_eq!(error.code(), crate::types::WEB_ABORTED);
            assert!(started.elapsed() < Duration::from_secs(1));
        });
    }

    #[test]
    fn validates_scheme_and_credentials() {
        assert!(validate_url("https://example.com/a").is_ok());
        assert!(validate_url("http://example.com").is_ok());
        assert_eq!(
            validate_url("file:///etc/passwd")
                .expect_err("scheme")
                .code(),
            WEB_INVALID_URL
        );
        assert_eq!(
            validate_url("https://user:pass@example.com")
                .expect_err("creds")
                .code(),
            WEB_INVALID_URL
        );
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn classifies_content_types() {
        assert_eq!(
            classify_content_type("text/html; charset=utf-8"),
            Some(BodyKind::Html)
        );
        assert_eq!(
            classify_content_type("application/xhtml+xml"),
            Some(BodyKind::Html)
        );
        assert_eq!(classify_content_type("text/plain"), Some(BodyKind::Text));
        assert_eq!(
            classify_content_type("application/json; charset=utf-8"),
            Some(BodyKind::Text)
        );
        assert_eq!(
            classify_content_type("application/ld+json"),
            Some(BodyKind::Text)
        );
        assert_eq!(classify_content_type(""), Some(BodyKind::Text));
        assert_eq!(classify_content_type("application/pdf"), None);
        assert_eq!(classify_content_type("image/png"), None);
    }

    #[test]
    fn strips_a_utf8_bom() {
        let bytes = [0xEF, 0xBB, 0xBF, b'h', b'i'];
        assert_eq!(decode_body(&bytes), "hi");
    }
}
