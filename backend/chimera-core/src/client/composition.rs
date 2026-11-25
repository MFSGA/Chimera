//! Shared owner construction following ref client::with_parts ordering.
//! The desktop root still owns partial setup cleanup and host-specific ports.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use super::effects::{
    actor::{EffectsArgs, EffectsClient},
    executor::CoreRuntimeEffects,
    plan::ApplicationEffectInputs,
    ports::ApplicationEffectsPort,
};
use super::{
    app_update::{
        self, AppUpdateArgs, AppUpdateBackend, AppUpdateClient, AppUpdateEventSink,
        AppUpdateSettings,
    },
    logs::FileLogsClient,
};
use crate::{
    actor_v2::CoreClient,
    clash::ws::StreamsClient,
    logs::{CoreLogStore, CoreLogsClient, RedbCoreLogStore, UnavailableCoreLogStore},
    proxies::ProxiesClient,
};
use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use chimera_contracts::bundle_metadata::BundleMetadata;
use std::{path::PathBuf, sync::Arc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct RuntimeObservers {
    pub streams: StreamsClient,
    pub streams_shutdown: CancellationToken,
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub effects: EffectsClient,
}

/// The capture stream exists before the effects owner can set its level.
pub async fn spawn_runtime_observers(
    core_log_dir: PathBuf,
    app: &ChimeraAppConfig,
    clash: &ClashConfig,
    core: CoreClient,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
    #[cfg(not(any(target_os = "android", target_os = "ios")))] effects: Arc<
        dyn ApplicationEffectsPort,
    >,
) -> anyhow::Result<RuntimeObservers> {
    let streams_shutdown = shutdown.child_token();
    // Optional log storage must not prevent Core startup.
    let store: Box<dyn CoreLogStore> =
        match tokio::task::spawn_blocking(move || RedbCoreLogStore::open(core_log_dir)).await {
            Ok(Ok(store)) => Box::new(store),
            Ok(Err(error)) => Box::new(UnavailableCoreLogStore(error.to_string())),
            Err(error) => Box::new(UnavailableCoreLogStore(error.to_string())),
        };
    let core_logs =
        CoreLogsClient::spawn(store, app.core_logs, shutdown.child_token(), tasks).await?;
    let streams = StreamsClient::spawn(
        core.clone(),
        core_logs,
        clash.overrides.log_level(),
        streams_shutdown.clone(),
        tasks,
    )
    .await?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let effects = EffectsClient::spawn_tracked(
        EffectsArgs {
            port: Arc::new(CoreRuntimeEffects::new(effects, streams.clone(), core)),
            initial: ApplicationEffectInputs::project(app, clash, None),
        },
        shutdown.child_token(),
        tasks,
    )
    .await?;
    Ok(RuntimeObservers {
        streams,
        streams_shutdown,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        effects,
    })
}

pub struct ApplicationObservers {
    pub proxies: ProxiesClient,
    pub app_updater: AppUpdateClient,
    pub file_logs: Option<FileLogsClient>,
}

/// Start after the source mutation gate is installed, as in the existing graph.
pub async fn spawn_application_observers(
    core: CoreClient,
    app: &ChimeraAppConfig,
    bundle_metadata: BundleMetadata,
    backend: Arc<dyn AppUpdateBackend>,
    events: Arc<dyn AppUpdateEventSink>,
    logs_dir: PathBuf,
    shutdown: CancellationToken,
    tasks: &TaskTracker,
) -> anyhow::Result<ApplicationObservers> {
    let proxies = ProxiesClient::spawn(core, shutdown.clone(), tasks).await?;
    let channel = bundle_metadata.release_channel.resolve(app.release_channel);
    let app_updater = AppUpdateClient::spawn(
        AppUpdateArgs {
            backend,
            events,
            settings: AppUpdateSettings {
                channel,
                sources: app.update_sources.clone(),
                auto_check: app.enable_auto_check_update,
                auto_download: app.enable_auto_download_update,
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
        tasks,
    )
    .await?;
    let file_logs = FileLogsClient::start(logs_dir).await.ok();
    if let Some(logs) = file_logs.clone() {
        // Ref's log client exposes no actor cell: a tracked task drains it.
        let token = shutdown.child_token();
        tasks.spawn(async move {
            token.cancelled().await;
            if let Err(error) = logs.shutdown().await {
                tracing::warn!(%error, "the application log did not shut down cleanly");
            }
        });
    }
    Ok(ApplicationObservers {
        proxies,
        app_updater,
        file_logs,
    })
}

use super::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    session_state::SessionStateClient,
};
use crate::state::{
    mirror::{ClashLegacyBridge, VergeLegacyBridge, WindowLegacyBridge},
    mutation::MutationCoordinator,
};
use chimera_config::application::ReleaseChannel;
use chimera_paths::PathResolver;

#[derive(Clone)]
pub struct LegacyBridgeSet {
    pub verge: Arc<dyn VergeLegacyBridge>,
    pub window: Arc<dyn WindowLegacyBridge>,
    pub clash: Arc<dyn ClashLegacyBridge>,
}

pub struct TypedConfigClients {
    pub application: ApplicationClient,
    pub session_state: SessionStateClient,
    pub clash_config: ClashConfigClient,
}

pub async fn new_typed_config_clients(
    paths: &PathResolver,
    bridges: &LegacyBridgeSet,
    build_channel: ReleaseChannel,
    mutations: MutationCoordinator,
    shutdown: tokio_util::sync::CancellationToken,
) -> anyhow::Result<TypedConfigClients> {
    let application = ApplicationClient::new_with_mutations(
        paths.application_config_path(),
        bridges.verge.snapshot_legacy()?,
        bridges.verge.clone(),
        build_channel,
        mutations.clone(),
        shutdown.clone(),
    )
    .await?;
    let session_state = SessionStateClient::new(
        paths.session_state_path(),
        bridges.window.snapshot_legacy()?,
        bridges.window.clone(),
    )
    .await?;
    let clash_config = ClashConfigClient::new_with_mutations(
        paths.clash_config_path(),
        bridges.clash.snapshot_legacy()?,
        bridges.clash.clone(),
        mutations,
        shutdown,
    )
    .await?;

    Ok(TypedConfigClients {
        application,
        session_state,
        clash_config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use chimera_config::state::PersistentState;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingBridges(Mutex<Vec<&'static str>>);

    impl VergeLegacyBridge for RecordingBridges {
        fn prepare(
            &self,
            snap: &ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            assert!(snap.enable_local_ip_probe);
            self.0.lock().unwrap().push("application");
            Ok(Box::new(NoopPreparedLegacyMirror))
        }
        fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
            Ok(ChimeraAppConfig::default())
        }
    }
    impl WindowLegacyBridge for RecordingBridges {
        fn prepare(&self, _: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            self.0.lock().unwrap().push("session");
            Ok(Box::new(NoopPreparedLegacyMirror))
        }
        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }
    impl ClashLegacyBridge for RecordingBridges {
        fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            assert!(snap.enable_tun_mode);
            self.0.lock().unwrap().push("clash");
            Ok(Box::new(NoopPreparedLegacyMirror))
        }
        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    #[tokio::test]
    async fn typed_owner_composition_reads_persisted_state_and_projects_in_order() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        paths.create_base_dirs().unwrap();
        let app = ChimeraAppConfig {
            enable_local_ip_probe: true,
            ..Default::default()
        };
        let clash = ClashConfig {
            enable_tun_mode: true,
            ..Default::default()
        };
        std::fs::write(
            paths.application_config_path(),
            serde_yaml_ng::to_string(&app).unwrap(),
        )
        .unwrap();
        std::fs::write(
            paths.clash_config_path(),
            serde_yaml_ng::to_string(&clash).unwrap(),
        )
        .unwrap();
        let bridge = Arc::new(RecordingBridges::default());
        let shutdown = CancellationToken::new();
        let clients = new_typed_config_clients(
            &paths,
            &LegacyBridgeSet {
                verge: bridge.clone(),
                window: bridge.clone(),
                clash: bridge.clone(),
            },
            ReleaseChannel::Stable,
            MutationCoordinator::pending(),
            shutdown.clone(),
        )
        .await
        .unwrap();
        assert!(clients.application.get_typed().enable_local_ip_probe);
        assert!(clients.clash_config.get_typed().enable_tun_mode);
        assert_eq!(
            *bridge.0.lock().unwrap(),
            ["application", "session", "clash"]
        );
        shutdown.cancel();
    }
}
