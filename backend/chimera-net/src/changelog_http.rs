//! Changelog HTTP adapter with live self and system proxy sources.

use std::{sync::Arc, time::Duration};

use anyhow::{Context as _, Result};
use async_trait::async_trait;

use chimera_contracts::{
    bundle_metadata::BundleMetadata,
    net::{SelfProxyPortSource, SystemProxySource},
};

/// Fetches release notes with the bundle user agent and existing HTTP deadlines.
pub struct ReqwestChangelogHttp {
    proxy_port: Arc<dyn SelfProxyPortSource>,
    bundle_metadata: BundleMetadata,
    system_proxy: Arc<dyn SystemProxySource>,
}

impl ReqwestChangelogHttp {
    /// Retains metadata and readers; proxy settings are resolved for each request.
    pub fn new(
        proxy_port: Arc<dyn SelfProxyPortSource>,
        bundle_metadata: BundleMetadata,
        system_proxy: Arc<dyn SystemProxySource>,
    ) -> Self {
        Self {
            proxy_port,
            bundle_metadata,
            system_proxy,
        }
    }
}

#[async_trait]
impl chimera_contracts::net::ChangelogHttp for ReqwestChangelogHttp {
    async fn get_text(&self, url: &url::Url) -> Result<String> {
        let mut builder = reqwest::Client::builder()
            .user_agent(format!("Chimera/{}", self.bundle_metadata.package_version))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(30));
        if let Some(port) = self.proxy_port.mixed_port() {
            builder = builder.proxy(reqwest::Proxy::all(crate::policy::get_self_proxy(port))?);
        }
        if let Ok(Some(proxy)) = self.system_proxy.system_proxy() {
            builder = builder.proxy(reqwest::Proxy::all(proxy).context("invalid system proxy")?);
        }
        Ok(builder
            .build()?
            .get(url.clone())
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_contracts::{bundle_metadata::TEST_BUNDLE_METADATA, net::ChangelogHttp as _};
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    struct NoSelfProxy;

    impl SelfProxyPortSource for NoSelfProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    struct ChangingSystemProxy {
        url: Mutex<Option<String>>,
        reads: AtomicUsize,
    }

    impl SystemProxySource for ChangingSystemProxy {
        fn system_proxy(&self) -> Result<Option<String>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.url.lock().unwrap().clone())
        }
    }

    async fn proxy_server(body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = axum::Router::new().fallback(move || async move { body });
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{address}"), handle)
    }

    #[tokio::test]
    async fn each_changelog_request_reads_the_current_system_proxy() {
        let (first, _first_server) = proxy_server("first release notes").await;
        let (second, _second_server) = proxy_server("second release notes").await;
        let source = Arc::new(ChangingSystemProxy {
            url: Mutex::new(Some(first)),
            reads: AtomicUsize::new(0),
        });
        let http =
            ReqwestChangelogHttp::new(Arc::new(NoSelfProxy), TEST_BUNDLE_METADATA, source.clone());
        // Only the test proxy can resolve this intentionally nonexistent domain.
        let url = url::Url::parse("http://changelog.invalid/notes").unwrap();
        assert_eq!(http.get_text(&url).await.unwrap(), "first release notes");
        *source.url.lock().unwrap() = Some(second);
        assert_eq!(http.get_text(&url).await.unwrap(), "second release notes");
        assert_eq!(source.reads.load(Ordering::SeqCst), 2);
    }
}
