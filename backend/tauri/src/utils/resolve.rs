use std::{
    collections::HashSet,
    sync::{OnceLock, RwLock},
    time::{Duration, Instant},
};

use anyhow::Result;
use semver::Version;
use tauri::{App, AppHandle, Emitter, Listener, Manager};
use tauri_plugin_shell::ShellExt;
use tracing::debug;

use crate::{
    config::{
        chimera::{ClashCore, WindowState, WindowType},
        core::Config,
    },
    core::{clash::core::CoreManager, handle, sysopt, tray},
    log_err,
    utils::init,
    window::{AppWindow, WindowConfig},
};

/// Legacy window implementation (original UI)
struct LegacyWindow;

static FRONTEND_READY_WINDOWS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

fn frontend_ready_windows() -> &'static RwLock<HashSet<String>> {
    FRONTEND_READY_WINDOWS.get_or_init(|| RwLock::new(HashSet::new()))
}

fn is_primary_window_label(label: &str) -> bool {
    label == crate::consts::LEGACY_WINDOW_LABEL || label == crate::consts::MAIN_WINDOW_LABEL
}

fn mark_frontend_mounted(label: &str) {
    if is_primary_window_label(label) {
        frontend_ready_windows()
            .write()
            .unwrap()
            .insert(label.to_string());
    }
}

impl AppWindow for LegacyWindow {
    fn label(&self) -> &str {
        crate::consts::LEGACY_WINDOW_LABEL
    }

    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }

    fn url(&self) -> &str {
        "/"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        Config::verge().latest().window_size_state.clone()
    }
}

/// Main window implementation (new UI)
struct MainWindow;

struct ProfileEditorWindow {
    label: String,
    url: String,
}

struct CssEditorWindow;

impl AppWindow for ProfileEditorWindow {
    fn label(&self) -> &str {
        &self.label
    }

    fn title(&self) -> &str {
        "Profile Editor"
    }

    fn url(&self) -> &str {
        &self.url
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .visible_on_create(true)
            .default_size(960.0, 680.0)
            .min_size(600.0, 400.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        None
    }
}

impl AppWindow for CssEditorWindow {
    fn label(&self) -> &str {
        "editor-css"
    }

    fn title(&self) -> &str {
        "Clash Chimera - CSS Editor"
    }

    fn url(&self) -> &str {
        "/editor/css"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 500.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        None
    }
}

impl AppWindow for MainWindow {
    fn label(&self) -> &str {
        crate::consts::MAIN_WINDOW_LABEL
    }

    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }

    fn url(&self) -> &str {
        "/main"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .visible_on_create(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }

    fn get_window_state(&self) -> Option<WindowState> {
        Config::verge().latest().window_size_state.clone()
    }
}

/// create legacy window
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_legacy_window(app_handle: &AppHandle) {
    log_err!(LegacyWindow.create(app_handle));
}

/// create main window
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_main_window(app_handle: &AppHandle) {
    log_err!(MainWindow.create(app_handle));
}

pub fn create_profile_editor_window(app_handle: &AppHandle, uid: &str) -> Result<()> {
    let encoded_uid: String = url::form_urlencoded::byte_serialize(uid.as_bytes()).collect();
    ProfileEditorWindow {
        label: format!("profile-editor-{uid}"),
        url: format!("/editor/profile?uid={encoded_uid}"),
    }
    .create(app_handle)
}

pub fn create_css_editor_window(app_handle: &AppHandle) -> Result<()> {
    CssEditorWindow.create(app_handle)
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

pub fn configured_window_label() -> &'static str {
    match Config::verge()
        .latest()
        .window_type
        .unwrap_or(WindowType::Main)
    {
        WindowType::Legacy => crate::consts::LEGACY_WINDOW_LABEL,
        WindowType::Main => crate::consts::MAIN_WINDOW_LABEL,
    }
}

/// Create window based on window_type config
/// This is the primary function to use when opening window from tray, etc.
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_window(app_handle: &AppHandle) {
    match configured_window_label() {
        crate::consts::LEGACY_WINDOW_LABEL => create_legacy_window(app_handle),
        _ => create_main_window(app_handle),
    }
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    app.listen("react_app_mounted", move |event| {
        let Ok(payload) = serde_json::from_str::<serde_json::Value>(event.payload()) else {
            tracing::warn!("invalid react_app_mounted payload: {}", event.payload());
            return;
        };
        let Some(label) = payload.get("label").and_then(serde_json::Value::as_str) else {
            tracing::warn!(
                "react_app_mounted payload missing label: {}",
                event.payload()
            );
            return;
        };
        tracing::debug!("frontend react app mounted: {label}");
        mark_frontend_mounted(label);
    });

    handle::Handle::global().init(app.app_handle().clone());
    debug!("todo init handle for widget not tray");
    crate::consts::setup_app_handle(app.app_handle().clone());

    log_err!(init::init_resources());

    let client = app.state::<crate::client::ChimeraClient>();
    log_err!(init::init_service((*client).clone()));
    log_err!(crate::client::ports::resolve_random_mixed_port(&client));

    // 启动核心
    log::trace!("init config");
    log_err!(Config::init_config());

    log::trace!("launch core");
    log_err!(CoreManager::global().init());

    log::trace!("init storage");
    log_err!(crate::core::storage::setup(app));

    // setup jobs
    /* log::trace!("setup jobs");
    {
        let storage = app.state::<Storage>();
        let storage = (*storage).clone();
        log_err!(crate::core::tasks::setup(app, storage));
    } */

    log_err!(sysopt::Sysopt::global().init_launch());
    log_err!(sysopt::Sysopt::global().init_sysproxy());

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
        log_err!(app.emit("update_systray", ()));
    }

    let silent_start = Config::verge()
        .latest()
        .enable_silent_start
        .unwrap_or(false);
    if !silent_start {
        create_window(app.app_handle());
    }

    crate::core::storage::register_web_storage_listener(app.app_handle());
}

/// reset runtime side effects before shutdown
pub fn resolve_reset() {
    log_err!(sysopt::Sysopt::global().reset_sysproxy());
}

/// resolve core version
pub async fn resolve_core_version(app_handle: &AppHandle, core_type: &ClashCore) -> Result<String> {
    let shell = app_handle.shell();
    let core = core_type.clone().to_string();
    // execute the command
    let cmd = match core_type {
        ClashCore::ClashPremium | ClashCore::Mihomo | ClashCore::MihomoAlpha => {
            shell.sidecar(core)?.args(["-v"])
        }
        ClashCore::ClashRs | ClashCore::ClashRsAlpha | ClashCore::ChimeraClient => {
            shell.sidecar(core)?.args(["-V"])
        }
    };
    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("failed to get core version"));
    }
    let out = String::from_utf8_lossy(&out.stdout);
    log::trace!(target: "app", "get core version: {out:?}");
    let out = out.trim().split(' ').collect::<Vec<&str>>();
    for item in out {
        log::debug!(target: "app", "check item: {item}");
        if item.starts_with('v')
            || item.starts_with('n')
            || item.starts_with("alpha")
            || Version::parse(item).is_ok()
        {
            match core_type {
                ClashCore::ClashRs | ClashCore::ChimeraClient => return Ok(format!("v{}", item)),
                _ => return Ok(item.to_string()),
            }
        }
    }
    Err(anyhow::anyhow!("failed to get core version"))
}

#[cfg(test)]
mod frontend_ready_tests {
    use super::*;
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
