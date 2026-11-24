//! Instance-owned application facade for runtime, profile, and core operations.
//!
//! This follows REF's client ownership direction while preserving Chimera's
//! staged migration from legacy globals and Chimera-specific core support.

mod app_lifecycle;
mod application;
pub(crate) mod application_workflow;
mod clash_api;
mod clash_config;
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
pub(crate) mod runtime_build;
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
use chimera_config::application::ReleaseChannel;
use chimera_config::profile::{
    FolderId, NewProfileRequest, ProfileDefinition, ProfileId, ProfileLayout, ProfileMetadata,
    ProfileMetadataPatch, ProfileSource, Profiles, RemoteProfileOptions, RemoteProfileOptionsPatch,
    TransformKind,
};
use chimera_contracts::{
    bundle_metadata::BundleMetadata, net::ChangelogHttp, system_dns::SystemDnsCache,
};
use chimera_core::{
    actor_v2::{
        CoreClient as CoreClientV2, router::CoreStatusProjection, service_actor::ServiceClient,
        service_router::ServiceHostStatus,
    },
    client::app_update::{self, AppUpdateArgs, AppUpdateClient, AppUpdateSettings},
    profile_import::url_derived_name,
};
use struct_patch::Patch as _;

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

use crate::{
    service::profile_file::ProfileFileService,
    state::mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
    state::mutation::MutationCoordinator,
    state::profiles::{CommitReport, ProfilesError, ReorderOp},
    utils::path::PathResolver,
};

pub use runtime::RuntimePaths;

#[derive(Clone)]
pub(crate) struct LegacyBridgeSet {
    pub(crate) verge: Arc<dyn VergeLegacyBridge>,
    pub(crate) window: Arc<dyn WindowLegacyBridge>,
    pub(crate) clash: Arc<dyn ClashLegacyBridge>,
}

pub(crate) struct ClientSetupArgs {
    pub(crate) paths: PathResolver,
    pub runtime_paths: RuntimePaths,
    pub(crate) bundle_metadata: BundleMetadata,
    pub(crate) app_update_backend: Arc<dyn app_update::AppUpdateBackend>,
    pub(crate) app_update_events: Arc<dyn app_update::AppUpdateEventSink>,
    pub(crate) changelog_http: Arc<dyn ChangelogHttp>,
    pub(crate) bridges: LegacyBridgeSet,
    /// The ref Core/Service Actor handles from the composition root.
    /// Legacy lifecycle ports remain compatibility adapters, not owners.
    pub(crate) core_v2: CoreClientV2,
    pub(crate) service_v2: ServiceClient,
    pub(crate) core: Arc<dyn CoreLifecyclePort>,
    pub(crate) service: Arc<dyn core_lifecycle::ServiceLifecyclePort>,

