//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use chimera_contracts::net::SelfProxyPortSource as _;
use chimera_os::system_dns::OsSystemDnsCache;
use chimera_traffic::{RedbTrafficStore, TrafficStore};
use chimera_utils::process::{AtomicStopStrategy, StopStrategy};
use tauri::Manager;

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{ChimeraClient, ClientSetupArgs, LegacyBridgeSet, RuntimePaths, TauriUiEventSink},
    utils::{init, path::PathResolver, task::spawn_tracked_on_tauri_runtime},
};

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::client::hotkey::{
    HotkeyArgs, HotkeyClient,
    adapters::{
        ChannelActionSink, PlatformAcceleratorValidator, TauriMainThread, TauriShortcutRegistrar,
        TauriWindowControl,
    },
    main_thread::{MainThreadExecutor, MainThreadHandoff},
    ports::{HotkeyAction, WindowControl},
};

struct LegacySelfProxyPort;

impl chimera_contracts::net::SelfProxyPortSource for LegacySelfProxyPort {
    fn mixed_port(&self) -> Option<u16> {
        Some(
            crate::config::core::Config::clash()
                .latest()
                .get_mixed_port(),
        )
    }
}

pub fn setup(
    app: &mut tauri::App,
    paths: PathResolver,
    resolved_paths: chimera_paths::PathResolver,
    bundle_metadata: chimera_contracts::bundle_metadata::BundleMetadata,
    startup_owners: &mut StartupOwners,
    startup_failure_capture: &parking_lot::Mutex<Option<crate::startup_failure::Report>>,
) -> anyhow::Result<()> {
    let shutdown = startup_owners.shutdown.clone();
    let tasks = startup_owners.tasks.clone();
    let system_proxy_source: Arc<dyn chimera_contracts::net::SystemProxySource> =
        Arc::new(chimera_os::system_proxy_source::OsSystemProxySource);
    let profile_service = Arc::new(
        crate::service::profile_file::ProfileFileService::new_with_system_proxy(
            paths.clone(),
            Arc::new(LegacySelfProxyPort),
            system_proxy_source.clone(),
            bundle_metadata,
            crate::service::profile_file::subscription_device_headers(),
        ),
    );

    let runtime_paths = RuntimePaths::from_resolver(&paths)?;
    let mut migrations = crate::startup_failure::report_boundary_with_backup_and_capture(
        crate::core::migration::Runner::with_paths(paths.clone(), false)
            .context("failed to setup config migrations"),
        crate::startup_failure::StartupStage::MigrationPrepare,
        &paths,
        None,
        Some(startup_failure_capture),
    )?;
    let migration_result = migrations
        .run_pending()
        .context("failed to run config migrations before client setup");
    crate::startup_failure::report_boundary_with_backup_and_capture(
        migration_result,
        crate::startup_failure::StartupStage::MigrationRun,
        &paths,
        migrations.backup_info(),
        Some(startup_failure_capture),
    )?;

    // ref initializes bundled resources before constructing runtime owners.
    let resources_dir = init::bundled_resources_dir(app)
        .inspect_err(|error| tracing::error!(%error, "failed to locate the bundled resources"))
        .ok();
    crate::log_err!(init::init_resources(
        paths.app_data_dir(),
        resources_dir.as_deref()
    ));

    let traffic_store = open_traffic_store(&paths);
    let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
    let bridges = LegacyBridgeSet {
        verge: Arc::new(LegacyVergeBridge::new(legacy_lock.clone())),
        window: Arc::new(LegacyWindowBridge::new(legacy_lock.clone())),
        clash: Arc::new(LegacyClashBridge::new(legacy_lock)),
    };
    app.manage(tokio::sync::RwLock::new(
        crate::core::updater::UpdaterManager::new(),
    ));
    let stop_strategy = Arc::new(AtomicStopStrategy::new(StopStrategy::default()));
    app.manage(Arc::clone(&stop_strategy));
    // The desktop executable handles windows_shutdown_helper_entry in main.rs.
    let local_control = crate::startup_failure::report_boundary_with_backup_and_capture(
        (|| {
            let shutdown_helper =
                std::env::current_exe().context("failed to resolve the desktop shutdown helper")?;
            tauri::async_runtime::block_on(crate::core::actor_v2::local_host::build(
                &paths,
                &shutdown_helper,
                stop_strategy,
            ))
            .context("failed to initialize the local core control plane")
        })(),
        crate::startup_failure::StartupStage::CoreHost,
        &paths,
        migrations.backup_info(),
        Some(startup_failure_capture),
    )?;
    startup_owners.core = Some(local_control.clone());
    // Match ref/backend/tauri/src/setup.rs: inject the original resolver from
    // the process entry point. Never re-discover paths through Tauri globals.
    let service_binary =
        chimera_os::service::service_binary(resolved_paths.app_install_dir()?.as_std_path());
    let os_service_process = Arc::new(chimera_os::service::OsServiceProcessControl::new(
        service_binary,
        resolved_paths,
        Arc::new(crate::adapters::elevation::InteractiveElevationConsent),
    ));
    // Only the legacy IPC health-check notification remains a Tauri adapter.
    let service_process: Arc<dyn chimera_contracts::service::ServiceProcessControl> =
        Arc::new(crate::core::service::control::HealthMonitorStopAdapter::new(os_service_process));
    let core_facade = Arc::new(crate::core::actor_v2::CoreFacade::new_local_with_control(
        local_control,
        runtime_paths.clone(),
        service_process,
        shutdown.child_token(),
        &tasks,
    ));
    let core_facade_monitor = core_facade.clone();
    let core_bridge: Arc<dyn crate::client::core_lifecycle::CoreLifecyclePort> = Arc::new(
        crate::client::core_lifecycle::LegacyCoreBridge::new(core_facade.clone()),
    );
    // Mirror ref's runtime-owner order: construct the local Core and Service
    // control plane before starting the Jobs owner. A failure below leaves
    // the partial Core in StartupOwners for the exit boundary to drain.
    let jobs = crate::startup_failure::report_boundary_with_backup_and_capture(
        tauri::async_runtime::block_on(crate::client::jobs::start(
            paths.jobs_path(),
            init::jobs_capture(),
            shutdown.child_token(),
            &tasks,
        ))
        .context("failed to start the jobs owner"),
        crate::startup_failure::StartupStage::Jobs,
        &paths,
        migrations.backup_info(),
        Some(startup_failure_capture),
    )?;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let main_thread_handoff = Arc::new(MainThreadHandoff::default());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    app.manage(Arc::clone(&main_thread_handoff));
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let main_thread: Arc<dyn MainThreadExecutor> = Arc::new(TauriMainThread::new(
        app.handle().clone(),
        main_thread_handoff,
    ));
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let hotkeys = tauri::async_runtime::block_on(HotkeyClient::spawn(HotkeyArgs {
        registrar: Arc::new(TauriShortcutRegistrar::new(
            app.handle().clone(),
            main_thread,
        )),
        sink: Arc::new(ChannelActionSink::new(hotkey_tx)),
    }))
    .context("failed to start the global shortcut owner")?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let accelerators = Arc::new(PlatformAcceleratorValidator);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let appimage = {
        #[cfg(target_os = "linux")]
        {
            app.handle()
                .env()
                .appimage
                .and_then(|path| path.into_string().ok())
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let auto_launch = tauri::utils::platform::current_exe()
        .map_err(
            |source| chimera_contracts::system_proxy::AutoLaunchError::LocateExecutable { source },
        )
        .and_then(|exe| chimera_os::autostart::AutoLaunchConfig::resolve(&exe, appimage))
        .and_then(chimera_os::autostart::AutoLaunchBackend::new)
        .context("failed to resolve the auto-launch registration")?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let pac = {
        let cache_path = camino::Utf8PathBuf::from_path_buf(paths.cache_dir().join("pac.js"))
            .map_err(|path| anyhow::anyhow!("PAC cache path is not UTF-8: {}", path.display()))?;
        chimera_os::pac::HttpPacBackend::new(cache_path)
            .context("failed to build the PAC backend")?
    };
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let system_proxy =
        tauri::async_runtime::block_on(crate::client::system_proxy::SystemProxyClient::spawn(
            crate::client::system_proxy::SystemProxyArgs {
                os: Arc::new(chimera_os::system_proxy::SysproxyOsProxy),
                auto_launch: Arc::new(auto_launch),
                pac: Arc::new(pac),
                schedule_guard_ticks: true,
                shutdown: shutdown.child_token(),
            },
            &tasks,
        ))
        .context("failed to start the system proxy owner")?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let effect_adapters = Arc::new(
        crate::client::effects::adapters::TauriApplicationEffectAdapters::new(app.handle().clone()),
    );
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let effects = Arc::new(
        crate::client::effects::executor::ApplicationEffectExecutor::new(
            system_proxy.clone(),
            hotkeys,
            accelerators.clone(),
            effect_adapters,
            core_bridge.clone(),
        ),
    );
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let window: Arc<dyn WindowControl> = Arc::new(TauriWindowControl::new(app.handle().clone()));

    let update_proxy_port = Arc::new(LegacySelfProxyPort);
    let app_update_backend: Arc<dyn chimera_core::client::app_update::AppUpdateBackend> =
        Arc::new(crate::adapters::app_update::TauriAppUpdateBackend::new(
            app.handle().clone(),
            update_proxy_port.clone(),
            bundle_metadata,
            system_proxy_source.clone(),
        ));
    let app_update_events: Arc<dyn chimera_core::client::app_update::AppUpdateEventSink> = Arc::new(
        crate::adapters::app_update::TauriAppUpdateEventSink::new(app.handle().clone()),
    );
    let changelog_http: Arc<dyn chimera_contracts::net::ChangelogHttp> =
        Arc::new(chimera_net::changelog_http::ReqwestChangelogHttp::new(
            update_proxy_port.clone(),
            bundle_metadata,
            system_proxy_source.clone(),
        ));

    crate::startup_failure::report_boundary_with_backup_and_capture(
        crate::core::storage::setup(app, &paths),
        crate::startup_failure::StartupStage::Storage,
        &paths,
        migrations.backup_info(),
        Some(startup_failure_capture),
    )?;

    let client = crate::startup_failure::report_boundary_with_backup_and_capture(
        ChimeraClient::try_new_with_args(ClientSetupArgs {
            paths: paths.clone(),
            runtime_paths: runtime_paths.clone(),
            bundle_metadata,
            app_update_backend,
            app_update_events,
            changelog_http,
            bridges,
            core: core_bridge,
            service: Arc::new(crate::client::core_lifecycle::LegacyServiceBridge::new(
                core_facade,
            )),
            profile_service,
            system_dns: Arc::new(OsSystemDnsCache::new(Arc::new(
                crate::adapters::elevation::InteractiveElevationConsent,
            ))),
            traffic_store,
            jobs,
            shutdown: shutdown.child_token(),
            tasks: tasks.clone(),
            ui_sink: Arc::new(TauriUiEventSink::new(app.handle().clone())),
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            effects,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            system_proxy,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            accelerators,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            window,
        }),
        crate::startup_failure::StartupStage::TypedClient,
        &paths,
        migrations.backup_info(),
        Some(startup_failure_capture),
    )?;
    rust_i18n::set_locale(client.get_app_config()?.language.as_str());
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    spawn_tracked_on_tauri_runtime(
        &tasks,
        hotkey_action_pump(hotkey_rx, client.clone(), shutdown.child_token()),
    );
    app.manage(client);
    startup_owners.core = None;
    core_facade_monitor.start_service_api_monitor(shutdown.child_token(), &tasks);
    Ok(())
}

#[derive(Clone, Default)]
pub(crate) struct StartupOwners {
    pub(crate) shutdown: tokio_util::sync::CancellationToken,
    pub(crate) tasks: tokio_util::task::TaskTracker,
    core: Option<chimera_core_manager::CoreControl>,
}

/// Keeps startup failures out of Tauri's setup error wrapper, then transfers
/// partial owners to the normal event-loop shutdown path.
pub(crate) fn run_setup(
    mut owners: StartupOwners,
    setup: impl FnOnce(&mut StartupOwners) -> anyhow::Result<()>,
    report_failure: impl FnOnce(&anyhow::Error) -> crate::startup_failure::Report,
    host: &impl StartupFailureHost,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(error) = setup(&mut owners) {
        let report = report_failure(&error);
        tracing::error!(
            stage = ?report.failure.stage,
            kind = ?report.failure.kind,
            path = ?report.failure.path,
            detail = %report.failure.detail,
            "application startup failed"
        );
        // Match ref: retain the partially composed owners first. The exit
        // boundary selects and cancels them while the event loop can still
        // service their cleanup work.
        host.show(report);
        host.retain(owners);
        host.exit(1);
    }
    Ok(())
}

pub(crate) trait StartupFailureHost {
    fn show(&self, report: crate::startup_failure::Report);
    fn retain(&self, owners: StartupOwners);
    fn exit(&self, code: i32);
}

pub(crate) struct TauriStartupFailureHost {
    pub handle: tauri::AppHandle,
}

impl StartupFailureHost for TauriStartupFailureHost {
    fn show(&self, report: crate::startup_failure::Report) {
        crate::startup_failure::show_report(&report);
    }

    fn retain(&self, owners: StartupOwners) {
        use tauri::Manager as _;
        if self.handle.try_state::<StartupOwners>().is_none() {
            self.handle.manage(owners);
        }
    }

    fn exit(&self, code: i32) {
        self.handle.exit(code);
    }
}

impl StartupOwners {
    pub(crate) fn request_shutdown(&self) {
        self.shutdown.cancel();
        self.tasks.close();
    }

    pub(crate) async fn wait_shutdown(&self) {
        self.tasks.wait().await;
        if let Some(core) = &self.core {
            if let Err(error) = core.shutdown().await {
                tracing::error!(%error, "failed to shut down the partially initialized core host");
            }
            if core.until_closed().await == chimera_core_manager::ExecutorExit::Died {
                tracing::error!("partially initialized core host executor exited unexpectedly");
            }
        }
    }
}

/// Traffic recording is optional; an unavailable store disables history without blocking launch.
fn open_traffic_store(paths: &PathResolver) -> Option<Arc<dyn TrafficStore>> {
    let dir = paths.app_data_dir().join("traffic");
    let open = || -> anyhow::Result<RedbTrafficStore> {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
        Ok(RedbTrafficStore::open(&dir.join("traffic.redb"))?)
    };
    match open() {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::error!(%error, "failed to open the traffic store; recording is disabled");
            None
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn hotkey_action_pump(
    mut actions: tokio::sync::mpsc::UnboundedReceiver<HotkeyAction>,
    client: ChimeraClient,
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        let action = tokio::select! {
            _ = shutdown.cancelled() => break,
            action = actions.recv() => match action {
                Some(action) => action,
                None => break,
            },
        };
        if let Err(error) = client.dispatch_hotkey_action(action).await {
            tracing::warn!(%error, %action, "hotkey action failed");
        }
    }
}
