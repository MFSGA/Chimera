//! Instance-owned application facade for runtime, profile, and core operations.
//!
//! This follows REF's client ownership direction while preserving Chimera's
//! staged migration from legacy globals and Chimera-specific core support.

mod app_lifecycle;
mod application;
pub(crate) mod application_workflow;
mod clash_api;
mod clash_config;
mod configuration_status;
pub mod convergence;
mod core_events;
pub(crate) mod core_lifecycle;
pub(crate) mod effects;
pub(crate) mod event_sink;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod hotkey;
pub(crate) mod jobs;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod network_automation;
pub(crate) mod ports;
pub(crate) mod profiles;
pub(crate) mod reconciler;
pub mod runtime;
pub(crate) mod runtime_inspection;
pub(crate) mod runtime_recovery;
mod service_status;
mod session_state;
mod system_dns;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) mod system_proxy;
pub(crate) mod traffic;

use std::sync::Arc;

use anyhow::{Context as _, anyhow};
use chimera_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadataPatch,
    RemoteProfileOptionsPatch, TransformKind,
};
use chimera_contracts::{
    bundle_metadata::BundleMetadata,
    net::{ChangelogHttp, NetworkDiagnostics},
    system_dns::SystemDnsCache,
};
use chimera_core::{
    actor_v2::{CoreClient as CoreClientV2, service_actor::ServiceClient},
    client::app_update::{self, AppUpdateClient},
};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) use self::application::SystemProxyMutation;
#[allow(unused_imports)]
pub(crate) use self::core_lifecycle::LegacyCoreBridge;
pub use self::runtime::{Degradation, DegradationPhase, MutationOutcome};
use self::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    core_lifecycle::CoreLifecyclePort, event_sink::UiEventSink,
    reconciler::RuntimeReconcilerClient, session_state::SessionStateClient,
};
#[allow(unused_imports)]
pub(crate) use self::{
    core_lifecycle::RuntimeTransformDiagnostics,
    event_sink::{LegacyUiEventSink, NoopUiEventSink, TauriUiEventSink},
    ports::SessionPortResolver,
    runtime_inspection::{RuntimeInspection, RuntimeInspectionContent},
};

#[cfg(test)]
use crate::service::profile_file::ProfileFileService;
#[cfg(test)]
use crate::state::profiles::ProfilesError;
use crate::utils::path::PathResolver;

pub use runtime::RuntimePaths;

pub(crate) use chimera_core::client::composition::LegacyBridgeSet;

pub(crate) struct ClientSetupArgs {
    pub(crate) paths: PathResolver,
    pub runtime_paths: RuntimePaths,
    pub(crate) bundle_metadata: BundleMetadata,
    pub(crate) app_update_backend: Arc<dyn app_update::AppUpdateBackend>,
    pub(crate) app_update_events: Arc<dyn app_update::AppUpdateEventSink>,
    pub(crate) host_adapters: chimera_core::client::host_adapters::ClientHostAdapters,
    pub(crate) resolved_paths: chimera_paths::PathResolver,
    pub(crate) self_proxy_port: Arc<dyn chimera_contracts::net::SelfProxyPortSource>,
    pub(crate) bridges: LegacyBridgeSet,
    /// The ref Core/Service Actor handles from the composition root.
    /// Legacy lifecycle ports remain compatibility adapters, not owners.
    pub(crate) core_v2: CoreClientV2,
    pub(crate) service_v2: ServiceClient,
    pub(crate) core: Arc<dyn CoreLifecyclePort>,
    pub(crate) service: Arc<dyn core_lifecycle::ServiceLifecyclePort>,

    pub(crate) system_dns: Arc<dyn SystemDnsCache>,
    pub(crate) traffic_store: Option<Arc<chimera_core::traffic::TrafficFileStorage>>,
    pub(crate) jobs: chimera_jobs::JobsClient,
    pub(crate) shutdown: tokio_util::sync::CancellationToken,
    pub(crate) tasks: tokio_util::task::TaskTracker,
    pub(crate) ui_sink: Arc<dyn UiEventSink>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) effects: Arc<dyn effects::ports::ApplicationEffectsPort>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) system_proxy: system_proxy::SystemProxyClient,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) window: Arc<dyn hotkey::ports::WindowControl>,
}

#[derive(Clone)]
pub(crate) struct ChimeraClient {
    inner: Arc<ChimeraClientInner>,
}

struct TypedConfigClients {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
}

fn utf8_path(path: std::path::PathBuf) -> anyhow::Result<camino::Utf8PathBuf> {
    camino::Utf8PathBuf::from_path_buf(path)
        .map_err(|path| anyhow::anyhow!("config path is not UTF-8: {}", path.display()))
}

struct ChimeraClientInner {
    rpc: std::sync::OnceLock<
        Result<chimera_rpc::registry::UnifiedRpc, chimera_rpc::registry::SetupError>,
    >,
    shared: chimera_core::client::ChimeraClient,
    clash_config: ClashConfigClient,
    /// The one candidate checker shared by the Profiles gate and legacy
    /// Clash read-only preflight during the staged ref source cutover.
    checked_gate: Option<Arc<reconciler::check::CandidateCheck>>,
    ui_sink: Arc<dyn UiEventSink>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    effects: effects::actor::EffectsClient,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    system_proxy: Option<system_proxy::SystemProxyClient>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    hotkey_mutation: tokio::sync::Mutex<()>,
    /// Serializes proxy host changes with every typed and legacy proxy
    /// preference write, including Agent's confirmed stale-proxy repair.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    system_proxy_mutation: tokio::sync::Mutex<()>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    accelerators: Arc<dyn hotkey::ports::AcceleratorValidator>,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    window: Arc<dyn hotkey::ports::WindowControl>,
    #[cfg(test)]
    _test_profile_dir: Option<Arc<tempfile::TempDir>>,
}

impl ChimeraClientInner {
    fn reconciler(&self) -> &Arc<dyn chimera_core::client::reconciler::facade::RuntimeReconciler> {
        self.shared
            .reconciler()
            .expect("composed desktop client has a reconciler")
    }
}

impl ChimeraClient {
    pub(crate) fn rpc(&self) -> anyhow::Result<&chimera_rpc::registry::UnifiedRpc> {
        use chimera_rpc::{
            audit::{RpcAudit, TracingRpcAuditSink},
            registry::{RpcDependencies, UnifiedRpc},
        };
        self.inner
            .rpc
            .get_or_init(|| {
                UnifiedRpc::new(
                    RpcDependencies {
                        client: self.inner.shared.clone(),
                        audit: RpcAudit::new(Arc::new(TracingRpcAuditSink)),
                    },
                    chimera_rpc_protocol::ServerInfo::desktop(env!("CARGO_PKG_VERSION")),
                    &[],
                )
            })
            .as_ref()
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }

