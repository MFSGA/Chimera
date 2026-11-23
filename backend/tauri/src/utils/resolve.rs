use std::{
    collections::HashSet,
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use chimera_config::application::{ChimeraAppConfig, WindowType as AppWindowType};
use tauri::{App, AppHandle, Emitter, Listener, Manager};
use tracing::debug;

use crate::{
    client::application_workflow::startup::StartupOutcome,
    config::chimera::ClashCore,
    core::{handle, tray},
    log_err,
    utils::init,
    window::{
        WindowManager,
        kinds::{EditorWindow, EditorWindowType, LegacyWindow, MainWindow},
    },
};

static FRONTEND_READY_WINDOWS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

/// A GUI-only indicator for the initial startup orchestration, not an
/// alternative core lifecycle authority. It is never used to control the core.
#[derive(Default)]
pub struct AppStartupStatus(AtomicU8);

impl AppStartupStatus {
    pub fn phase(&self) -> &'static str {
        match self.0.load(Ordering::Acquire) {
            1 => "ready",
            2 => "failed",
            _ => "starting",
        }
    }

    fn finish(&self, succeeded: bool) {
        self.0
            .store(if succeeded { 1 } else { 2 }, Ordering::Release);
    }
}

fn frontend_ready_windows() -> &'static RwLock<HashSet<String>> {
    FRONTEND_READY_WINDOWS.get_or_init(|| RwLock::new(HashSet::new()))
}

fn is_primary_window_label(label: &str) -> bool {
    label == crate::consts::LEGACY_WINDOW_LABEL || label == crate::consts::MAIN_WINDOW_LABEL
}

pub(crate) fn mark_frontend_mounted(label: &str) {
    if is_primary_window_label(label) {
        frontend_ready_windows()
            .write()
            .unwrap()
            .insert(label.to_string());
    }
}

/// Open either retained UI through the shared reference-style manager.
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_legacy_window(app_handle: &AppHandle) {
    log_err!(
        app_handle
            .state::<WindowManager>()
            .open(&LegacyWindow, None)
    );
}

#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_main_window(app_handle: &AppHandle) {
    log_err!(app_handle.state::<WindowManager>().open(&MainWindow, None));
}

pub fn create_profile_editor_window(app_handle: &AppHandle, uid: &str) -> Result<()> {
    let (window, params) = EditorWindow::for_type(EditorWindowType::Profile, Some(uid))?;
    app_handle.state::<WindowManager>().open(&window, params)?;
    Ok(())
}

pub fn create_css_editor_window(app_handle: &AppHandle) -> Result<()> {
    let (window, params) = EditorWindow::for_type(EditorWindowType::CssEditor, None)?;
    app_handle.state::<WindowManager>().open(&window, params)?;
    Ok(())
}

pub fn mark_frontend_unmounted(label: &str) {
    frontend_ready_windows().write().unwrap().remove(label);
}

pub fn wait_for_frontend_ready(label: &str, timeout: Duration) -> bool {
    let start_at = Instant::now();

    while !frontend_ready_windows().read().unwrap().contains(label) {
        if start_at.elapsed() >= timeout {
            return false;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    true
}

fn app_config_for_window(app_handle: &AppHandle) -> ChimeraAppConfig {
    app_handle
        .state::<crate::client::ChimeraClient>()
        .get_app_config()
        .unwrap_or_else(|error| {
            log::error!(target: "app", "failed to read startup window settings: {error:#}");
            ChimeraAppConfig::default()
        })
}

fn window_label(window_type: AppWindowType) -> &'static str {
    match window_type {
        AppWindowType::Legacy => crate::consts::LEGACY_WINDOW_LABEL,
        AppWindowType::Main => crate::consts::MAIN_WINDOW_LABEL,
    }
}

fn initial_window_type(config: &ChimeraAppConfig) -> Option<AppWindowType> {
    (!config.enable_silent_start).then_some(config.window_type)
}

pub fn configured_window_label(app_handle: &AppHandle) -> &'static str {
    window_label(app_config_for_window(app_handle).window_type)
}

fn create_window_for_type(app_handle: &AppHandle, window_type: AppWindowType) {
    match window_type {
        AppWindowType::Legacy => create_legacy_window(app_handle),
        AppWindowType::Main => create_main_window(app_handle),
    }
}

