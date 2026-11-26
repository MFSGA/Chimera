//! Shared application facade over the already-composed actor owners.
//! Legacy desktop mutation orchestration is still supplied by the shell.
use super::{
    app_update::AppUpdateClient,
    application::ApplicationClient,
    clash_config::ClashConfigClient,
    clash_info::ClashInfo,
    core_lifecycle::ports::CoreLifecyclePort,
    geo_mode::ActiveCoreGeodataMode,
    profiles,
    session_state::SessionStateClient,
    traffic::{self, SelectedProfile, SettingsRetention, SystemClock},
};
use crate::{
    actor_v2::{
        CoreClient as CoreClientV2, router::CoreStatusProjection, service_actor::ServiceClient,
        service_router::ServiceHostStatus,
    },
    clash::ws::ClashConnectionsFrame,
    geo::{CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexClient},
    traffic::{TrafficArgs, TrafficClient},
};
use anyhow::{Context as _, Result, anyhow};
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::ClashConfig,
    profile::{ProfileId, Profiles},
};
use chimera_contracts::{
    bundle_metadata::BundleMetadata,
    net::{ChangelogHttp, DirectEgress, DirectEgressProbe, NetworkDiagnostics},
    system_dns::SystemDnsCache,
};
use chimera_traffic::{
    ClosedCursor, ClosedPage, Dimension, Metric, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficReport, TrafficSummary, UsageCursor, UsageGroup, UsagePage,
};
use std::sync::Arc;
use tokio::sync::watch;

mod profile_operations;
pub use profile_operations::auto_activation_failure_degradation;

pub struct ClientParts {
    pub reconciler: Option<Arc<dyn super::reconciler::facade::RuntimeReconciler>>,
    pub profile_fs: Arc<dyn crate::state::profiles::ports::ProfileFsPort>,
    pub profile_paths: chimera_profile_io::fs_service::ProfileFileService,
    pub ui_sink: Arc<dyn super::event_sink::UiEventSink>,
    pub application: ApplicationClient,
    pub session_state: SessionStateClient,
    pub app_updater: Option<AppUpdateClient>,
    pub changelog_http: Option<Arc<dyn ChangelogHttp>>,
    pub network_diagnostics: Arc<dyn NetworkDiagnostics>,
    pub icons: crate::service::icon::IconService,
    pub bundle_metadata: Option<BundleMetadata>,
    pub core_api: Option<CoreClientV2>,
    pub service_actor: Option<ServiceClient>,
    pub core: Arc<dyn CoreLifecyclePort>,
    pub shutdown: tokio_util::sync::CancellationToken,
    pub tasks: tokio_util::task::TaskTracker,
    pub file_logs: Option<crate::client::logs::FileLogsClient>,
    pub streams: crate::clash::ws::StreamsClient,
    pub streams_shutdown: tokio_util::sync::CancellationToken,
    pub proxies: crate::proxies::ProxiesClient,
    pub typed_profiles: Option<profiles::ProfilesClient>,
    pub system_dns: Arc<dyn SystemDnsCache>,
    pub clash_config: ClashConfigClient,
    pub direct_egress: Arc<dyn DirectEgressProbe>,
}

#[derive(Clone)]
pub struct ChimeraClient {
    inner: Arc<ChimeraClientInner>,
}

#[derive(Clone)]
pub struct WeakChimeraClient(std::sync::Weak<ChimeraClientInner>);

impl WeakChimeraClient {
    #[must_use]
    pub fn upgrade(&self) -> Option<ChimeraClient> {
        self.0.upgrade().map(|inner| ChimeraClient { inner })
    }
}