    pub(crate) async fn call_shared_rpc<T: serde::de::DeserializeOwned>(
        &self,
        window: &str,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<T> {
        use chimera_rpc::{audit::RpcTransport, registry::RpcCaller};
        let value = self
            .rpc()?
            .call(
                RpcTransport::TauriIpc,
                method,
                RpcCaller::desktop(window),
                params,
            )
            .await
            .map_err(|error| anyhow::anyhow!(error.message))?;
        Ok(serde_json::from_value(value)?)
    }

    pub(crate) async fn retry_configuration_runtime_rpc(&self, window: &str) -> anyhow::Result<()> {
        self.call_shared_rpc(window, "retry_configuration_runtime", serde_json::json!({}))
            .await
    }
}

impl std::ops::Deref for ChimeraClient {
    type Target = chimera_core::client::ChimeraClient;
    fn deref(&self) -> &Self::Target {
        &self.inner.shared
    }
}

#[cfg(test)]
struct NoSelfProxyPort;

#[cfg(test)]
impl chimera_contracts::net::SelfProxyPortSource for NoSelfProxyPort {
    fn mixed_port(&self) -> Option<u16> {
        None
    }
}

impl ChimeraClient {
    pub(crate) fn try_new_with_args(args: ClientSetupArgs) -> anyhow::Result<Self> {
        let ClientSetupArgs {
            paths,
            runtime_paths,
            bundle_metadata,
            app_update_backend,
            app_update_events,
            host_adapters,
            resolved_paths,
            self_proxy_port,
            bridges,
            core_v2,
            service_v2,
            core,
            service,
            system_dns,
            traffic_store,
            jobs,
            shutdown,
            tasks,
            ui_sink,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
                effects: effects_port,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            system_proxy,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            accelerators,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            window,
        } = args;
        let mutations = chimera_core::state::mutation::MutationCoordinator::pending();
        let typed = tauri::async_runtime::block_on(
            chimera_core::client::composition::new_typed_config_clients(
                &resolved_paths,
                &bridges,
                bundle_metadata.release_channel,
                mutations.clone(),
                shutdown.clone(),
            ),
        )?;
        let typed = TypedConfigClients {
            application: typed.application,
            session_state: typed.session_state,
            clash_config: ClashConfigClient::from_shared(typed.clash_config),
        };
        let file_ports = (host_adapters.profile_files)(self_proxy_port.clone());
        let observers = tauri::async_runtime::block_on(
            chimera_core::client::composition::spawn_runtime_observers(
                paths.app_logs_dir().join("core"),
                &typed.application.get_typed(),
                &typed.clash_config.get()?,
                core_v2.clone(),
                shutdown.clone(),
                &tasks,
                #[cfg(not(any(target_os = "android", target_os = "ios")))]
                effects_port,
            ),
        )?;
        let streams_shutdown = observers.streams_shutdown;
        let streams = observers.streams;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = observers.effects;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let commit_notifications: Arc<dyn effects::ports::CommitNotifications> = Arc::new((
            effects.clone(),
            event_sink::UiCommitNotifications(ui_sink.clone()),
        ));
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let commit_notifications: Arc<dyn effects::ports::CommitNotifications> =
            Arc::new(event_sink::UiCommitNotifications(ui_sink.clone()));
        let typed_profiles =
            tauri::async_runtime::block_on(profiles::ProfilesClient::new_with_jobs(
                Arc::new(mutations.clone()),
                utf8_path(paths.profiles_path())?,
                jobs,
                file_ports.fs.clone(),
                file_ports.fetcher,
                file_ports.materialization,
                shutdown.clone(),
                &tasks,
            ))?;
        let profile_content_source = Arc::new(crate::enhance::FsProfileContentSource::new(
            paths.app_profiles_dir(),
        ));
        let core_lifecycle = tauri::async_runtime::block_on(
            RuntimeReconcilerClient::spawn_with_profiles_and_content(
                core.clone(),
                typed.application.clone(),
                typed.clash_config.clone(),
                Some(typed_profiles.snapshot_handle()),
                Some(typed_profiles.content_reader()),
                Some(profile_content_source.clone()),
                runtime_paths.clone(),
                Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
                    crate::adapters::elevation::InteractiveElevationConsent,
                ))),
                service,
                shutdown.clone(),
                &tasks,
            ),
        )?;
        let profile_gate = Arc::new(reconciler::composition::prepare_production_gate(
            &typed.application,
            &typed.clash_config,
            typed_profiles.content_reader(),
            profile_content_source,
            runtime_paths,
            &core_v2,
            &core_lifecycle,
            host_adapters.core_specs,
            host_adapters.scripts,
        )?);
        // Both Application and Clash legacy postcommit paths validate via
        // the exact Profiles production checker. Install it in the sole
        // CoreLifecycle workflow before opening Profile mutation admission.
        tauri::async_runtime::block_on(
            core_lifecycle.install_candidate_gate(profile_gate.clone()),
        )?;
        mutations.connect(commit_notifications, profile_gate.clone());
        let owners = tauri::async_runtime::block_on(
            chimera_core::client::composition::spawn_application_observers(
                core_v2.clone(),
                &typed.application.get_typed(),
                bundle_metadata.clone(),
                app_update_backend,
                app_update_events,
                paths.app_logs_dir(),
                shutdown.clone(),
                &tasks,
            ),
        )?;
        let proxies = owners.proxies;
        let app_updater = owners.app_updater;
        let file_logs = owners.file_logs;
        let changelog_http = (host_adapters.changelog_http)(self_proxy_port);
        let icons = chimera_core::service::icon::IconService::new(
            resolved_paths,
            host_adapters.icon_fetcher,
        );
        let client = Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
            core,
            Some(core_v2),
            Some(service_v2),
            file_logs,
            streams,
            streams_shutdown,
            proxies,
            file_ports.fs,
            chimera_profile_io::fs_service::ProfileFileService::from_profiles_dir(
                paths.app_profiles_dir(),
            ),
            Some(typed_profiles),
            Some(profile_gate),
            system_dns,
            ui_sink,
            Some(app_updater),
            Some(changelog_http),
            host_adapters.network_diagnostics,
            icons,
            Some(bundle_metadata),
            shutdown.clone(),
            tasks.clone(),
            #[cfg(test)]
            None,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Some(system_proxy),
            accelerators,
            window,
        );
        tauri::async_runtime::block_on(client.inner.shared.initialize_traffic(
            traffic_store,
            paths.app_data_dir().to_path_buf(),
            paths.cache_dir(),
        ))?;
        // ref's CoreActor publishes host/instance changes independently of
        // application configuration commits. Subscribe once from the Tauri
        // reactor after the complete Client exists; a sync setup-thread spawn
        // here would panic without an entered Tokio runtime.
        anyhow::ensure!(
            tauri::async_runtime::block_on(async { client.start_core_status_observer() }),
            "client shutdown began before its CoreActor observer was registered"
        );
        anyhow::ensure!(
            tauri::async_runtime::block_on(async { client.start_configuration_status_observer() }),
            "client shutdown began before its configuration observer was registered"
        );
        Ok(client)
    }

    #[cfg(test)]
    async fn with_parts(
        core: Arc<dyn CoreLifecyclePort>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
    ) -> Self {
        let typed = TypedConfigClients {
            application: ApplicationClient::legacy()
                .expect("test application client should initialize"),
            session_state: SessionStateClient::legacy()
                .expect("test session state client should initialize"),
            clash_config: ClashConfigClient::legacy()
                .expect("test clash config client should initialize"),
        };
        let test_profile_dir = Arc::new(tempfile::tempdir().expect("test profile directory"));
        let paths = PathResolver::with_base_dirs(
            test_profile_dir.path().join("config"),
            test_profile_dir.path().join("data"),
        );
        let profile_service = Arc::new(ProfileFileService::new(
            paths.clone(),
            Arc::new(NoSelfProxyPort),
        ));
        let runtime_paths = RuntimePaths::from_config_root(paths.app_config_dir().to_path_buf());
        let core_lifecycle = RuntimeReconcilerClient::direct(
            core.clone(),
            typed.application.clone(),
            typed.clash_config.clone(),
            runtime_paths,
        );
        let proxy_shutdown = tokio_util::sync::CancellationToken::new();
        let proxy_tasks = tokio_util::task::TaskTracker::new();
        let proxies = chimera_core::proxies::ProxiesClient::spawn_with_source(
            clash_api::proxy_core_client(core.clone()),
            proxy_shutdown,
            &proxy_tasks,
        )
        .await
        .expect("test proxy actor should start");

        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = {
            let initial = effects::plan::ApplicationEffectInputs::project(
                &typed.application.get_typed(),
                &typed
                    .clash_config
                    .get()
                    .expect("test clash config should be readable"),
                None,
            );
            effects::actor::EffectsClient::spawn(effects::actor::EffectsArgs {
                port: Arc::new(effects::ports::NoopApplicationEffects),
                initial,
            })
            .await
            .expect("test effects actor should start")
        };

        let streams_shutdown = tokio_util::sync::CancellationToken::new();
        let streams_tasks = tokio_util::task::TaskTracker::new();
        let streams = chimera_core::clash::ws::StreamsClient::spawn(
            chimera_core::actor_v2::CoreClient::spawn(
                chimera_core::actor_v2::test_support::endpoint("http://127.0.0.1:1/".into()),
            )
            .await
            .unwrap(),
            chimera_core::logs::CoreLogsClient::spawn(
                Box::new(chimera_core::logs::UnavailableCoreLogStore("test".into())),
                Default::default(),
                streams_shutdown.clone(),
                &streams_tasks,
            )
            .await
            .unwrap(),
            chimera_config::clash::config::overrides::LogLevel::Info,
            streams_shutdown.clone(),
            &streams_tasks,
        )
        .await
        .expect("test streams actor should start");

        Self::with_parts_and_typed_config(
            typed,
            core_lifecycle,
            core,
            None,
            None,
            None,
            streams,
            streams_shutdown,
            proxies,
            profile_service,
            chimera_profile_io::fs_service::ProfileFileService::from_profiles_dir(
                paths.app_profiles_dir(),
            ),
            None,
            None,
            system_dns,
            ui_sink,
            None,
            None,
            Arc::new(chimera_contracts::net::MockNetworkDiagnostics::new()),
            chimera_core::service::icon::IconService::new(
                chimera_paths::PathResolver::with_base_dirs(
                    utf8_path(paths.app_config_dir().to_path_buf()).unwrap(),
                    utf8_path(paths.app_data_dir().to_path_buf()).unwrap(),
                ),
                Arc::new(chimera_contracts::net::MockIconFetcher::new()),
            ),
            None,
            tokio_util::sync::CancellationToken::new(),
            tokio_util::task::TaskTracker::new(),
            Some(test_profile_dir),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            None,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Arc::new(hotkey::adapters::PlatformAcceleratorValidator),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            Arc::new(hotkey::ports::MockWindowControl::new()),
        )
    }

    fn with_parts_and_typed_config(
        typed: TypedConfigClients,
        core_lifecycle: RuntimeReconcilerClient,
        core: Arc<dyn CoreLifecyclePort>,
        core_api: Option<CoreClientV2>,
        service_actor: Option<ServiceClient>,
        file_logs: Option<chimera_core::client::logs::FileLogsClient>,
        streams: chimera_core::clash::ws::StreamsClient,
        streams_shutdown: tokio_util::sync::CancellationToken,
        proxies: chimera_core::proxies::ProxiesClient,
        profile_fs: Arc<dyn chimera_core::state::profiles::ports::ProfileFsPort>,
        profile_paths: chimera_profile_io::fs_service::ProfileFileService,
        typed_profiles: Option<profiles::ProfilesClient>,
        checked_gate: Option<Arc<reconciler::check::CandidateCheck>>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
        app_updater: Option<AppUpdateClient>,
        changelog_http: Option<Arc<dyn ChangelogHttp>>,
        network_diagnostics: Arc<dyn NetworkDiagnostics>,
        icons: chimera_core::service::icon::IconService,
        bundle_metadata: Option<BundleMetadata>,
        shutdown: tokio_util::sync::CancellationToken,
        tasks: tokio_util::task::TaskTracker,
        #[cfg(test)] _test_profile_dir: Option<Arc<tempfile::TempDir>>,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        effects: effects::actor::EffectsClient,
        #[cfg(not(any(target_os = "android", target_os = "ios")))] system_proxy: Option<
            system_proxy::SystemProxyClient,
        >,
        #[cfg(not(any(target_os = "android", target_os = "ios")))] accelerators: Arc<
            dyn hotkey::ports::AcceleratorValidator,
        >,
        #[cfg(not(any(target_os = "android", target_os = "ios")))] window: Arc<
            dyn hotkey::ports::WindowControl,
        >,
    ) -> Self {
        let shared =
            chimera_core::client::ChimeraClient::with_parts(chimera_core::client::ClientParts {
                reconciler: Some(core_lifecycle.shared_handle()),
                ui_sink: Arc::new(event_sink::SharedUiEventSink(ui_sink.clone())),
                profile_fs,
                profile_paths,
                application: typed.application,
                session_state: typed.session_state,
                clash_config: (*typed.clash_config).clone(),
                direct_egress: Arc::new(
                    chimera_net::direct_egress::NetworkDirectEgressProbe::default(),
                ),
                app_updater,
                changelog_http,
                network_diagnostics,
                icons,
                bundle_metadata,
                core_api,
                service_actor,
                core,
                shutdown,
                tasks,
                file_logs,
                streams,
                streams_shutdown,
                proxies,
                typed_profiles,
                system_dns,
            });
        let inner = ChimeraClientInner {
            rpc: std::sync::OnceLock::new(),
            shared,
            clash_config: typed.clash_config,
            checked_gate,
            ui_sink,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            system_proxy,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            hotkey_mutation: tokio::sync::Mutex::new(()),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            system_proxy_mutation: tokio::sync::Mutex::new(()),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            accelerators,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            window,
            #[cfg(test)]
            _test_profile_dir,
        };
        Self {
            inner: Arc::new(inner),
        }
    }
}

