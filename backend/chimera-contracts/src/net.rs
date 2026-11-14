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