struct ChimeraClientInner {
    reconciler: Option<Arc<dyn super::reconciler::facade::RuntimeReconciler>>,
    profile_fs: Arc<dyn crate::state::profiles::ports::ProfileFsPort>,
    profile_paths: chimera_profile_io::fs_service::ProfileFileService,
    ui_sink: Arc<dyn super::event_sink::UiEventSink>,
    application: ApplicationClient,
    session_state: SessionStateClient,
    app_updater: Option<AppUpdateClient>,
    changelog_http: Option<Arc<dyn ChangelogHttp>>,
    network_diagnostics: Arc<dyn NetworkDiagnostics>,
    icons: crate::service::icon::IconService,
    bundle_metadata: Option<BundleMetadata>,
    core_api: Option<CoreClientV2>,
    service_actor: Option<ServiceClient>,
    ref_facade: Option<crate::actor_v2::facade::CoreFacade>,
    core: Arc<dyn CoreLifecyclePort>,
    shutdown: tokio_util::sync::CancellationToken,
    tasks: tokio_util::task::TaskTracker,
    file_logs: Option<crate::client::logs::FileLogsClient>,
    streams: crate::clash::ws::StreamsClient,
    streams_shutdown: tokio_util::sync::CancellationToken,
    proxies: crate::proxies::ProxiesClient,
    typed_profiles: Option<profiles::ProfilesClient>,
    system_dns: Arc<dyn SystemDnsCache>,
    traffic_store: parking_lot::Mutex<Option<Arc<crate::traffic::TrafficFileStorage>>>,
    traffic: parking_lot::RwLock<Option<crate::traffic::TrafficClient>>,
    local_source: Arc<traffic::LocalSourceCache>,
    geo_index: parking_lot::RwLock<Option<crate::geo::GeoIndexClient>>,
    clash_config: ClashConfigClient,
    direct_egress: Arc<dyn DirectEgressProbe>,
}

impl Drop for ChimeraClientInner {
    fn drop(&mut self) {
        self.streams_shutdown.cancel();
    }
}

impl ChimeraClient {
    pub fn with_parts(parts: ClientParts) -> Self {
        let ref_facade = parts
            .core_api
            .as_ref()
            .zip(parts.service_actor.as_ref())
            .map(|(core, service)| {
                crate::actor_v2::facade::CoreFacade::new(core.clone(), service.clone())
            });
        let local_source = Arc::new(traffic::LocalSourceCache::new(
            parts.application.clone(),
            parts.clash_config.clone(),
        ));
        Self {
            inner: Arc::new(ChimeraClientInner {
                reconciler: parts.reconciler,
                profile_fs: parts.profile_fs,
                profile_paths: parts.profile_paths,
                ui_sink: parts.ui_sink,
                application: parts.application,
                session_state: parts.session_state,
                app_updater: parts.app_updater,
                changelog_http: parts.changelog_http,
                network_diagnostics: parts.network_diagnostics,
                icons: parts.icons,
                bundle_metadata: parts.bundle_metadata,
                core_api: parts.core_api,
                service_actor: parts.service_actor,
                core: parts.core,
                shutdown: parts.shutdown,
                tasks: parts.tasks,
                file_logs: parts.file_logs,
                streams: parts.streams,
                streams_shutdown: parts.streams_shutdown,
                proxies: parts.proxies,
                typed_profiles: parts.typed_profiles,
                system_dns: parts.system_dns,
                clash_config: parts.clash_config,
                direct_egress: parts.direct_egress,
                ref_facade,
                local_source,
                traffic_store: parking_lot::Mutex::new(None),
                traffic: parking_lot::RwLock::new(None),
                geo_index: parking_lot::RwLock::new(None),
            }),
        }
    }

    /// Real actor handle supplied by the composition root. Only isolated
    /// read-only facade fixtures may omit it.
    pub fn reconciler(&self) -> Option<&Arc<dyn super::reconciler::facade::RuntimeReconciler>> {
        self.inner.reconciler.as_ref()
    }

    fn runtime_reconciler(&self) -> Result<&dyn super::reconciler::facade::RuntimeReconciler> {
        self.reconciler()
            .map(Arc::as_ref)
            .context("runtime reconciler is not connected")
    }

    pub async fn start_core(&self) -> Result<()> {
        self.runtime_reconciler()?.start_core().await
    }
    pub async fn stop_core(&self) -> Result<()> {
        self.runtime_reconciler()?.stop_core().await
    }
    pub async fn shutdown_core(&self) -> Result<()> {
        self.runtime_reconciler()?.shutdown().await
    }
    pub async fn retry_configuration_runtime(&self) -> Result<()> {
        anyhow::ensure!(!self.shutdown().is_cancelled(), "client is shutting down");
        self.runtime_reconciler()?.reconcile().await
    }
    pub async fn replace_core_binary(
        &self,
        artifact: super::core_lifecycle::ports::PreparedCoreBinary,
    ) -> Result<()> {
        self.runtime_reconciler()?
            .replace_core_binary(artifact)
            .await
    }