    pub(crate) profile_service: Arc<ProfileFileService>,

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

async fn new_typed_config_clients(
    paths: &PathResolver,
    bridges: &LegacyBridgeSet,
    build_channel: ReleaseChannel,
) -> anyhow::Result<TypedConfigClients> {
    let application = ApplicationClient::new(
        utf8_path(paths.application_config_path())?,
        bridges.verge.snapshot_legacy()?,
        bridges.verge.clone(),
        build_channel,
    )
    .await?;
    let session_state = SessionStateClient::new(
        utf8_path(paths.session_state_path())?,
        bridges.window.snapshot_legacy()?,
        bridges.window.clone(),
    )
    .await?;
    let clash_config = ClashConfigClient::new(
        utf8_path(paths.clash_config_path())?,
        bridges.clash.snapshot_legacy()?,
        bridges.clash.clone(),
    )
    .await?;

    Ok(TypedConfigClients {
        application,
        session_state,
        clash_config,
    })
}

struct ChimeraClientInner {
    application: ApplicationClient,
    session_state: SessionStateClient,
    clash_config: ClashConfigClient,
    app_updater: Option<AppUpdateClient>,
    changelog_http: Option<Arc<dyn ChangelogHttp>>,
    bundle_metadata: Option<BundleMetadata>,
    core_lifecycle: RuntimeReconcilerClient,
    core_api: Option<CoreClientV2>,
    service_actor: Option<ServiceClient>,
    /// Ref CoreFacade is the read-only host/API authority for the production
    /// Client. Lifecycle mutations remain with the legacy bridge until the
    /// workflow and facade are moved as one stateful owner.
    ref_facade: Option<chimera_core::actor_v2::facade::CoreFacade>,
    core: Arc<dyn CoreLifecyclePort>,
    shutdown: tokio_util::sync::CancellationToken,
    tasks: tokio_util::task::TaskTracker,
    file_logs: Option<chimera_core::client::logs::FileLogsClient>,
    streams: chimera_core::clash::ws::StreamsClient,
    streams_shutdown: tokio_util::sync::CancellationToken,
    proxies: chimera_core::proxies_actor::ProxiesClient,
    typed_profiles: Option<profiles::ProfilesClient>,
    /// The one candidate checker shared by the Profiles gate and legacy
    /// Clash read-only preflight during the staged ref source cutover.
    checked_gate: Option<Arc<reconciler::check::CandidateCheck>>,
    profile_service: Arc<ProfileFileService>,
    system_dns: Arc<dyn SystemDnsCache>,
    traffic_store: parking_lot::Mutex<Option<Arc<chimera_core::traffic::TrafficFileStorage>>>,
    traffic: parking_lot::RwLock<Option<crate::core::traffic::TrafficClient>>,
    /// The same opt-in source-address cache serves the traffic recorder and
    /// the explicit public-egress probe, without spawning a second probe.
    local_source: Arc<traffic::LocalSourceCache>,
    geo_index: parking_lot::RwLock<Option<crate::core::geo::GeoIndexClient>>,
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

impl Drop for ChimeraClientInner {
    fn drop(&mut self) {
        self.streams_shutdown.cancel();
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
            changelog_http,
            bridges,
            core_v2,
            service_v2,
            core,
            service,
            profile_service,
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
        let typed = tauri::async_runtime::block_on(new_typed_config_clients(
            &paths,
            &bridges,
            bundle_metadata.release_channel,
        ))?;
        let streams_shutdown = shutdown.child_token();
        // Keep Core log shards in a dedicated subdirectory, never in the
        // app/service text-log rotation namespace. Optional storage failure
        // must not prevent the desktop or core from starting.
        let core_log_dir = paths.app_logs_dir().join("core");
        let core_log_store = tauri::async_runtime::block_on(async {
            tokio::task::spawn_blocking(move || {
                chimera_core::logs::RedbCoreLogStore::open(core_log_dir)
            })
            .await
        });
        let core_log_store: Box<dyn chimera_core::logs::CoreLogStore> = match core_log_store {
            Ok(Ok(store)) => Box::new(store),
            Ok(Err(error)) => Box::new(chimera_core::logs::UnavailableCoreLogStore(
                error.to_string(),
            )),
            Err(error) => Box::new(chimera_core::logs::UnavailableCoreLogStore(
                error.to_string(),
            )),
        };
        let core_logs = tauri::async_runtime::block_on(chimera_core::logs::CoreLogsClient::spawn(
            core_log_store,
            typed.application.get_typed().core_logs,
            shutdown.child_token(),
            &tasks,
        ))?;
        let streams =
            tauri::async_runtime::block_on(chimera_core::clash::ws::StreamsClient::spawn(
                core_v2.clone(),
                core_logs,
                typed.clash_config.get()?.overrides.log_level(),
                streams_shutdown.clone(),
                &tasks,
            ))?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        effects_port.bind_streams(streams.clone())?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let effects = tauri::async_runtime::block_on(async {
            let initial = effects::plan::ApplicationEffectInputs::project(
                &typed.application.get_typed(),
                &typed
                    .clash_config
                    .get()
                    .expect("test clash config should be readable"),
                None,
            );
            effects::actor::EffectsClient::spawn(effects::actor::EffectsArgs {
                port: effects_port,
                initial,
            })
            .await
        })?;
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let mutations = MutationCoordinator::pending_with_notifications(Arc::new((
            effects.clone(),
            event_sink::UiCommitNotifications(ui_sink.clone()),
        )));
        #[cfg(any(target_os = "android", target_os = "ios"))]
        let mutations = MutationCoordinator::pending();
        let typed_profiles =
            tauri::async_runtime::block_on(profiles::ProfilesClient::new_with_jobs(
                mutations.clone(),
                utf8_path(paths.profiles_path())?,
                jobs,
                profile_service.clone(),
                profile_service.clone(),
                profile_service.clone(),
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
        )?);
        // Both Application and Clash legacy postcommit paths validate via
        // the exact Profiles production checker. Install it in the sole
        // CoreLifecycle workflow before opening Profile mutation admission.
        tauri::async_runtime::block_on(
            core_lifecycle.install_candidate_gate(profile_gate.clone()),
        )?;
        tauri::async_runtime::block_on(
            mutations.connect_with_runtime_gate(core_lifecycle.clone(), profile_gate.clone()),
        )?;
        let proxies =
            tauri::async_runtime::block_on(chimera_core::proxies_actor::ProxiesClient::spawn(
                chimera_core::proxies_actor::CoreClient::new(Arc::new(core_v2.clone())),
                shutdown.clone(),
                &tasks,
            ))?;
        let app_config = typed.application.get_typed();
        let channel = bundle_metadata
            .release_channel
            .resolve(app_config.release_channel);
        let app_updater = tauri::async_runtime::block_on(AppUpdateClient::spawn(
            AppUpdateArgs {
                backend: app_update_backend,
                events: app_update_events,
                settings: AppUpdateSettings {
                    channel,
                    sources: app_config.update_sources,
                    auto_check: app_config.enable_auto_check_update,
                    auto_download: app_config.enable_auto_download_update,
                },
                supported: !bundle_metadata.is_portable
                    && (cfg!(any(target_os = "windows", target_os = "macos"))
                        || (cfg!(target_os = "linux") && bundle_metadata.is_appimage)),
                endpoints: app_update::endpoints::update_endpoints_for_target(
                    channel,
                    bundle_metadata.sourceforge_project,
                    bundle_metadata.is_fixed_webview,
                ),
                bundle_metadata,
                shutdown: shutdown.child_token(),
            },
            &tasks,
        ))?;
        let file_logs = tauri::async_runtime::block_on(
            chimera_core::client::logs::FileLogsClient::start(paths.app_logs_dir()),
        )
        .ok();
        if let Some(logs) = file_logs.clone() {
            let token = shutdown.clone();
            tasks.spawn_on(
                async move {
                    token.cancelled().await;
                    let _ = logs.shutdown().await;
                },
                tauri::async_runtime::handle().inner(),
            );
        }
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
            profile_service,
            Some(typed_profiles),
            Some(profile_gate),
            system_dns,
            ui_sink,
            Some(app_updater),
            Some(changelog_http),
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
        *client.inner.traffic_store.lock() = traffic_store;
        let has_traffic_store = client.inner.traffic_store.lock().is_some();
        if has_traffic_store {
            let geo_index =
                tauri::async_runtime::block_on(crate::core::geo::GeoIndexClient::spawn(
                    crate::core::geo::GeoIndexArgs {
                        source: Arc::new(crate::core::geo::FsCountryIndexSource::new(
                            paths.app_data_dir().to_path_buf(),
                            paths.cache_dir().join("geodata-index"),
                        )),
                        mode_source: Arc::new(crate::client::traffic::ActiveCoreGeodataMode(
                            client
                                .inner
                                .core_api
                                .as_ref()
                                .context("production geodata needs the ref CoreActor")?
                                .clone(),
                        )),
                    },
                    shutdown,
                    &tasks,
                ));
            match geo_index {
                Ok(geo_index) => *client.inner.geo_index.write() = Some(geo_index),
                Err(error) => tracing::warn!(%error, "traffic geography index is unavailable"),
            }
        }
        // ref's CoreActor publishes host/instance changes independently of
        // application configuration commits. Subscribe once from the Tauri
        // reactor after the complete Client exists; a sync setup-thread spawn
        // here would panic without an entered Tokio runtime.
        anyhow::ensure!(
            tauri::async_runtime::block_on(async { client.start_core_status_observer() }),
            "client shutdown began before its CoreActor observer was registered"
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
        let proxies = chimera_core::proxies_actor::ProxiesClient::spawn(
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
            None,
            None,
            system_dns,
            ui_sink,
            None,
            None,
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
        proxies: chimera_core::proxies_actor::ProxiesClient,
        profile_service: Arc<ProfileFileService>,
        typed_profiles: Option<profiles::ProfilesClient>,
        checked_gate: Option<Arc<reconciler::check::CandidateCheck>>,
        system_dns: Arc<dyn SystemDnsCache>,
        ui_sink: Arc<dyn UiEventSink>,
        app_updater: Option<AppUpdateClient>,
        changelog_http: Option<Arc<dyn ChangelogHttp>>,
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
        let ref_facade = core_api
            .as_ref()
            .zip(service_actor.as_ref())
            .map(|(core, service)| {
                chimera_core::actor_v2::facade::CoreFacade::new(core.clone(), service.clone())
            });
        let local_source = Arc::new(traffic::LocalSourceCache::new(
            typed.application.clone(),
            typed.clash_config.clone(),
        ));
        let inner = ChimeraClientInner {
            application: typed.application,
            session_state: typed.session_state,
            clash_config: typed.clash_config,
            app_updater,
            changelog_http,
            bundle_metadata,
            core_lifecycle,
            core_api,
            service_actor,
            ref_facade,
            core,
            shutdown,
            tasks,
            file_logs,
            streams,
            streams_shutdown,
            proxies,
            typed_profiles,
            checked_gate,
            profile_service,
            system_dns,
            traffic_store: parking_lot::Mutex::new(None),
            traffic: parking_lot::RwLock::new(None),
            local_source,
            geo_index: parking_lot::RwLock::new(None),
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
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) async fn unregister_hotkeys(&self) {
        for status in self.inner.effects.shutdown().await {
            if let crate::client::effects::status::EffectHealth::Degraded {
                code, message, ..
            } = status.health
            {
                tracing::warn!(%code, %message, "failed to release application effects at shutdown");
            }
        }
    }
}

impl ChimeraClient {
    /// Read the ref Core Actor's authoritative host and generation projection.
    /// The legacy CoreLifecyclePort remains a Chimera runtime-config adapter,
    /// not the owner of Core routing state.
    pub(crate) fn routed_core_status(&self) -> Option<CoreStatusProjection> {
        if let Some(facade) = &self.inner.ref_facade {
            return Some(facade.core_status());
        }
        self.inner.core_api.as_ref().map(CoreClientV2::status)
    }

    /// Read the ref Service Actor directly, without asking the old Facade to
    /// reconstruct its own Service phase from a separate IPC status cache.
    pub(crate) fn service_actor_status(&self) -> Option<ServiceHostStatus> {
        if let Some(facade) = &self.inner.ref_facade {
            return Some(facade.service_status());
        }
        self.inner.service_actor.as_ref().map(ServiceClient::status)
    }

    pub(crate) fn shutdown_child_token(&self) -> tokio_util::sync::CancellationToken {
        self.inner.shutdown.child_token()
    }

    pub(crate) fn spawn_tracked(
        &self,
        task: impl std::future::Future<Output = ()> + Send + 'static,
    ) -> bool {
        if self.inner.shutdown.is_cancelled() {
            return false;
        }
        self.inner.tasks.spawn(task);
        true
    }

    pub(crate) fn file_logs(
        &self,
    ) -> chimera_logging::LogResult<&chimera_core::client::logs::FileLogsClient> {
        self.inner
            .file_logs
            .as_ref()
            .ok_or(chimera_logging::LogError::Unavailable)
    }

    pub(crate) fn streams(&self) -> &chimera_core::clash::ws::StreamsClient {
        &self.inner.streams
    }

    fn profiles_client(&self) -> anyhow::Result<&profiles::ProfilesClient> {
        self.inner
            .typed_profiles
            .as_ref()
            .ok_or_else(|| anyhow!("typed ProfilesClient is not configured"))
    }

    fn refresh_profiles(&self) {
        self.inner.proxies.invalidate();
        self.inner.ui_sink.refresh_profiles();
    }

    async fn collect_post_commit_degradations(&self, report: &CommitReport) -> Vec<Degradation> {
        let mut degradations = report
            .degradations
            .iter()
            .map(|degradation| {
                tracing::warn!(
                    phase = ?degradation.phase,
                    code = ?degradation.code,
                    retryable = degradation.code.retryable(),
                    message = %degradation.message,
                    "profile commit completed with a degraded side effect",
                );
                application_workflow::profiles::map_profile_degradation(degradation)
            })
            .collect::<Vec<_>>();
        degradations.extend(report.runtime_degradations.clone());
        degradations
    }

    async fn after_commit(&self, report: &CommitReport) -> MutationOutcome<()> {
        MutationOutcome::from_parts((), self.collect_post_commit_degradations(report).await)
            .with_commit(report.receipt.clone())
    }

    fn auto_activation_failure_degradation(error: &impl std::fmt::Display) -> Degradation {
        tracing::warn!(
            %error,
            "profile auto-activation failed after commit; retaining committed profile id",
        );
        Degradation {
            phase: DegradationPhase::SystemEffect,
            code: "profile_auto_activation_failed".into(),
            message: error.to_string(),
            retryable: true,
        }
    }

    /// Match ref's post-commit profile activation reason only when the
    /// actual ProfilesActor failure supplies a lossless shared cause. Other
    /// errors (including a missing ProfilesClient) retain the legacy flat
    /// diagnostic instead of impersonating an upstream ProfilesError.
    fn typed_auto_activation_failure(
        profile: &ProfileId,
        error: &ProfilesError,
    ) -> Option<chimera_contracts::runtime::Degradation> {
        use chimera_contracts::runtime::{
            Degradation as SharedDegradation, DegradationPhase as SharedPhase,
            DegradationReason as SharedReason,
        };

        Some(SharedDegradation {
            phase: SharedPhase::SystemEffect,
            reason: SharedReason::ProfileAutoActivationFailed {
                profile: profile.clone(),
                cause: Box::new(error.typed_failure()?),
            },
            message: error.to_string(),
            retryable: true,
        })
    }

    async fn try_auto_activate_if_none(&self, uid: ProfileId) -> MutationOutcome<()> {
        let profiles = match self.profiles_client() {
            Ok(profiles) => profiles.clone(),
            Err(error) => {
                return MutationOutcome::from_parts(
                    (),
                    vec![Self::auto_activation_failure_degradation(&error)],
                );
            }
        };
        match profiles.set_current_if_none(uid.clone()).await {
            Ok(None) => MutationOutcome::from_parts((), Vec::new()),
            Ok(Some(report)) => {
                self.refresh_profiles();
                self.after_commit(&report).await
            }
            Err(error) => {
                let degradation = if let Some(typed) =
                    Self::typed_auto_activation_failure(&uid, &error)
                {
                    tracing::warn!(%error, profile = %uid, "typed profile auto-activation failed after commit");
                    Degradation::from_shared(typed)
                } else {
                    Self::auto_activation_failure_degradation(&error)
                };
                MutationOutcome::from_parts((), vec![degradation])
            }
        }
    }

    pub(crate) fn get_profiles(&self) -> anyhow::Result<Arc<Profiles>> {
        Ok(self.profiles_client()?.snapshot())
    }

    /// The secret-free profile listing for read-only consumers. The full
    /// source document remains available through `get_profiles` for editors.
    pub(crate) fn get_profiles_listing(
        &self,
    ) -> anyhow::Result<chimera_config::profile::ProfilesListing> {
        Ok((&*self.profiles_client()?.snapshot()).into())
    }

    pub(crate) async fn get_transparent_proxy_status(
        &self,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        if let Some(core) = &self.inner.core_api {
            return Ok(core.transparent_proxy_status().await?);
        }
        // Test-only legacy lifecycle injection.
        self.inner.core.transparent_proxy_status().await
    }

    pub(crate) async fn profile_sync_status(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<crate::client::jobs::ProfileSyncStatus> {
        Ok(self.profiles_client()?.profile_sync_status(uid).await?)
    }

    pub(crate) async fn profile_sync_runs(
        &self,
        uid: ProfileId,
        after: Option<chimera_jobs::dto::RunCursorDto>,
    ) -> anyhow::Result<chimera_jobs::dto::RunPageDto> {
        Ok(self
            .profiles_client()?
            .profile_sync_runs(uid, after)
            .await?)
    }

    pub(crate) async fn profile_sync_logs(
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

    pub(crate) async fn add_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        if matches!(
            request.definition.source(),
            Some(ProfileSource::Remote { .. })
        ) {
            return Err(anyhow!(
                "remote profiles must be created via import_profile"
            ));
        }
        let report = self.profiles_client()?.add(request, initial_file).await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| anyhow!("profile add committed without a created uid"))?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            created,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    pub(crate) async fn create_profile(
        &self,
        request: NewProfileRequest,
        initial_file: Option<String>,
    ) -> anyhow::Result<MutationOutcome<ProfileId>> {
        let is_config = matches!(request.definition, ProfileDefinition::Config { .. });
        let mut outcome = self.add_profile(request, initial_file).await?;
        if is_config {
            let uid = outcome.value().clone();
            outcome = outcome.append_commit_result(self.try_auto_activate_if_none(uid).await);
        }
        Ok(outcome)
    }

    /// Download and suggest an import kind without committing a source document.
    // The desktop IPC is introduced with the generated frontend contract.
    #[allow(dead_code)]
    pub(crate) async fn probe_profile_url(
        &self,
        url: url::Url,
        options: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<chimera_core::profile_import::ProfileUrlProbe> {
        let mut option = RemoteProfileOptions::default();
        if let Some(patch) = options {
            option.apply(patch);
        }
        Ok(self.profiles_client()?.probe(url, option).await?)
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
        let update_interval_explicit = patch
            .as_ref()
            .and_then(|patch| patch.update_interval_minutes)
            .is_some();
        let (name, custom_name) = match name {
            Some(name) if !name.trim().is_empty() => (name, true),
            _ => (url_derived_name(&url), false),
        };
        let mut options = RemoteProfileOptions::default();
        if let Some(patch) = patch {
            options.apply(patch);
        }
        let report = self
            .profiles_client()?
            .import_with_transform(
                url,
                transform,
                ProfileMetadata {
                    name,
                    desc: None,
                    custom_name,
                },
                options,
                update_interval_explicit,
                folder,
            )
            .await?;
        let created = report
            .created
            .clone()
            .ok_or_else(|| anyhow!("profile import committed without a created uid"))?;
        self.refresh_profiles();
        let outcome = MutationOutcome::from_parts(
            created.clone(),
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone());
        if transform.is_some() {
            return Ok(outcome);
        }
        Ok(outcome.append_commit_result(self.try_auto_activate_if_none(created).await))
    }

    pub(crate) async fn delete_profile(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.delete(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn reorder_profile(
        &self,
        active: ProfileId,
        over: ProfileId,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .reorder(ReorderOp::Move { active, over })
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn reorder_profiles_by_list(
        &self,
        list: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .reorder(ReorderOp::ByList(list))
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    /// Atomically persist the Profile folders and Config display order.
    pub(crate) async fn set_profile_layout(
        &self,
        layout: ProfileLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_layout(layout).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    /// Create a folder and move Config members into it without a second write.
    pub(crate) async fn create_profile_folder(
        &self,
        name: String,
        members: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        let (report, folder) = self.profiles_client()?.create_folder(name, members).await?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            folder,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    /// Presentation-only proxy group ordering for one Config Profile.
    pub(crate) async fn set_proxy_group_layout(
        &self,
        profile: ProfileId,
        layout: chimera_config::profile::ProxyGroupLayout,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .set_proxy_group_layout(profile, layout)
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn create_proxy_group_folder(
        &self,
        profile: ProfileId,
        name: String,
        members: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<FolderId>> {
        let (report, folder) = self
            .profiles_client()?
            .create_proxy_group_folder(profile, name, members)
            .await?;
        self.refresh_profiles();
        Ok(MutationOutcome::from_parts(
            folder,
            self.collect_post_commit_degradations(&report).await,
        )
        .with_commit(report.receipt.clone()))
    }

    pub(crate) async fn refresh_profile(
        &self,
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.refresh(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn patch_profile_metadata(
        &self,
        uid: ProfileId,
        patch: ProfileMetadataPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.patch_metadata(uid, patch).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn patch_remote_profile_options(
        &self,
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self
            .profiles_client()?
            .patch_remote_options(uid, patch)
            .await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn replace_profile_definition(
        &self,
        uid: ProfileId,
        definition: ProfileDefinition,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self
            .profiles_client()?
            .replace_definition(uid, definition)
            .await
        {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn activate_profile(
        &self,
        uid: Option<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_current(uid).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn set_global_transforms(
        &self,
        ids: Vec<ProfileId>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.set_global_transforms(ids).await {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn set_profile_valid_fields(
        &self,
        fields: Vec<String>,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = self.profiles_client()?.set_valid_fields(fields).await?;
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn save_profile_file(
        &self,
        uid: ProfileId,
        data: String,
    ) -> anyhow::Result<MutationOutcome<()>> {
        let report = match self.profiles_client()?.save_file(uid, data).await {
            Ok(report) => report,
            Err(error) => {
                self.inner.ui_sink.refresh_runtime_transform_diagnostics();
                return Err(error.into());
            }
        };
        self.refresh_profiles();
        Ok(self.after_commit(&report).await)
    }

    pub(crate) async fn read_profile_file(&self, uid: ProfileId) -> anyhow::Result<String> {
        let snapshot = self.profiles_client()?.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        let raw = crate::state::profiles::ports::ProfileFsPort::read(
            self.inner.profile_service.as_ref(),
            &source.materialized().file,
        )?;
        profile_file_text_for_editor(raw, item.definition.is_config())
    }

    pub(crate) async fn get_profile_materialized_path(
        &self,
        uid: ProfileId,
    ) -> anyhow::Result<std::path::PathBuf> {
        let snapshot = self.profiles_client()?.snapshot();
        let item = snapshot
            .items
            .get(&uid)
            .ok_or(ProfilesError::ProfileNotFound(uid))?;
        let source = item
            .definition
            .source()
            .ok_or(ProfilesError::ProfileHasNoFile)?;
        self.inner
            .profile_service
            .resolve_path(&source.materialized().file)
            .map_err(Into::into)
    }
}

/// REF parity: config Profiles are YAML mappings, but transforms and scripts
/// must be returned byte-for-byte as text (even when they are not valid YAML).
fn profile_file_text_for_editor(raw: String, is_config: bool) -> anyhow::Result<String> {
    if is_config {
        crate::service::profile_file::normalize_yaml_document(&raw)
            .context("failed to normalize profile YAML")
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    #[test]
    fn transform_profile_file_read_preserves_non_yaml_source_exactly() {
        let javascript = "function main(config) { return config; }\n// [invalid YAML";
        let lua = "return { [\"proxies\"] = {} }\n-- not a YAML mapping";
        for source in [javascript, lua] {
            assert_eq!(
                super::profile_file_text_for_editor(source.into(), false).unwrap(),
                source,
                "script and merge transform content must never be normalized as YAML"
            );
        }
    }

    #[test]
    fn config_profile_file_read_normalizes_yaml_and_rejects_invalid_mapping() {
        let normalized = super::profile_file_text_for_editor("mode: rule\n".into(), true).unwrap();
        assert_eq!(normalized, "mode: rule\n");
        let error =
            super::profile_file_text_for_editor("not: [valid YAML".into(), true).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to normalize profile YAML")
        );
    }

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
        client.inner.shutdown.cancel();
        client.inner.tasks.close();
        client.inner.tasks.wait().await;
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
            .core_api = Some(core_api.clone());

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
            client.inner.core_lifecycle.service_status().phase,
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
            .service_actor = Some(actor.clone());

        assert_eq!(client.service_status().phase, ServicePhase::NotInstalled);
        assert_eq!(
            client.inner.core_lifecycle.service_status().phase,
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
        inner.ref_facade = Some(chimera_core::actor_v2::facade::CoreFacade::new(
            core.clone(),
            service.clone(),
        ));

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
            .core_api = Some(core.clone());

        // A bare TaskTracker::spawn here would panic without a reactor. The
        // composition root must enter Tauri's runtime for registration.
        assert!(tauri::async_runtime::block_on(async {
            client.start_core_status_observer()
        }));

        client.request_shutdown();
        tauri::async_runtime::block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), client.inner.tasks.wait())
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
            .core_api = Some(core.clone());

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
        tokio::time::timeout(std::time::Duration::from_secs(5), client.inner.tasks.wait())
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
        inner.application = application.clone();
        inner.core_lifecycle = RuntimeReconcilerClient::direct(
            inner.core.clone(),
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
        let detail = ChimeraClient::typed_auto_activation_failure(&profile, &error)
            .expect("a real MVCC conflict has a lossless structured cause");
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
        assert!(
            ChimeraClient::typed_auto_activation_failure(
                &profile,
                &ProfilesError::Persist("untyped source commit error".into())
            )
            .is_none()
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