#[cfg(test)]
mod startup_window_config_tests {
    use super::{initial_window_type, window_label};
    use chimera_config::application::{ChimeraAppConfig, WindowType};

    #[test]
    fn startup_window_selection_uses_the_typed_window_type() {
        let main_config = ChimeraAppConfig {
            window_type: WindowType::Main,
            ..ChimeraAppConfig::default()
        };
        assert_eq!(initial_window_type(&main_config), Some(WindowType::Main));
        assert_eq!(
            window_label(WindowType::Main),
            crate::consts::MAIN_WINDOW_LABEL
        );

        let legacy_config = ChimeraAppConfig {
            window_type: WindowType::Legacy,
            ..ChimeraAppConfig::default()
        };
        assert_eq!(
            initial_window_type(&legacy_config),
            Some(WindowType::Legacy)
        );
        assert_eq!(
            window_label(WindowType::Legacy),
            crate::consts::LEGACY_WINDOW_LABEL
        );
    }

    #[test]
    fn silent_start_suppresses_initial_window_selection() {
        let config = ChimeraAppConfig {
            enable_silent_start: true,
            ..ChimeraAppConfig::default()
        };

        assert_eq!(initial_window_type(&config), None);
        assert_eq!(
            window_label(config.window_type),
            crate::consts::LEGACY_WINDOW_LABEL
        );
    }
}

/// Create window based on window_type config
/// This is the primary function to use when opening window from tray, etc.
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_window(app_handle: &AppHandle) {
    create_window_for_type(app_handle, app_config_for_window(app_handle).window_type);
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);

    handle::Handle::global().init(app.app_handle().clone());
    debug!("todo init handle for widget not tray");
    crate::consts::setup_app_handle(app.app_handle().clone());

    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    {
        let app_handle = app.app_handle().clone();
        app.listen("update_systray", move |_| {
            let app_handle_clone = app_handle.clone();
            log_err!(app_handle.run_on_main_thread(move || {
                log_err!(
                    tray::Tray::update_systray(&app_handle_clone),
                    "failed to update systray"
                );
            }));
        });
    }

    let app_shutdown = app.state::<crate::setup::StartupOwners>().clone();
    crate::core::storage::register_web_storage_listener(
        app.app_handle(),
        app_shutdown.shutdown.child_token(),
        app_shutdown.tasks.clone(),
    );

    // Match ref/backend/tauri/src/utils/resolve.rs: finish the initial
    // service/runtime reconciliation before opening the first window. The
    // Service compatibility check is Chimera-specific, but it precedes the
    // same one-shot startup reconciliation boundary.
    let client = (*app.state::<crate::client::ChimeraClient>()).clone();
    let report = tracing::info_span!("startup_reconcile").in_scope(|| {
        tauri::async_runtime::block_on(async {
            if let Err(error) = init::init_service(client.clone()).await {
                tracing::warn!(%error, "Service startup preparation failed");
            }
            log_err!(crate::client::ports::resolve_random_mixed_port(&client).await);
            client.startup_reconcile().await
        })
    });
    if let Some(observation) = &report.observation {
        tracing::info!(
            operation_id = report.operation_id,
            desired = ?observation.desired,
            service = ?observation.service,
            runtime = ?observation.runtime,
            "startup reconciliation observation"
        );
    }
    match &report.outcome {
        StartupOutcome::Ready => {
            app.state::<AppStartupStatus>().finish(true);
            tracing::info!(
                operation_id = report.operation_id,
                "startup reconciliation ready"
            );
        }
        outcome => {
            app.state::<AppStartupStatus>().finish(false);
            tracing::warn!(
                operation_id = report.operation_id,
                ?outcome,
                "startup reconciliation did not reach ready"
            );
        }
    }
    // As in ref, a non-ready startup does not block background producers:
    // any changes are serialized by the application workflow actor.
    log_err!(tauri::async_runtime::block_on(
        client.start_background_sources()
    ));

    // As in ref's setup_proxies, refresh the proxy view after the startup
    // conclusion. The proxy owner retries if the core API is unavailable.
    client.request_proxy_refresh();
    #[cfg(any(windows, target_os = "linux", target_os = "macos"))]
    log_err!(app.emit("update_systray", ()));

    // Keep Chimera's additional Legacy UI selection at the presentation
    // boundary; the shared startup lifecycle above is ref-first.
    let app_config = app_config_for_window(app.app_handle());
    if let Some(window_type) = initial_window_type(&app_config) {
        create_window_for_type(app.app_handle(), window_type);
    }
}