impl ChimeraClient {
    pub(crate) async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        Ok(self
            .inner
            .shared
            .add_profile(request, initial_file)
            .await?
            .into())
    }

    pub(crate) async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        Ok(self
            .inner
            .shared
            .create_profile(request, initial_file)
            .await?
            .into())
    }

    /// Download and suggest an import kind without committing a source document.
    // The desktop IPC is introduced with the generated frontend contract.
    #[allow(dead_code)]
    pub(crate) async fn probe_profile_url(
        &self,
        url: url::Url,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<chimera_core::profile_import::ProfileUrlProbe> {
        Ok(self.inner.shared.probe_profile_url(url, options).await?)
    }

    pub(crate) async fn import_profile(
        &self,
        url: url::Url,
        name: Option<String>,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        self.import_profile_to_folder(url, name, patch, None).await
    }

    pub(crate) async fn import_profile_to_folder(
        &self,
        url: url::Url,
        name: Option<String>,
        patch: Option<RemoteProfileOptionsPatch>,
        folder: Option<chimera_config::profile::FolderId>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        self.import_profile_with_transform(url, name, patch, None, folder)
            .await
    }

    /// Ref import semantics: only Config imports may auto-activate.
    pub(crate) async fn import_profile_with_transform(
        &self,
        url: url::Url,
        name: Option<String>,
        patch: Option<RemoteProfileOptionsPatch>,
        transform: Option<TransformKind>,
        folder: Option<FolderId>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        Ok(self
            .inner
            .shared
            .import_profile_with_transform(url, name, patch, transform, folder)
            .await?
            .into())
    }

    pub(crate) async fn delete_profile(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.delete_profile(uid).await?.into())
    }

    pub(crate) async fn reorder_profile(
        &self,
        active: ProfileId,
        over: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .reorder_profile(active, over)
            .await?
            .into())
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .reorder_profiles_by_list(list)
            .await?
            .into())
    }

    /// Atomically persist the Profile folders and Config display order.
    pub(crate) async fn set_profile_layout(
        &self,
        layout: ProfileLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.set_profile_layout(layout).await?.into())
    }

    /// Create a folder and move Config members into it without a second write.
    pub(crate) async fn create_profile_folder(
        &self,
        name: String,
        members: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        Ok(self
            .inner
            .shared
            .create_profile_folder(name, members)
            .await?
            .into())
    }

    /// Presentation-only proxy group ordering for one Config Profile.
    pub(crate) async fn set_proxy_group_layout(
        &self,
        profile: ProfileId,
        layout: chimera_config::profile::ProxyGroupLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .set_proxy_group_layout(profile, layout)
            .await?
            .into())
    }

    pub(crate) async fn create_proxy_group_folder(
        &self,
        profile: ProfileId,
        name: String,
        members: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        Ok(self
            .inner
            .shared
            .create_proxy_group_folder(profile, name, members)
            .await?
            .into())
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.refresh_profile(uid, patch).await?.into())
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .patch_profile_metadata(uid, patch)
            .await?
            .into())
    }

    pub(crate) async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .patch_remote_profile_options(uid, patch)
            .await?
            .into())
    }

    pub(crate) async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .replace_profile_definition(uid, definition)
            .await?
            .into())
    }

    pub(crate) async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.activate_profile(uid).await?.into())
    }

    pub(crate) async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.set_global_transforms(ids).await?.into())
    }

    pub(crate) async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self
            .inner
            .shared
            .set_profile_valid_fields(fields)
            .await?
            .into())
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        Ok(self.inner.shared.save_profile_file(uid, data).await?.into())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    use struct_patch::Patch as _;

    use async_trait::async_trait;
    use chimera_ipc::api::status::CoreState;

    use super::core_lifecycle::CoreStatusSnapshot;
    use super::*;
    use crate::client::system_dns::NoopSystemDnsCache;
    use crate::{config::chimera::ClashCore, core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        fail_rebuild: bool,
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
            crate::config::core::Config::clash()
                .latest()
                .get_client_info()
        }

        async fn reconcile(
            &self,
            _clash: chimera_config::clash::config::ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            if self.fail_rebuild {
                anyhow::bail!("injected rebuild failure");
            }
            Ok(())
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop");
            Ok(())
        }

        async fn change_core(&self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("change-core");
            Ok(())
        }
        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 7,
                run_type: RunType::Normal,
            })
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _break_when: bool) {
            self.events.lock().unwrap().push("profile-change");
        }
    }

    struct RecordingUi {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingSystemDns {
        flushes: Arc<Mutex<usize>>,
        fail: bool,
    }

    impl SystemDnsCache for RecordingSystemDns {
        fn flush(&self) -> Result<(), chimera_contracts::system_dns::SystemDnsError> {
            *self.flushes.lock().unwrap() += 1;
            if self.fail {
                return Err(
                    chimera_contracts::system_dns::SystemDnsError::FlushRejected {
                        command: "injected DNS cache flush failure",
                        code: Some(1),
                    },
                );
            }
            Ok(())
        }
    }

    impl UiEventSink for RecordingUi {
        fn core_status_changed(&self, _: chimera_core::actor_v2::router::CoreStatusInfo) {
            self.events.lock().unwrap().push("core-status-changed");
        }
        fn refresh_clash(&self) {
            self.events.lock().unwrap().push("refresh-ui");
        }
        fn refresh_runtime_transform_diagnostics(&self) {
            self.events.lock().unwrap().push("refresh-diagnostics");
        }
        fn refresh_profiles(&self) {
            self.events.lock().unwrap().push("refresh-profiles");
        }
    }

    pub(super) async fn recording_client(
        fail_rebuild: bool,
    ) -> (ChimeraClient, Arc<Mutex<Vec<&'static str>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild,
            }),
            Arc::new(NoopSystemDnsCache),
            Arc::new(RecordingUi {
                events: events.clone(),
            }),
        )
        .await;
        (client, events)
    }

    #[tokio::test]
    async fn shared_rpc_inventory_admits_only_linked_and_permitted_commands() {
        use chimera_contracts::auth::{Grant, Principal, Role};
        use chimera_rpc::{audit::RpcTransport, registry::RpcCaller};
        use chimera_rpc_protocol::{AuthContext, RpcDescribeResponse, RpcOwner, RpcTarget};
        let (client, events) = recording_client(false).await;
        let rpc = client.rpc().unwrap();
        let owner = RpcCaller::desktop("test-window");
        let viewer = RpcCaller::new(
            AuthContext::new(
                Principal::Token {
                    id: "test-id".into(),
                    name: "viewer".into(),
                },
                Grant::Role(Role::Viewer),
            ),
            RpcOwner::new("test-owner".into()),
        );
        let described: RpcDescribeResponse = serde_json::from_value(
            rpc.call(
                RpcTransport::TauriIpc,
                "rpc_describe",
                owner.clone(),
                serde_json::json!({}),
            )
            .await
            .unwrap(),
        )
        .unwrap();
        assert_eq!(described, rpc.describe(RpcTarget::Desktop, owner.auth()));
        assert!(
            described
                .commands
                .iter()
                .any(|c| c.name == "get_config_features" && c.allowed)
        );
        assert!(
            described
                .commands
                .iter()
                .any(|c| c.name == "retry_configuration_runtime" && c.allowed)
        );
        assert!(
            described
                .commands
                .iter()
                .any(|c| c.name == "get_runtime_config" && !c.allowed)
        );
        let expected = serde_json::to_value(client.config_features()).unwrap();
        assert_eq!(
            rpc.call(
                RpcTransport::Http,
                "get_config_features",
                viewer.clone(),
                serde_json::json!({})
            )
            .await
            .unwrap(),
            expected
        );
        let before = events.lock().unwrap().clone();
        let denied = rpc
            .call(
                RpcTransport::Http,
                "retry_configuration_runtime",
                viewer.clone(),
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(denied.kind, "forbidden");
        assert_eq!(*events.lock().unwrap(), before);
        let unlinked = rpc
            .call(
                RpcTransport::TauriIpc,
                "get_runtime_config",
                owner.clone(),
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(unlinked.kind, "unsupported");
        let unknown = rpc
            .call(
                RpcTransport::Http,
                "private-sentinel",
                viewer.clone(),
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(unknown.kind, "unknown_method");
        assert!(!unknown.message.contains("private-sentinel"));
        let wrong_transport = rpc
            .call(
                RpcTransport::Local,
                "get_config_features",
                owner.clone(),
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(wrong_transport.kind, "unsupported");
        client.request_shutdown();
        let error = client
            .retry_configuration_runtime_rpc("test-window")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("client is shutting down"));
        let public_failure = rpc
            .call(
                RpcTransport::Http,
                "get_core_status_summary",
                viewer,
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(public_failure.message, "application command failed");
    }

    /// Initial state: the test client owns an uncancelled root token and an
    /// empty tracker. Operation: start tracked work that waits on a child of
    /// that token, then cancel the root and close the tracker. Expected result:
    /// the work signals completion before `wait` returns. The oneshot is an
    /// independent observation; this fails if the task escapes the tracker or
    /// listens to a different shutdown token.
    #[tokio::test]
    async fn tracked_background_work_drains_after_root_shutdown() {
        let (client, _) = recording_client(false).await;
        let shutdown = client.shutdown_child_token();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (finished_tx, finished_rx) = tokio::sync::oneshot::channel();
        assert!(client.clone().spawn_tracked(async move {
            started_tx.send(()).unwrap();
            shutdown.cancelled().await;
            finished_tx.send(()).unwrap();
        }));

        started_rx.await.unwrap();
        client.inner.shared.shutdown().cancel();
        client.inner.shared.tasks().close();
        client.inner.shared.tasks().wait().await;
        finished_rx.await.unwrap();
    }

    #[tokio::test]
    async fn system_proxy_mutation_guard_serializes_competing_transactions() {
        let (client, _) = recording_client(false).await;
        let guard = client.lock_system_proxy_mutation().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let waiter = {
            let client = client.clone();
            let acquired = acquired.clone();
            tokio::spawn(async move {
                let _guard = client.lock_system_proxy_mutation().await;
                acquired.store(true, Ordering::Release);
            })
        };

        tokio::task::yield_now().await;
        assert!(!acquired.load(Ordering::Acquire));
        drop(guard);
        waiter.await.unwrap();
        assert!(acquired.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn flush_system_dns_cache_forwards_to_injected_adapter() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let flushes = Arc::new(Mutex::new(0));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(RecordingSystemDns {
                flushes: flushes.clone(),
                fail: false,
            }),
            Arc::new(RecordingUi { events }),
        )
        .await;

        client.flush_system_dns_cache().await.unwrap();
        assert_eq!(*flushes.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn flush_system_dns_cache_propagates_adapter_failure() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = ChimeraClient::with_parts(
            Arc::new(RecordingCore {
                events: events.clone(),
                fail_rebuild: false,
            }),
            Arc::new(RecordingSystemDns {
                flushes: Arc::new(Mutex::new(0)),
                fail: true,
            }),
            Arc::new(RecordingUi { events }),
        )
        .await;

        let error = client.flush_system_dns_cache().await.unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected DNS cache flush failure")
        );
    }

    #[tokio::test]
    async fn core_status_is_read_through_the_injected_lifecycle_port() {
        let (client, _) = recording_client(false).await;
        let snapshot = client.core_status().await.unwrap();
        assert!(matches!(snapshot.state, CoreState::Stopped(None)));
        assert_eq!(snapshot.state_changed_at, 7);
        assert_eq!(snapshot.run_type, RunType::Normal);
    }

    /// The production Client owns the same ref CoreClient as its compatibility
    /// facade. Keep test-only injected legacy ports possible without creating
    /// a second router, and verify the typed projection is not synthesized
    /// from the legacy CoreLifecyclePort's state.
    struct StoppedRoutedHost;

    #[async_trait]
    impl chimera_core::actor_v2::endpoint::ControlEndpoint for StoppedRoutedHost {
        fn host(&self) -> chimera_core::actor_v2::endpoint::ExecutionHost {
            chimera_core::actor_v2::endpoint::ExecutionHost::Local
        }

        async fn submit(
            &self,
            submission: chimera_core::actor_v2::endpoint::CoreSubmission,
        ) -> Result<chimera_ipc::api::core::v2::OperationInfo, chimera_core_manager::CoreError>
        {
            assert!(
                matches!(
                    submission.envelope.command,
                    chimera_core_manager::CoreCommand::Stop
                ),
                "the stopped test host only accepts idempotent Stop"
            );
            Ok(chimera_ipc::api::core::v2::OperationInfo {
                id: submission.envelope.operation_id.to_string(),
                phase: chimera_ipc::api::core::v2::OperationPhase::Running,
                output: None,
                error: None,
            })
        }

        async fn wait_operation(
            &self,
            id: chimera_core_manager::OperationId,
            _: std::time::Duration,
        ) -> Option<chimera_ipc::api::core::v2::OperationInfo> {
            Some(chimera_ipc::api::core::v2::OperationInfo {
                id: id.to_string(),
                phase: chimera_ipc::api::core::v2::OperationPhase::Succeeded,
                output: Some(chimera_ipc::api::core::v2::OperationOutputInfo::Stopped),
                error: None,
            })
        }

        async fn status(
            &self,
        ) -> Result<
            chimera_core::actor_v2::endpoint::CoreStatusSnapshot,
            chimera_core_manager::CoreError,
        > {
            Ok(chimera_core::actor_v2::endpoint::CoreStatusSnapshot {
                instance_id: None,
                controller: None,
                state: Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }),
                state_changed_at: 17,
                revision: None,
                source_hash: None,
                healthy: Some(true),
                applied_kind: None,
            })
        }
    }

    #[tokio::test]
    async fn direct_ref_core_handle_has_independent_routing_projection() {
        let (mut client, _) = recording_client(false).await;
        assert!(client.routed_core_status().is_none());

        let core_api = CoreClientV2::spawn(Arc::new(StoppedRoutedHost))
            .await
            .unwrap();
        Arc::get_mut(&mut client.inner)
            .expect("the fixture has one Client owner")
            .shared
            .test_set_core_api(Some(core_api.clone()));

        let routed = client
            .routed_core_status()
            .expect("the typed Client owns its Core Actor");
        assert_eq!(
            routed.host,
            chimera_core::actor_v2::endpoint::ExecutionHost::Local
        );
        assert_eq!(routed.generation, 0);
        // The legacy frontend's two-state adapter stays separate until it
        // can represent all ref CoreActor transition and degraded states.
        assert_eq!(client.core_status().await.unwrap().state_changed_at, 7);
        core_api.finish_shutdown().await;
    }

    /// A production Client must not read the old CoreLifecycle watch for UI
    /// Service status after its ref ServiceActor has observed the daemon.
    struct OfflineProbeService;

    #[async_trait]
    impl chimera_contracts::service::ServiceProcessControl for OfflineProbeService {
        async fn probe(&self) -> Result<String, chimera_contracts::service::ServiceCommandError> {
            Ok(serde_json::to_string(&chimera_ipc::types::StatusInfo {
                name: "chimera-service".into(),
                version: "1.10.1".into(),
                status: chimera_ipc::types::ServiceStatus::NotInstalled,
                server: None,
            })
            .unwrap())
        }

        async fn install(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not install a daemon")
        }
        async fn update(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not update a daemon")
        }
        async fn uninstall(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not uninstall a daemon")
        }
        async fn start_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not start a daemon")
        }
        async fn stop_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not stop a daemon")
        }
        async fn restart_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            panic!("a read-only Service query must not restart a daemon")
        }
    }

    #[tokio::test]
    async fn production_service_status_uses_ref_actor_not_legacy_workflow_watch() {
        use chimera_core::actor_v2::{
            endpoint::EndpointHandle, service_endpoint_factory::IpcServiceEndpointFactory,
            service_policy::ServicePhase,
        };

        let (mut client, _) = recording_client(false).await;
        assert_eq!(
            client.inner.reconciler().service_status().phase,
            ServicePhase::Probing
        );
        let actor = ServiceClient::spawn(
            Arc::new(OfflineProbeService),
            Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
                panic!("uninstalled daemon must not open a Core endpoint")
            })),
            3,
        )
        .await
        .expect("spawn ServiceActor");
        assert_eq!(
            actor.probe().await.expect("probe ServiceActor").phase,
            chimera_core::actor_v2::service_router::ServicePhase::NotInstalled
        );
        Arc::get_mut(&mut client.inner)
            .expect("test fixture has one owner")
            .shared
            .test_set_service_actor(Some(actor.clone()));

        assert_eq!(client.service_status().phase, ServicePhase::NotInstalled);
        assert_eq!(
            client.inner.reconciler().service_status().phase,
            ServicePhase::Probing,
            "the legacy workflow watch must not be consulted by UI status"
        );

        let tasks = tokio_util::task::TaskTracker::new();
        let shutdown = tokio_util::sync::CancellationToken::new();
        actor.drain_on_shutdown(&tasks, shutdown.clone());
        shutdown.cancel();
        tasks.close();
        tokio::time::timeout(std::time::Duration::from_secs(5), tasks.wait())
            .await
            .expect("ServiceActor drains after status test");
    }

    /// The production facade must provide Core/Service observations from the
    /// same typed actors and reject a stopped Core before acquiring an API
    /// lease; an injected legacy Core status must not substitute for either.
    #[tokio::test]
    async fn production_ref_facade_fences_stopped_core_api_and_projects_both_actors() {
        use chimera_core::actor_v2::{
            endpoint::{EndpointHandle, ExecutionHost},
            service_endpoint_factory::IpcServiceEndpointFactory,
            service_router::ServicePhase,
        };

        let (mut client, _) = recording_client(false).await;
        let core = CoreClientV2::spawn(Arc::new(StoppedRoutedHost))
            .await
            .expect("spawn CoreActor");
        let service = ServiceClient::spawn(
            Arc::new(OfflineProbeService),
            Arc::new(IpcServiceEndpointFactory::new(|| -> EndpointHandle {
                panic!("an uninstalled daemon cannot provide an endpoint")
            })),
            3,
        )
        .await
        .expect("spawn ServiceActor");
        assert_eq!(
            service.probe().await.expect("probe ServiceActor").phase,
            ServicePhase::NotInstalled
        );
        let inner = Arc::get_mut(&mut client.inner).expect("one test Client owner");
        inner
            .shared
            .test_set_ref_facade(Some(chimera_core::actor_v2::facade::CoreFacade::new(
                core.clone(),
                service.clone(),
            )));

        assert_eq!(
            client
                .routed_core_status()
                .expect("ref facade provides core projection")
                .host,
            ExecutionHost::Local
        );
        assert_eq!(
            client
                .service_actor_status()
                .expect("ref service projection")
                .phase,
            ServicePhase::NotInstalled
        );
        let observed = client.refreshed_routed_core_status().await.unwrap();
        assert!(matches!(
            observed.snapshot.and_then(|s| s.state),
            Some(chimera_ipc::api::status::CoreStateDetail::Stopped { .. })
        ));
        let error = client
            .clash_api_client()
            .await
            .expect_err("the ref facade must not lease a stopped controller");
        assert!(error.to_string().contains("core is stopped"));

        core.finish_shutdown().await;
        let tasks = tokio_util::task::TaskTracker::new();
        let shutdown = tokio_util::sync::CancellationToken::new();
        service.drain_on_shutdown(&tasks, shutdown.clone());
        shutdown.cancel();
        tasks.close();
        tokio::time::timeout(std::time::Duration::from_secs(5), tasks.wait())
            .await
            .expect("ServiceActor must drain");
    }

    #[tokio::test]
    async fn running_clash_config_requires_a_live_ref_core_api_capability() {
        use crate::client::core_lifecycle::{ActorRunningConfigBridge, ports::RunningConfigPort};

        // The old Facade could return a cached HTTP controller after the
        // owning Core had stopped. The ref bridge must instead acquire a
        // fresh instance-bound lease from CoreActor for every read/write.
        let core = CoreClientV2::spawn(Arc::new(StoppedRoutedHost))
            .await
            .unwrap();
        let running_config = ActorRunningConfigBridge::new(core.clone());
        assert!(
            RunningConfigPort::read(&running_config).await.is_err(),
            "a stopped Core without an API binding cannot serve live config"
        );
        assert!(
            RunningConfigPort::patch(&running_config, &serde_yaml::Mapping::new())
                .await
                .is_err(),
            "live config writes must not bypass CoreActor's API lease"
        );

        core.finish_shutdown().await;
        assert!(
            RunningConfigPort::read(&running_config).await.is_err(),
            "a shut-down CoreActor must never fall back to a Facade controller"
        );
        assert!(
            RunningConfigPort::patch(&running_config, &serde_yaml::Mapping::new())
                .await
                .is_err(),
            "live config writes must fail closed after CoreActor shutdown"
        );
    }

    #[test]
    fn synchronous_tauri_setup_can_register_ref_core_observer() {
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "simulate Tauri's synchronous setup callback, not a Tokio task"
        );
        let (mut client, _) = tauri::async_runtime::block_on(recording_client(false));
        let core = tauri::async_runtime::block_on(CoreClientV2::spawn(Arc::new(StoppedRoutedHost)))
            .unwrap();
        Arc::get_mut(&mut client.inner)
            .expect("the fixture has one Client owner")
            .shared
            .test_set_core_api(Some(core.clone()));

        // A bare TaskTracker::spawn here would panic without a reactor. The
        // composition root must enter Tauri's runtime for registration.
        assert!(tauri::async_runtime::block_on(async {
            client.start_core_status_observer()
        }));

        client.request_shutdown();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.inner.shared.tasks().wait(),
            )
            .await
            .expect("CoreActor observer must drain from synchronous setup");
            core.finish_shutdown().await;
        });
    }

    #[tokio::test]
    async fn ref_core_status_observer_notifies_ui_and_drains_on_shutdown() {
        let (mut client, events) = recording_client(false).await;
        let core = CoreClientV2::spawn(Arc::new(StoppedRoutedHost))
            .await
            .unwrap();
        Arc::get_mut(&mut client.inner)
            .expect("the fixture has one Client owner")
            .shared
            .test_set_core_api(Some(core.clone()));

        assert!(client.start_core_status_observer());
        // The ref router publishes an observable terminal transition. The
        // observer must detect it without an independent legacy Core poll.
        core.shutdown().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let observed = events.lock().unwrap();
                if observed.contains(&"refresh-ui") && observed.contains(&"core-status-changed") {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the ref projection must notify the UI");

        client.request_shutdown();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.inner.shared.tasks().wait(),
        )
        .await
        .expect("the tracked observer must exit after root cancellation");
        core.finish_shutdown().await;
    }

    /// These checks must not depend on the process-global legacy application
    /// config used by unrelated concurrent tests. The source owner and CAS
    /// version are real; only the lower Core port is injected.
    struct CoreSelectionMirror;

    impl crate::state::mirror::VergeLegacyBridge for CoreSelectionMirror {
        fn prepare(
            &self,
            _next: &chimera_config::application::ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
            Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::application::ChimeraAppConfig> {
            Ok(chimera_config::application::ChimeraAppConfig::default())
        }
    }

    async fn isolated_core_selection_client(
        fail_rebuild: bool,
    ) -> (ChimeraClient, Arc<Mutex<Vec<&'static str>>>) {
        let (mut client, events) = recording_client(fail_rebuild).await;
        let root = client
            .inner
            ._test_profile_dir
            .as_ref()
            .expect("test client retains a temporary root")
            .path()
            .join("core-selection-application.yaml");
        let application = ApplicationClient::new(
            camino::Utf8PathBuf::from_path_buf(root).unwrap(),
            chimera_config::application::ChimeraAppConfig::default(),
            Arc::new(CoreSelectionMirror),
            chimera_config::application::ReleaseChannel::Stable,
        )
        .await
        .unwrap();
        let inner = Arc::get_mut(&mut client.inner).expect("unique test client");
        inner.shared.test_set_application(application.clone());
        let reconciler = RuntimeReconcilerClient::direct(
            inner.shared.core().clone(),
            application,
            inner.clash_config.clone(),
            RuntimePaths::from_config_root(
                inner
                    ._test_profile_dir
                    .as_ref()
                    .unwrap()
                    .path()
                    .to_path_buf(),
            ),
        );
        inner.shared.test_set_reconciler(reconciler.shared_handle());
        (client, events)
    }

    #[tokio::test]
    async fn change_core_is_a_committed_application_patch_not_a_direct_core_mutation() {
        let (client, events) = isolated_core_selection_client(false).await;
        let outcome = client.change_core(ClashCore::Mihomo).await.unwrap();
        assert_eq!(client.get_app_config().unwrap().core, ClashCore::Mihomo);
        assert_eq!(outcome.value(), &());
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
        assert!(
            outcome
                .degradations()
                .iter()
                .all(|degradation| { degradation.code != "application_runtime_deferred" }),
            "legacy mirror failures may degrade the test fixture but not the Core reconcile"
        );
    }

    #[tokio::test]
    async fn change_core_commits_source_even_when_postcommit_runtime_apply_fails() {
        let (client, events) = isolated_core_selection_client(true).await;
        let outcome = client.change_core(ClashCore::ClashRs).await.unwrap();
        assert_eq!(client.get_app_config().unwrap().core, ClashCore::ClashRs);
        assert!(matches!(outcome, MutationOutcome::CommittedDegraded { .. }));
        assert!(outcome.degradations().iter().any(|degradation| {
            degradation.code == "application_runtime_deferred"
                && degradation.message.contains("injected rebuild failure")
        }));
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
    }

    #[tokio::test]
    async fn clash_runtime_failure_is_reported_as_committed_degraded() {
        let (client, events) = recording_client(true).await;
        let previous = client.get_clash_config().unwrap();
        let mut patch = chimera_config::clash::config::ClashConfig::new_empty_patch();
        patch.enable_clash_fields = Some(!previous.enable_clash_fields);
        let outcome = client.patch_clash_config(patch).await.unwrap();
        assert_ne!(
            client.get_clash_config().unwrap().enable_clash_fields,
            previous.enable_clash_fields,
        );
        assert!(matches!(
            &outcome,
            MutationOutcome::CommittedDegraded { .. }
        ));
        assert!(outcome.degradations().iter().any(|degradation| {
            degradation.code == "clash_runtime_deferred"
                && degradation.message.contains("injected rebuild failure")
        }));
        let MutationOutcome::CommittedDegraded { commits, .. } = outcome else {
            unreachable!("failed runtime apply must produce committed degraded");
        };
        assert!(commits.iter().any(|receipt| {
            receipt.domain == "clash" && receipt.runtime == runtime::RuntimeCommitStatus::Deferred
        }));
        assert!(events.lock().unwrap().contains(&"rebuild"));
    }

    #[tokio::test]
    async fn clash_ui_only_patch_commits_without_rebuilding_runtime() {
        let (client, events) = recording_client(false).await;
        let mut patch = chimera_config::clash::config::ClashConfig::new_empty_patch();
        patch.web_ui_list = Some(vec!["https://dashboard.example".into()]);
        let outcome = client.patch_clash_config(patch).await.unwrap();
        assert_eq!(
            client.get_clash_config().unwrap().web_ui_list,
            vec!["https://dashboard.example"],
        );
        assert!(
            outcome
                .degradations()
                .iter()
                .all(|item| item.code != "clash_runtime_deferred")
        );
        assert!(
            !events.lock().unwrap().contains(&"rebuild"),
            "source-only patch must not mutate Core",
        );
    }

    #[tokio::test]
    async fn stop_core_runs_through_the_injected_lifecycle_port() {
        let (client, events) = recording_client(false).await;
        client.stop_core().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["stop"]);
    }

    #[tokio::test]
    async fn runtime_rebuild_does_not_emit_profile_change_side_effects() {
        let (client, events) = recording_client(false).await;
        client.rebuild_running_config().await.unwrap();
        assert_eq!(events.lock().unwrap().as_slice(), ["rebuild", "refresh-ui"]);
    }

    #[tokio::test]
    async fn rebuild_failure_stops_follow_up_side_effects() {
        let (client, events) = recording_client(true).await;
        let error = client.rebuild_running_config().await.unwrap_err();
        assert!(error.to_string().contains("injected rebuild failure"));
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["rebuild", "refresh-diagnostics"]
        );
    }

    #[test]
    fn auto_activation_uses_actual_typed_profile_conflict_without_changing_ipc() {
        use chimera_contracts::{
            profiles::command_failure::ProfileFailure, runtime::DegradationReason,
        };

        let profile = ProfileId("created-profile".into());
        let error = ProfilesError::VersionConflict {
            expected: 2,
            actual: 3,
        };
        let detail =
            chimera_core::client::auto_activation_failure_degradation(profile.clone(), &error);
        match &detail.reason {
            DegradationReason::ProfileAutoActivationFailed {
                profile: uid,
                cause,
            } => {
                assert_eq!(uid, &profile);
                assert!(matches!(
                    cause.as_ref(),
                    ProfileFailure::VersionConflict {
                        expected: 2,
                        actual: 3,
                        cleanup: None
                    }
                ));
            }
            other => panic!("unexpected autoactivation cause: {other:?}"),
        }
        let nested = serde_json::to_value(&detail).unwrap();
        assert_eq!(nested["reason"]["code"], "profile_auto_activation_failed");
        assert_eq!(nested["reason"]["profile"], "created-profile");
        assert_eq!(nested["reason"]["cause"]["kind"], "version_conflict");

        let legacy = Degradation::from_shared(detail);
        let wire = serde_json::to_value(legacy).unwrap();
        assert_eq!(wire["code"], "profile_auto_activation_failed");
        assert_eq!(wire["phase"], "system_effect");
        assert_eq!(wire["retryable"], true);
        assert!(wire.get("reason").is_none());
        let legacy_source = chimera_core::client::auto_activation_failure_degradation(
            profile,
            &ProfilesError::Persist("untyped source commit error".into()),
        );
        assert_eq!(
            serde_json::to_value(&legacy_source).unwrap()["reason"]["cause"]["kind"],
            "persist"
        );
        let wire = serde_json::to_value(Degradation::from_shared(legacy_source)).unwrap();
        assert_eq!(wire["code"], "profile_auto_activation_failed");
        assert_eq!(
            wire["message"],
            "failed to persist profiles: untyped source commit error"
        );
    }

    #[test]
    fn mutation_outcome_serializes_ref_wire() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        assert_eq!(
            serde_json::to_string(&applied).unwrap(),
            r#"{"status":"committed","value":null,"commits":[],"notifications_pending":true}"#
        );
        let degraded = MutationOutcome::from_parts(
            (),
            vec![Degradation {
                phase: DegradationPhase::RuntimeBuild,
                code: "runtime_rebuild_failed".into(),
                message: "boom".into(),
                retryable: true,
            }],
        );
        let json = serde_json::to_string(&degraded).unwrap();
        assert!(json.contains(r#""status":"committed_degraded""#));
        assert!(json.contains(r#""phase":"runtime_build""#));
    }
}
