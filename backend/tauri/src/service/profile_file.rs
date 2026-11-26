//! Desktop composition adapter for the shared profile filesystem and subscription ports.

use std::{path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use chimera_config::profile::{
    ExternalProfilePath, ManagedProfilePath, Profiles, RemoteProfileOptions,
};
use chimera_contracts::profiles::ports::{
    ProfileFsPort as SharedProfileFsPort,
    ProfileMaterializationPort as SharedProfileMaterializationPort,
    SubscriptionFetcher as SharedSubscriptionFetcher,
};
use chimera_contracts::{bundle_metadata::BundleMetadata, net::SystemProxySource};
use chimera_net::{device_headers::DeviceHeaders, subscription_http::SubscriptionHttpFetcher};
use chimera_profile_io::fs_service::ProfileFileService as FilesystemService;
use url::Url;

use crate::{
    state::profiles::ports::{
        CleanupOutcome, FetchedSubscription, MaterializationReconcileReport,
        MaterializationResource, PreparedCleanup, PreparedMaterialization, ProfileFsPort,
        ProfileMaterializationPort, SubscriptionFetcher,
    },
    utils::path::PathResolver,
};

// The old public path is retained for Legacy callers; its owner is contracts.
pub use chimera_contracts::net::SelfProxyPortSource;

// Legacy tests construct a profile service without consulting host proxy settings.
#[cfg(test)]
struct NoSystemProxyForTests;

#[cfg(test)]
impl SystemProxySource for NoSystemProxyForTests {
    fn system_proxy(&self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

pub struct ProfileFileService {
    files: FilesystemService,
    self_proxy_port: Arc<dyn SelfProxyPortSource>,
    system_proxy: Arc<dyn SystemProxySource>,
    bundle_metadata: BundleMetadata,
    device_headers: DeviceHeaders,
    http_timeout: Duration,
}

impl ProfileFileService {
    pub fn new_with_system_proxy(
        paths: PathResolver,
        self_proxy_port: Arc<dyn SelfProxyPortSource>,
        system_proxy: Arc<dyn SystemProxySource>,
        bundle_metadata: BundleMetadata,
        device_headers: DeviceHeaders,
    ) -> Self {
        Self {
            files: FilesystemService::from_profiles_dir(paths.app_profiles_dir()),
            self_proxy_port,
            system_proxy,
            bundle_metadata,
            device_headers,
            http_timeout: Duration::from_secs(30),
        }
    }

    #[cfg(test)]
    pub fn new(paths: PathResolver, self_proxy_port: Arc<dyn SelfProxyPortSource>) -> Self {
        Self::new_with_system_proxy(
            paths,
            self_proxy_port,
            Arc::new(NoSystemProxyForTests),
            BundleMetadata {
                app_version: crate::utils::dirs::APP_VERSION,
                ..chimera_contracts::bundle_metadata::TEST_BUNDLE_METADATA
            },
            subscription_device_headers(),
        )
    }

    #[cfg(test)]
    fn with_system_proxy_source(mut self, source: Arc<dyn SystemProxySource>) -> Self {
        self.system_proxy = source;
        self
    }

    #[cfg(test)]
    fn with_http_timeout(mut self, timeout: Duration) -> Self {
        self.http_timeout = timeout;
        self
    }

    /// The legacy open-profile command uses the same checked path as the owner.
    pub fn resolve_path(&self, path: &ManagedProfilePath) -> anyhow::Result<PathBuf> {
        Ok(self.files.resolve_path(path)?)
    }
}

pub use chimera_profile_io::yaml::normalize_yaml_document;

impl ProfileFsPort for ProfileFileService {
    fn read(&self, path: &ManagedProfilePath) -> anyhow::Result<String> {
        Ok(SharedProfileFsPort::read(&self.files, path)?)
    }

    fn write_atomic(&self, path: &ManagedProfilePath, content: &str) -> anyhow::Result<()> {
        Ok(SharedProfileFsPort::write_atomic(
            &self.files,
            path,
            content,
        )?)
    }

    fn remove(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        Ok(SharedProfileFsPort::remove(&self.files, path)?)
    }

    fn read_external(&self, target: &ExternalProfilePath) -> anyhow::Result<String> {
        Ok(SharedProfileFsPort::read_external(&self.files, target)?)
    }

    fn ensure_not_symlink(&self, path: &ManagedProfilePath) -> anyhow::Result<()> {
        Ok(SharedProfileFsPort::ensure_not_symlink(&self.files, path)?)
    }

    fn ensure_symlink(
        &self,
        path: &ManagedProfilePath,
        target: &ExternalProfilePath,
    ) -> anyhow::Result<()> {
        Ok(SharedProfileFsPort::ensure_symlink(
            &self.files,
            path,
            target,
        )?)
    }
}

impl ProfileMaterializationPort for ProfileFileService {
    fn prepare_state_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization> {
        Ok(SharedProfileMaterializationPort::prepare_state_first(
            &self.files,
            path,
            resource,
            expected_revision,
        )?)
    }

    fn prepare_file_first(
        &self,
        path: &ManagedProfilePath,
        resource: MaterializationResource,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedMaterialization> {
        Ok(SharedProfileMaterializationPort::prepare_file_first(
            &self.files,
            path,
            resource,
            expected_revision,
        )?)
    }

    fn promote(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        Ok(SharedProfileMaterializationPort::promote(
            &self.files,
            prepared,
        )?)
    }

    fn complete(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        Ok(SharedProfileMaterializationPort::complete(
            &self.files,
            prepared,
        )?)
    }

    fn compensate(&self, prepared: &PreparedMaterialization) -> anyhow::Result<()> {
        Ok(SharedProfileMaterializationPort::compensate(
            &self.files,
            prepared,
        )?)
    }

    fn prepare_cleanup(
        &self,
        path: &ManagedProfilePath,
        expected_revision: u64,
    ) -> anyhow::Result<PreparedCleanup> {
        Ok(SharedProfileMaterializationPort::prepare_cleanup(
            &self.files,
            path,
            expected_revision,
        )?)
    }

    fn activate_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()> {
        Ok(SharedProfileMaterializationPort::activate_cleanup(
            &self.files,
            cleanup,
        )?)
    }

    fn cancel_cleanup(&self, cleanup: &PreparedCleanup) -> anyhow::Result<()> {
        Ok(SharedProfileMaterializationPort::cancel_cleanup(
            &self.files,
            cleanup,
        )?)
    }

    fn retry_cleanup(
        &self,
        cleanup: &PreparedCleanup,
        profiles: &Profiles,
    ) -> anyhow::Result<CleanupOutcome> {
        Ok(SharedProfileMaterializationPort::retry_cleanup(
            &self.files,
            cleanup,
            profiles,
        )?)
    }

    fn reconcile(&self, profiles: &Profiles) -> anyhow::Result<MaterializationReconcileReport> {
        Ok(SharedProfileMaterializationPort::reconcile(
            &self.files,
            profiles,
        )?)
    }
}

/// Existing Profiles Actor still uses an anyhow-based port. Route through the
/// upstream typed HTTP owner and retain this boundary until Profile Actor ports
/// themselves migrate to chimera-contracts.
#[async_trait::async_trait]
impl SubscriptionFetcher for ProfileFileService {
    async fn fetch(
        &self,
        url: &Url,
        options: &RemoteProfileOptions,
    ) -> anyhow::Result<FetchedSubscription> {
        let fetched = SubscriptionHttpFetcher::new(
            self.self_proxy_port.clone(),
            self.bundle_metadata,
            self.system_proxy.clone(),
            self.device_headers.clone(),
        )
        .with_http_timeout(self.http_timeout)
        .fetch(url, options)
        .await
        .with_context(|| format!("subscription download failed: {url}"))?;

        Ok(fetched)
    }
}

/// Hardware identification belongs to the host; the network crate only
/// receives sanitized header values, as in ref's DeviceHeaders boundary.
pub(crate) fn subscription_device_headers() -> DeviceHeaders {
    let info = crate::utils::hwid::get_device_info();
    DeviceHeaders {
        hwid: info.hwid,
        device_os: chimera_net::device_headers::sanitize_for_header(&info.device_os),
        os_version: chimera_net::device_headers::sanitize_for_header(&info.os_version),
        device_model: chimera_net::device_headers::sanitize_for_header(&info.device_model),
    }
}

#[cfg(test)]
use chimera_config::profile::RemoteSyncInterval;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::profiles::ports::SubscriptionFetcher;
    use axum::{
        Router,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::{any, get},
    };
    use std::{
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use url::Url;

    struct NoProxy;
    impl SelfProxyPortSource for NoProxy {
        fn mixed_port(&self) -> Option<u16> {
            None
        }
    }

    struct CountingNoProxy {
        hits: Arc<AtomicUsize>,
    }

    impl SelfProxyPortSource for CountingNoProxy {
        fn mixed_port(&self) -> Option<u16> {
            self.hits.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    struct NoSystemProxy;

    impl SystemProxySource for NoSystemProxy {
        fn system_proxy(&self) -> anyhow::Result<Option<String>> {
            Ok(None)
        }
    }

    struct FixedSystemProxy(String);

    impl SystemProxySource for FixedSystemProxy {
        fn system_proxy(&self) -> anyhow::Result<Option<String>> {
            Ok(Some(self.0.clone()))
        }
    }

    struct FailingSystemProxy(Arc<AtomicUsize>);

    impl SystemProxySource for FailingSystemProxy {
        fn system_proxy(&self) -> anyhow::Result<Option<String>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            anyhow::bail!("injected OS proxy read failed")
        }
    }

    struct FixedSelfProxyPort(u16);

    impl SelfProxyPortSource for FixedSelfProxyPort {
        fn mixed_port(&self) -> Option<u16> {
            Some(self.0)
        }
    }

    fn service() -> (tempfile::TempDir, ProfileFileService) {
        service_with(Arc::new(NoProxy))
    }

    fn service_with(
        self_proxy_port: Arc<dyn SelfProxyPortSource>,
    ) -> (tempfile::TempDir, ProfileFileService) {
        let temp = tempfile::tempdir().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            temp.path().join("config"),
            temp.path().join("data"),
        );
        (
            temp,
            ProfileFileService::new(paths, self_proxy_port)
                .with_system_proxy_source(Arc::new(NoSystemProxy)),
        )
    }

    async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{addr}/"), handle)
    }

    fn options_direct() -> RemoteProfileOptions {
        options(false, false)
    }

    fn options(self_proxy: bool, with_proxy: bool) -> RemoteProfileOptions {
        RemoteProfileOptions {
            user_agent: None,
            with_proxy,
            self_proxy,
            update_interval_minutes: RemoteSyncInterval::default(),
        }
    }

    #[test]
    fn desktop_filesystem_port_preserves_shared_error_and_transaction() {
        use chimera_contracts::profiles::error::ProfileFileError;
        let (_temp, service) = service();
        let path = ManagedProfilePath::new("shared.yaml").unwrap();
        let error = ProfileFsPort::read(&service, &path).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ProfileFileError>(),
            Some(ProfileFileError::ReadFile { source, .. })
                if source.kind() == std::io::ErrorKind::NotFound
        ));
        let prepared = ProfileMaterializationPort::prepare_state_first(
            &service,
            &path,
            MaterializationResource::File {
                content: "proxies: []\n".into(),
            },
            1,
        )
        .unwrap();
        ProfileMaterializationPort::promote(&service, &prepared).unwrap();
        ProfileMaterializationPort::complete(&service, &prepared).unwrap();
        assert_eq!(
            ProfileFsPort::read(&service, &path).unwrap(),
            "proxies: []\n"
        );
    }

    #[test]
    fn normalize_yaml_document_round_trips_mappings_and_rejects_garbage() {
        let normalized = normalize_yaml_document("b: 2\na: 1\n").unwrap();
        let value: serde_yaml::Mapping = serde_yaml::from_str(&normalized).unwrap();
        assert_eq!(value.len(), 2);
        assert!(normalize_yaml_document(": not yaml [").is_err());
    }

    // Pure subscription header parsers and their tests now live in
    // chimera-net/src/subscription.rs (direct upstream source).

    #[tokio::test]
    async fn fetch_parses_userinfo_and_title_headers() {
        let router = Router::new().route(
            "/",
            get(|| async {
                let mut headers = AxumHeaderMap::new();
                headers.insert(
                    "subscription-userinfo",
                    "upload=1; download=2; total=3; expire=0".parse().unwrap(),
                );
                headers.insert("profile-title", "My Sub".parse().unwrap());
                headers.insert("profile-update-interval", "6".parse().unwrap());
                (headers, "proxies: []\n")
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "proxies: []\n");
        assert_eq!(fetched.filename.as_deref(), Some("My Sub"));
        assert_eq!(fetched.subscription.upload, Some(1));
        assert_eq!(fetched.subscription.download, Some(2));
        assert_eq!(fetched.subscription.total, Some(3));
        assert!(fetched.subscription.expire.is_none());
        assert_eq!(fetched.suggested_update_interval_minutes, Some(360));
    }

    #[tokio::test]
    async fn fetch_sends_default_user_agent_and_hwid() {
        let seen = Arc::new(Mutex::new(None::<(String, bool)>));
        let seen_request = Arc::clone(&seen);
        let router = Router::new().route(
            "/",
            get(move |headers: AxumHeaderMap| {
                let seen_request = Arc::clone(&seen_request);
                async move {
                    let ua = headers
                        .get(header::USER_AGENT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    let has_hwid = headers
                        .get("x-hwid")
                        .and_then(|v| v.to_str().ok())
                        .is_some_and(|value| !value.trim().is_empty());
                    *seen_request.lock().unwrap() = Some((ua, has_hwid));
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service();
        service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();

        let (ua, has_hwid) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            ua,
            format!("clash-chimera/v{}", crate::utils::dirs::APP_VERSION)
        );
        assert!(has_hwid);
    }

    #[tokio::test]
    async fn fetch_falls_back_to_direct_when_self_proxy_port_is_unavailable() {
        let hits = Arc::new(AtomicUsize::new(0));
        let source_hits = Arc::new(AtomicUsize::new(0));
        let request_hits = Arc::clone(&hits);
        let router = Router::new().route(
            "/",
            get(move || {
                let request_hits = Arc::clone(&request_hits);
                async move {
                    request_hits.fetch_add(1, Ordering::SeqCst);
                    "ok: true\n"
                }
            }),
        );
        let (url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(true, false))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn fetch_continues_proxy_chain_when_self_proxy_port_is_unavailable() {
        let source_hits = Arc::new(AtomicUsize::new(0));
        let proxy_hits = Arc::new(AtomicUsize::new(0));
        let request_hits = Arc::clone(&proxy_hits);
        let router = Router::new().fallback(any(move || {
            let request_hits = Arc::clone(&request_hits);
            async move {
                request_hits.fetch_add(1, Ordering::SeqCst);
                "ok: true\n"
            }
        }));
        let (proxy_url, _server) = serve(router).await;
        let (_temp, service) = service_with(Arc::new(CountingNoProxy {
            hits: Arc::clone(&source_hits),
        }));
        let service = service.with_system_proxy_source(Arc::new(FixedSystemProxy(
            proxy_url.trim_end_matches('/').to_owned(),
        )));

        let fetched = service
            .fetch(
                &Url::parse("http://subscription.invalid/profile.yaml").unwrap(),
                &options(true, true),
            )
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(source_hits.load(Ordering::SeqCst), 1);
        assert_eq!(proxy_hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_system_proxy_read_keeps_direct_subscription_fallback() {
        let reads = Arc::new(AtomicUsize::new(0));
        let (url, _server) = serve(Router::new().route("/", get(|| async { "ok: true\n" }))).await;
        let (_temp, service) = service();
        let service = service.with_system_proxy_source(Arc::new(FailingSystemProxy(reads.clone())));

        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options(false, true))
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(reads.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn self_proxy_has_priority_without_reading_system_proxy() {
        let (proxy_url, _server) =
            serve(Router::new().fallback(any(|| async { "ok: true\n" }))).await;
        let port = Url::parse(&proxy_url).unwrap().port().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let (_temp, service) = service_with(Arc::new(FixedSelfProxyPort(port)));
        let service = service.with_system_proxy_source(Arc::new(FailingSystemProxy(reads.clone())));

        let fetched = service
            .fetch(
                &Url::parse("http://subscription.invalid/profile.yaml").unwrap(),
                &options(true, true),
            )
            .await
            .unwrap();

        assert_eq!(fetched.content, "ok: true\n");
        assert_eq!(reads.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn fetch_retries_transient_errors_but_not_auth_failures() {
        let hits = Arc::new(AtomicUsize::new(0));
        let flaky_hits = Arc::clone(&hits);
        let flaky = Router::new().route(
            "/",
            get(move || {
                let flaky_hits = Arc::clone(&flaky_hits);
                async move {
                    if flaky_hits.fetch_add(1, Ordering::SeqCst) == 0 {
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    } else {
                        "ok: true\n".into_response()
                    }
                }
            }),
        );
        let (url, _server) = serve(flaky).await;
        let (_temp, service) = service();
        let fetched = service
            .fetch(&Url::parse(&url).unwrap(), &options_direct())
            .await
            .unwrap();
        assert_eq!(fetched.content, "ok: true\n");
        assert!(hits.load(Ordering::SeqCst) >= 2);

        for status in [
            StatusCode::FORBIDDEN,
            StatusCode::UNAUTHORIZED,
            StatusCode::NOT_FOUND,
        ] {
            let hits = Arc::new(AtomicUsize::new(0));
            let status_hits = Arc::clone(&hits);
            let router = Router::new().route(
                "/",
                get(move || {
                    let status_hits = Arc::clone(&status_hits);
                    async move {
                        status_hits.fetch_add(1, Ordering::SeqCst);
                        status
                    }
                }),
            );
            let (url, _server) = serve(router).await;
            assert!(
                service
                    .fetch(&Url::parse(&url).unwrap(), &options_direct())
                    .await
                    .is_err()
            );
            assert_eq!(hits.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn fetch_timeout_is_managed_internally() {
        let slow = Router::new().route(
            "/",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(60)).await;
                "never"
            }),
        );
        let (url, _server) = serve(slow).await;
        let (_temp, service) = service();
        let service = service.with_http_timeout(Duration::from_millis(300));
        let started = Instant::now();
        assert!(
            service
                .fetch(&Url::parse(&url).unwrap(), &options_direct())
                .await
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