/// Resolve a sidecar version through the same typed OS reader as ref.
/// The legacy UI's historical `v` prefix remains only at this presentation boundary.
pub async fn resolve_core_version(
    _app_handle: &AppHandle,
    core_type: &ClashCore,
) -> Result<String> {
    use chimera_contracts::core_version::CoreVersionReader as _;

    let reader = chimera_os::core_version::SidecarCoreVersionReader::new(std::sync::Arc::new(
        crate::adapters::core_version::TauriCoreVersionExecutableSource,
    ));
    let version = reader.read(*core_type).await?;
    Ok(format_core_version(core_type, &version))
}

fn format_core_version(core_type: &ClashCore, version: &str) -> String {
    match core_type {
        ClashCore::ClashRs | ClashCore::ChimeraClient => format!("v{version}"),
        _ => version.to_owned(),
    }
}

#[cfg(test)]
mod core_version_tests {
    use super::format_core_version;
    use crate::config::chimera::ClashCore;

    #[test]
    fn alpha_version_matches_its_manifest_without_a_false_update_badge() {
        let version = "0.26.3-alpha+sha.229218a";

        assert_eq!(
            format_core_version(&ClashCore::ChimeraClientAlpha, version),
            version
        );
        assert_eq!(
            format_core_version(&ClashCore::ChimeraClient, "0.26.3"),
            "v0.26.3"
        );
    }
}

#[cfg(test)]
mod frontend_ready_tests {
    use super::*;

    #[test]
    fn app_startup_phase_tracks_initial_task_completion() {
        let startup = AppStartupStatus::default();
        assert_eq!(startup.phase(), "starting");
        startup.finish(true);
        assert_eq!(startup.phase(), "ready");
        startup.finish(false);
        assert_eq!(startup.phase(), "failed");
    }

    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    #[test]
    fn primary_frontend_ready_survives_other_primary_window_destroy() {
        let _guard = test_lock();
        frontend_ready_windows().write().unwrap().clear();

        mark_frontend_mounted(crate::consts::MAIN_WINDOW_LABEL);
        mark_frontend_mounted(crate::consts::LEGACY_WINDOW_LABEL);
        assert!(wait_for_frontend_ready(
            crate::consts::MAIN_WINDOW_LABEL,
            Duration::ZERO
        ));

        mark_frontend_unmounted(crate::consts::LEGACY_WINDOW_LABEL);
        assert!(wait_for_frontend_ready(
            crate::consts::MAIN_WINDOW_LABEL,
            Duration::ZERO
        ));

        mark_frontend_unmounted(crate::consts::MAIN_WINDOW_LABEL);
        assert!(!wait_for_frontend_ready(
            crate::consts::MAIN_WINDOW_LABEL,
            Duration::ZERO
        ));
    }

    #[test]
    fn editor_frontend_does_not_mark_primary_frontend_ready() {
        let _guard = test_lock();
        frontend_ready_windows().write().unwrap().clear();

        mark_frontend_mounted("editor-css");
        mark_frontend_mounted("profile-editor-example");

        assert!(!wait_for_frontend_ready(
            crate::consts::MAIN_WINDOW_LABEL,
            Duration::ZERO
        ));
    }

    #[test]
    fn waiting_for_main_does_not_accept_ready_legacy_window() {
        let _guard = test_lock();
        frontend_ready_windows().write().unwrap().clear();

        mark_frontend_mounted(crate::consts::LEGACY_WINDOW_LABEL);

        assert!(!wait_for_frontend_ready(
            crate::consts::MAIN_WINDOW_LABEL,
            Duration::ZERO
        ));
        assert!(wait_for_frontend_ready(
            crate::consts::LEGACY_WINDOW_LABEL,
            Duration::ZERO
        ));
    }
}
