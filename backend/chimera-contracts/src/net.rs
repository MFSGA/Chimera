//! Network ports shared by application features and their host adapters.

use anyhow::Result;
use async_trait::async_trait;

/// Reads the mixed proxy port currently owned by the application runtime.
pub trait SelfProxyPortSource: Send + Sync + 'static {
    fn mixed_port(&self) -> Option<u16>;
}

/// Fetches release notes through the host's configured network policy.
#[async_trait]
pub trait ChangelogHttp: Send + Sync + 'static {
    async fn get_text(&self, url: &url::Url) -> Result<String>;
}

/// Reads the currently enabled OS proxy URL whenever a network client is created.
/// A disabled system proxy is represented as `None`, matching the pinned ref.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait SystemProxySource: Send + Sync + 'static {
    fn system_proxy(&self) -> Result<Option<String>>;
}

/// Downloads an icon's media type and bytes using the caller's current self-proxy port.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait IconFetcher: Send + Sync + 'static {
    /// Returns the content type and response bytes, preserving request/body failures.
    async fn fetch(&self, url: &url::Url, self_proxy_port: u16) -> Result<(String, bytes::Bytes)>;
}

/// Probes network endpoints using the self-proxy port supplied for each operation.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
#[async_trait::async_trait]
pub trait NetworkDiagnostics: Send + Sync + 'static {
    /// Measures milliseconds after a warm-up request; no matching response is `None`.
    async fn url_delay_test(
        &self,
        url: &str,
        expected_status: u16,
        self_proxy_port: u16,
    ) -> Option<u64>;

    /// Returns the ASN service's JSON document without reshaping it.
    async fn get_ipsb_asn(&self, self_proxy_port: u16) -> Result<serde_json::Value>;
}
