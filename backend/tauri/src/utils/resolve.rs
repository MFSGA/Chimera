use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::Result;
use semver::Version;
use tauri::{App, AppHandle, Emitter, Listener, Manager};
use tauri_plugin_shell::ShellExt;
use tracing::debug;

use crate::{
    config::{
        chimera::{ClashCore, IVerge, WindowState},
        core::Config,
    },
    core::{clash::core::CoreManager, handle, sysopt, tray},
    log_err,
    utils::init,
    window::{AppWindow, WindowConfig},
};

/// Legacy window implementation (original UI)
struct LegacyWindow;

static FRONTEND_READY: AtomicBool = AtomicBool::new(false);

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

    fn set_window_state(&self, state: Option<WindowState>) {
        Config::verge().data().patch_config(IVerge {
            window_size_state: state,
            ..IVerge::default()
        });
    }
}

/// create legacy window
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_legacy_window(app_handle: &AppHandle) {
    log_err!(LegacyWindow.create(app_handle));
}

pub fn mark_frontend_unmounted() {
    FRONTEND_READY.store(false, Ordering::Release);
}

pub fn wait_for_frontend_ready(timeout: Duration) -> bool {
    let start_at = Instant::now();

    while !FRONTEND_READY.load(Ordering::Acquire) {
        if start_at.elapsed() >= timeout {
            return false;
        }

        std::thread::sleep(Duration::from_millis(100));
    }

    true
}

/// Create window based on use_legacy_ui config
/// This is the primary function to use when opening window from tray, etc.
#[tracing_attributes::instrument(skip(app_handle))]
pub fn create_window(app_handle: &AppHandle) {
    let use_legacy = Config::verge().latest().use_legacy_ui.unwrap_or(true);

    if use_legacy {
        create_legacy_window(app_handle);
    } else {
        todo!()
        // create_main_window(app_handle);
    }
}

pub fn save_legacy_window_state(app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
    LegacyWindow.save_state(app_handle, save_to_file)
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    app.listen("react_app_mounted", move |_| {
        tracing::debug!("frontend react app mounted");
        FRONTEND_READY.store(true, Ordering::Release);
    });

    handle::Handle::global().init(app.app_handle().clone());
    debug!("todo init handle for widget not tray");
    // crate::consts::setup_app_handle(app.app_handle().clone());

    log_err!(init::init_resources());
    log_err!(init::init_service());

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

    create_window(app.app_handle());

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