    #[cfg(feature = "test-support")]
    pub fn test_set_reconciler(
        &mut self,
        reconciler: Arc<dyn super::reconciler::facade::RuntimeReconciler>,
    ) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .reconciler = Some(reconciler);
    }

    pub fn downgrade(&self) -> WeakChimeraClient {
        WeakChimeraClient(Arc::downgrade(&self.inner))
    }
    pub(crate) fn clash_config_owner(&self) -> &ClashConfigClient {
        &self.inner.clash_config
    }
    pub fn application(&self) -> &ApplicationClient {
        &self.inner.application
    }
    pub fn session_state(&self) -> &SessionStateClient {
        &self.inner.session_state
    }
    pub fn core_api(&self) -> &Option<CoreClientV2> {
        &self.inner.core_api
    }
    pub fn service_actor(&self) -> &Option<ServiceClient> {
        &self.inner.service_actor
    }
    pub fn ref_facade(&self) -> &Option<crate::actor_v2::facade::CoreFacade> {
        &self.inner.ref_facade
    }
    pub fn core(&self) -> &Arc<dyn CoreLifecyclePort> {
        &self.inner.core
    }
    pub fn shutdown(&self) -> &tokio_util::sync::CancellationToken {
        &self.inner.shutdown
    }
    pub fn tasks(&self) -> &tokio_util::task::TaskTracker {
        &self.inner.tasks
    }
    pub fn proxies(&self) -> &crate::proxies::ProxiesClient {
        &self.inner.proxies
    }
    pub fn typed_profiles(&self) -> &Option<profiles::ProfilesClient> {
        &self.inner.typed_profiles
    }
    #[cfg(feature = "test-support")]
    pub fn test_set_file_logs(&mut self, value: crate::client::logs::FileLogsClient) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .file_logs = Some(value);
    }

    #[cfg(feature = "test-support")]
    pub fn test_set_core_api(&mut self, value: Option<CoreClientV2>) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .core_api = value;
    }
    #[cfg(feature = "test-support")]
    pub fn test_set_service_actor(&mut self, value: Option<ServiceClient>) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .service_actor = value;
    }
    #[cfg(feature = "test-support")]
    pub fn test_set_ref_facade(&mut self, value: Option<crate::actor_v2::facade::CoreFacade>) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .ref_facade = value;
    }
    #[cfg(feature = "test-support")]
    pub fn test_set_typed_profiles(&mut self, value: profiles::ProfilesClient) {
        Arc::get_mut(&mut self.inner)
            .expect("one shared fixture owner")
            .typed_profiles = Some(value);
    }

    #[cfg(feature = "test-support")]
    pub fn test_set_application(&mut self, value: ApplicationClient) {
        let inner = Arc::get_mut(&mut self.inner).expect("one shared fixture owner");
        inner.local_source = Arc::new(traffic::LocalSourceCache::new(
            value.clone(),
            inner.clash_config.clone(),
        ));
        inner.application = value;
    }

    /// Optional geography owner is started before the traffic observer; failure
    /// leaves geography unknown instead of preventing the client from starting.
    pub async fn initialize_traffic(
        &self,
        store: Option<Arc<crate::traffic::TrafficFileStorage>>,
        data_dir: std::path::PathBuf,
        cache_dir: std::path::PathBuf,
    ) -> Result<()> {
        let enabled = store.is_some();
        *self.inner.traffic_store.lock() = store;
        if enabled {
            let core = self
                .inner
                .core_api
                .as_ref()
                .context("production geodata needs the ref CoreActor")?
                .clone();
            let geo = GeoIndexClient::spawn(
                crate::geo::GeoIndexArgs {
                    source: Arc::new(crate::geo::FsCountryIndexSource::new(
                        data_dir,
                        cache_dir.join("geodata-index"),
                    )),
                    mode_source: Arc::new(ActiveCoreGeodataMode(core)),
                },
                self.inner.shutdown.clone(),
                &self.inner.tasks,
            )
            .await;
            match geo {
                Ok(index) => *self.inner.geo_index.write() = Some(index),
                Err(error) => tracing::warn!(%error, "traffic geography index is unavailable"),
            }
        }
        Ok(())
    }

    pub async fn url_delay_test(&self, url: &str, expected_status: u16) -> Option<u64> {
        self.inner
            .network_diagnostics
            .url_delay_test(url, expected_status, self.clash_info().port)
            .await
    }

    pub async fn get_ipsb_asn(&self) -> anyhow::Result<serde_json::Value> {
        self.inner
            .network_diagnostics
            .get_ipsb_asn(self.clash_info().port)
            .await
    }

    pub async fn get_cached_icon(&self, url: &str) -> anyhow::Result<(String, bytes::Bytes)> {
        self.inner
            .icons
            .get_cached_icon(url, self.clash_info().port)
            .await
    }

    /// Read the ref Core Actor's authoritative host and generation projection.
    /// The legacy CoreLifecyclePort remains a Chimera runtime-config adapter,
    /// not the owner of Core routing state.
    pub fn routed_core_status(&self) -> Option<CoreStatusProjection> {
        if let Some(facade) = &self.inner.ref_facade {
            return Some(facade.core_status());
        }
        self.inner.core_api.as_ref().map(CoreClientV2::status)
    }

    /// Read the ref Service Actor directly, without asking the old Facade to
    /// reconstruct its own Service phase from a separate IPC status cache.
    pub fn service_actor_status(&self) -> Option<ServiceHostStatus> {
        if let Some(facade) = &self.inner.ref_facade {
            return Some(facade.service_status());
        }
        self.inner.service_actor.as_ref().map(ServiceClient::status)
    }

    pub fn shutdown_child_token(&self) -> tokio_util::sync::CancellationToken {
        crate::client::app_lifecycle::shutdown_child_token(&self.inner.shutdown)
    }

    pub fn spawn_tracked(
        &self,
        task: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> bool {
        crate::client::app_lifecycle::spawn_tracked(&self.inner.shutdown, &self.inner.tasks, task)
    }

    pub fn file_logs(&self) -> chimera_logging::LogResult<&crate::client::logs::FileLogsClient> {
        self.inner
            .file_logs
            .as_ref()
            .ok_or(chimera_logging::LogError::Unavailable)
    }

    pub fn streams(&self) -> &crate::clash::ws::StreamsClient {
        &self.inner.streams
    }

    pub fn profiles_client(&self) -> anyhow::Result<&profiles::ProfilesClient> {
        self.inner
            .typed_profiles
            .as_ref()
            .ok_or_else(|| anyhow!("typed ProfilesClient is not configured"))
    }

    pub fn get_profiles(&self) -> anyhow::Result<Arc<Profiles>> {
        Ok(self.profiles_client()?.snapshot())
    }

    /// The secret-free profile listing for read-only consumers. The full
    /// source document remains available through `get_profiles` for editors.
    pub fn get_profiles_listing(&self) -> anyhow::Result<chimera_config::profile::ProfilesListing> {
        Ok((&*self.profiles_client()?.snapshot()).into())
    }

    pub async fn get_transparent_proxy_status(
        &self,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        if let Some(core) = &self.inner.core_api {
            return Ok(core.transparent_proxy_status().await?);
        }
        // Test-only legacy lifecycle injection.
        self.inner.core.transparent_proxy_status().await
    }

    pub async fn profile_sync_status(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<profiles::ProfileSyncStatus> {
        Ok(self.profiles_client()?.profile_sync_status(uid).await?)
    }

    pub async fn profile_sync_runs(
        &self,
        uid: ProfileId,
        after: Option<chimera_jobs::dto::RunCursorDto>,
    ) -> anyhow::Result<chimera_jobs::dto::RunPageDto> {
        Ok(self
            .profiles_client()?
            .profile_sync_runs(uid, after)
            .await?)
    }

    pub async fn profile_sync_logs(
        &self,
        uid: ProfileId,
        run: String,
        after: Option<String>,
    ) -> anyhow::Result<chimera_jobs::dto::LogPageDto> {
        Ok(self
            .profiles_client()?
            .profile_sync_logs(uid, run, after)
            .await?)
    }

    pub fn get_app_config(&self) -> anyhow::Result<ChimeraAppConfig> {
        Ok(self.inner.application.get_typed())
    }

    /// Which runtime keys the committed core selection actually supports.
    /// The same matrix is used by the config builder and the settings UI.
    pub fn config_features(&self) -> chimera_config::clash::features::ConfigFeatures {
        self.inner.application.get_typed().core.config_features()
    }

    fn app_updater(&self) -> anyhow::Result<&crate::client::app_update::AppUpdateClient> {
        self.inner
            .app_updater
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("application updater is unavailable"))
    }

    pub async fn get_app_update_state(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.state().await?)
    }

    pub async fn check_app_update(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.check().await?)
    }

    pub async fn download_app_update(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.download().await?)
    }

    pub async fn cancel_app_update_download(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.cancel_download().await?)
    }

    pub async fn install_app_update(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.install().await?)
    }

    pub async fn discard_app_update_package(
        &self,
    ) -> anyhow::Result<crate::client::app_update::AppUpdateSnapshot> {
        Ok(self.app_updater()?.discard().await?)
    }

    pub async fn get_app_update_changelog(
        &self,
        from: &str,
        to: &str,
    ) -> anyhow::Result<crate::client::app_update::changelog::AppUpdateChangelog> {
        let http = self
            .inner
            .changelog_http
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("application update changelog is unavailable"))?;
        let sources = self.inner.application.get_typed().update_sources;
        Ok(crate::client::app_update::changelog::resolve_changelog(
            http.as_ref(),
            &sources,
            from,
            to,
        )
        .await?)
    }

    pub fn installed_release_channel(&self) -> Option<chimera_config::application::ReleaseChannel> {
        self.inner
            .bundle_metadata
            .map(|metadata| metadata.release_channel)
    }

    pub fn release_channel(&self) -> anyhow::Result<chimera_config::application::ReleaseChannel> {
        let metadata = self
            .inner
            .bundle_metadata
            .ok_or_else(|| anyhow::anyhow!("application bundle metadata is unavailable"))?;
        Ok(metadata
            .release_channel
            .resolve(self.inner.application.get_typed().release_channel))
    }

    pub fn configure_app_update(&self, config: &ChimeraAppConfig) {
        let (Some(updater), Some(metadata)) = (&self.inner.app_updater, self.inner.bundle_metadata)
        else {
            return;
        };
        updater.configure(crate::client::app_update::AppUpdateSettings {
            channel: config.release_channel.unwrap_or(metadata.release_channel),
            sources: config.update_sources.clone(),
            auto_check: config.enable_auto_check_update,
            auto_download: config.enable_auto_download_update,
        });
    }

    pub fn get_clash_config(&self) -> Result<ClashConfig> {
        self.inner.clash_config.get()
    }

    pub fn clash_info(&self) -> ClashInfo {
        self.inner.core.effective_clash_info()
    }

    /// A fresh ref CoreActor projection for diagnostics that must not trust
    /// the legacy Facade's controller cache. The caller is responsible for
    /// checking host/generation before and after using a leased API client.
    pub async fn refreshed_routed_core_status(
        &self,
    ) -> Result<crate::actor_v2::router::CoreStatusProjection> {
        if let Some(facade) = &self.inner.ref_facade {
            return Ok(facade.refresh_status().await?);
        }
        let core = self
            .inner
            .core_api
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("the ref Core Actor is unavailable"))?;
        Ok(core.refresh_status().await?)
    }

    pub async fn flush_system_dns_cache(&self) -> anyhow::Result<()> {
        let system_dns = self.inner.system_dns.clone();
        tokio::task::spawn_blocking(move || system_dns.flush())
            .await
            .context("system DNS cache flush task failed")??;
        Ok(())
    }
    pub async fn probe_direct_egress(&self) -> anyhow::Result<DirectEgress> {
        let geo = self.inner.geo_index.read().clone();
        Ok(crate::client::direct_egress::probe(
            &self.inner.local_source,
            self.inner.direct_egress.as_ref(),
            geo.as_ref(),
        )
        .await)
    }

    pub async fn start_traffic(
        &self,
        frames: tokio::sync::watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
    ) -> Result<()> {
        let Some(store) = self.inner.traffic_store.lock().take() else {
            return Ok(());
        };
        let profiles = self
            .profiles_client()
            .context("traffic recording needs committed profiles")?
            .clone();
        let geo_index = self.inner.geo_index.read().clone();
        let geo = geo_index
            .as_ref()
            .map(GeoIndexClient::subscribe)
            .unwrap_or_else(|| {
                let (_, receiver) = watch::channel(None::<Arc<CountryIndexSnapshot>>);
                receiver
            });
        let geo_mode_source = geo_index
            .as_ref()
            .and_then(|_| self.inner.core_api.as_ref())
            .map(|core| {
                Arc::new(ActiveCoreGeodataMode(core.clone())) as Arc<dyn CoreGeodataModeSource>
            });
        let args = TrafficArgs {
            store,
            profiles: Arc::new(SelectedProfile::new(profiles.snapshot_handle())),
            retention: Arc::new(SettingsRetention::new(self.inner.application.clone())),
            clock: Arc::new(SystemClock),
            frames,
            geo,
            geo_mode_source,
            geo_mode_refresh: geo_index,
            local_source: self.inner.local_source.clone(),
        };
        let traffic =
            crate::traffic::spawn(args, self.inner.shutdown.child_token(), &self.inner.tasks)
                .await
                .context("failed to start traffic recording actor")?;
        *self.inner.traffic.write() = Some(traffic);
        Ok(())
    }

    fn traffic(&self) -> Result<TrafficClient> {
        self.inner
            .traffic
            .read()
            .clone()
            .ok_or_else(|| anyhow!("traffic recording is unavailable"))
    }

    pub async fn traffic_summary(&self) -> Result<TrafficSummary> {
        self.traffic()?.summary().await
    }

    pub async fn query_traffic_report(&self, request: ReportRequest) -> Result<TrafficReport> {
        self.traffic()?.report(request).await
    }

    pub async fn query_traffic_usage(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        metric: Metric,
        after: Option<UsageCursor>,
        limit: usize,
    ) -> Result<UsagePage> {
        self.traffic()?
            .usage(query, group_by, metric, after, limit)
            .await
    }

    pub async fn query_traffic_usage_by_keys(
        &self,
        query: TrafficQuery,
        group_by: Dimension,
        keys: Vec<String>,
    ) -> Result<Vec<UsageGroup>> {
        self.traffic()?.usage_by_keys(query, group_by, keys).await
    }

    pub async fn query_traffic_closed_connections(
        &self,
        range: TrafficRange,
        filters: Vec<TrafficFilter>,
        before: Option<ClosedCursor>,
        limit: usize,
    ) -> Result<ClosedPage> {
        self.traffic()?
            .closed_connections(range, filters, before, limit)
            .await
    }

    pub async fn query_traffic_active_connection_ids(
        &self,
        filters: Vec<TrafficFilter>,
    ) -> Result<Vec<String>> {
        self.traffic()?.active_connection_ids(filters).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::core_lifecycle::status::{CoreStatusSnapshot, RunType};
    use chimera_config::application::ClashCore;
    use chimera_contracts::{
        net::{MockDirectEgressProbe, MockIconFetcher, MockNetworkDiagnostics},
        system_dns::MockSystemDnsCache,
    };
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    // The facade must not use the legacy lifecycle for status while the actual
    // ref CoreActor is present. No production owner is substituted.
    struct Port;
    #[async_trait::async_trait]
    impl CoreLifecyclePort for Port {
        fn effective_clash_info(&self) -> ClashInfo {
            ClashInfo {
                port: 17321,
                ..Default::default()
            }
        }
        async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> Result<()> {
            panic!("read-only facade test must not reconcile")
        }
        async fn stop(&self) -> Result<()> {
            panic!("unexpected legacy stop")
        }
        async fn change_core(&self, _: ClashCore) -> Result<()> {
            panic!("unexpected legacy mutation")
        }
        async fn status(&self) -> Result<CoreStatusSnapshot> {
            panic!("legacy status must not be used")
        }
        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }
        async fn on_profile_change(&self, _: bool) {
            panic!("unexpected legacy mutation")
        }
    }

    #[tokio::test]
    async fn shared_facade_owns_real_handles_and_weak_reference_does_not_extend_lifetime() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths =
            chimera_paths::PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        let shutdown = CancellationToken::new();
        let streams_shutdown = shutdown.child_token();
        let tasks = TaskTracker::new();
        let core = CoreClientV2::spawn(crate::actor_v2::test_support::endpoint(
            "http://127.0.0.1:1/".into(),
        ))
        .await
        .unwrap();
        let logs = crate::logs::CoreLogsClient::spawn(
            Box::new(crate::logs::UnavailableCoreLogStore("test".into())),
            Default::default(),
            streams_shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let streams = crate::clash::ws::StreamsClient::spawn(
            core.clone(),
            logs,
            chimera_config::clash::config::overrides::LogLevel::Info,
            streams_shutdown.clone(),
            &tasks,
        )
        .await
        .unwrap();
        let proxies = crate::proxies::ProxiesClient::spawn(core.clone(), shutdown.clone(), &tasks)
            .await
            .unwrap();
        let mut network = MockNetworkDiagnostics::new();
        network
            .expect_url_delay_test()
            .with(
                mockall::predicate::eq("https://example.test/"),
                mockall::predicate::eq(204),
                mockall::predicate::eq(17321),
            )
            .once()
            .returning(|_, _, _| Some(13));
        let mut dns = MockSystemDnsCache::new();
        dns.expect_flush().once().returning(|| Ok(()));
        struct TestEvents;
        impl crate::client::event_sink::UiEventSink for TestEvents {
            fn state_changed(&self, _: crate::client::event_sink::StateChanged) {}
        }
        let client = ChimeraClient::with_parts(ClientParts {
            reconciler: None,
            ui_sink: Arc::new(TestEvents),
            profile_fs: Arc::new(crate::state::profiles::ports::MockProfileFsPort::new()),
            profile_paths: chimera_profile_io::fs_service::ProfileFileService::new(paths.clone()),
            application: ApplicationClient::legacy().unwrap(),
            session_state: SessionStateClient::legacy().unwrap(),
            clash_config: ClashConfigClient::legacy().unwrap(),
            app_updater: None,
            changelog_http: None,
            network_diagnostics: Arc::new(network),
            icons: crate::service::icon::IconService::new(paths, Arc::new(MockIconFetcher::new())),
            bundle_metadata: None,
            core_api: Some(core.clone()),
            service_actor: None,
            core: Arc::new(Port),
            shutdown: shutdown.clone(),
            tasks: tasks.clone(),
            file_logs: None,
            streams,
            streams_shutdown: streams_shutdown.clone(),
            proxies,
            typed_profiles: None,
            system_dns: Arc::new(dns),
            direct_egress: Arc::new(MockDirectEgressProbe::new()),
        });
        assert_eq!(
            client.url_delay_test("https://example.test/", 204).await,
            Some(13)
        );
        client.flush_system_dns_cache().await.unwrap();
        assert_eq!(
            client.probe_direct_egress().await.unwrap(),
            DirectEgress::Disabled
        );
        assert_eq!(
            client.routed_core_status().unwrap().host,
            core.status().host
        );
        assert!(client.traffic_summary().await.is_err());
        let weak = client.downgrade();
        let clone = weak.upgrade().unwrap();
        drop(client);
        assert!(!streams_shutdown.is_cancelled());
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
        assert!(streams_shutdown.is_cancelled());
        shutdown.cancel();
        tasks.close();
        tokio::time::timeout(std::time::Duration::from_secs(5), tasks.wait())
            .await
            .unwrap();
        core.finish_shutdown().await;
    }
}
