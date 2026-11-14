use std::{
    collections::HashSet,
    sync::{
        OnceLock, RwLock,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Result;
use tauri::{App, AppHandle, Emitter, Listener, Manager};
use tracing::debug;

use crate::{
    config::{
        chimera::{ClashCore, WindowType},
        core::Config,
    },
    core::{handle, tray},
    log_err,
    utils::init,
    window::{
        WindowManager,
        kinds::{EditorWindow, EditorWindowType, LegacyWindow, MainWindow},
    },
};

static FRONTEND_READY_WINDOWS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

fn spawn_tracked_on_tauri_runtime(
    tasks: &tokio_util::task::TaskTracker,
    task: impl std::future::Future<Output = ()> + Send + 'static,
) {
    tasks.spawn_on(task, tauri::async_runtime::handle().inner());
}

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

/// Perform startup's blocking Service/port preparation without allowing the
/// blocking worker to outlive its tracked owner. Once cancellation is
/// requested, no core reconciliation is started from the prepared result.
async fn prepare_startup_while_active<T, F>(
    shutdown: tokio_util::sync::CancellationToken,
    prepare: F,
) -> std::result::Result<Option<T>, tauri::Error>
where
    T: Send + 'static,
    F: FnOnce(tokio_util::sync::CancellationToken) -> T + Send + 'static,
{
    if shutdown.is_cancelled() {
        return Ok(None);
    }

    let worker_shutdown = shutdown.clone();
    let result = tauri::async_runtime::spawn_blocking(move || prepare(worker_shutdown)).await?;
    // The worker is fully joined before this task can finish, even when
    // shutdown was requested during a slow or blocking configuration write.
    Ok((!shutdown.is_cancelled()).then_some(result))
}

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    handle::Handle::global().init(app.app_handle().clone());
    debug!("todo init handle for widget not tray");
    crate::consts::setup_app_handle(app.app_handle().clone());

    log_err!(init::init_resources());

    log::trace!("init storage");
    log_err!(crate::core::storage::setup(app));

    // setup jobs
    /* log::trace!("setup jobs");
    {
        let storage = app.state::<Storage>();
        let storage = (*storage).clone();
        log_err!(crate::core::tasks::setup(app, storage));
    } */

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

    let app_shutdown = app.state::<crate::setup::AppShutdown>().clone();
    crate::core::storage::register_web_storage_listener(
        app.app_handle(),
        app_shutdown.shutdown.child_token(),
        app_shutdown.tasks.clone(),
    );

    // The visible webview must not wait on service IPC or core reconciliation.
    // Keep startup under the same AppShutdown tracker as the other owners:
    // shutdown must wait for both blocking configuration writes and any
    // in-flight core reconciliation before cleanup shuts down the core.
    let client = (*app.state::<crate::client::ChimeraClient>()).clone();
    let app_handle = app.app_handle().clone();
    let startup_shutdown = app_shutdown.shutdown.child_token();
    // Tauri setup runs on the event-loop thread, not inside Tokio. Use its
    // runtime explicitly so the tracked task can start without a current handle.
    spawn_tracked_on_tauri_runtime(&app_shutdown.tasks, async move {
        // Service initialization is synchronous and may write its configured
        // port. Do not detach the blocking worker on cancellation: the
        // tracked owner joins it before shutdown begins core cleanup.
        let prepared =
            prepare_startup_while_active(startup_shutdown.clone(), move |preparation_shutdown| {
                if !preparation_shutdown.is_cancelled() {
                    log_err!(init::init_service(client.clone()));
                }
                client
            })
            .await;
        match prepared {
            Ok(None) => {
                tracing::debug!("startup cancelled before core reconciliation");
            }
            Ok(Some(client)) => {
                // Port resolution writes the legacy mirror. Serialize it with
                // user port changes and let shutdown cancel a task waiting for
                // that transaction lock before any write begins.
                let resolved = tokio::select! {
                    _ = startup_shutdown.cancelled() => None,
                    result = crate::client::ports::resolve_random_mixed_port(&client) => Some(result),
                };
                let Some(resolved) = resolved else {
                    tracing::debug!("startup cancelled before port reconciliation");
                    return;
                };
                log_err!(resolved);
                if startup_shutdown.is_cancelled() {
                    tracing::debug!("startup cancelled before core reconciliation");
                    return;
                }

                log::trace!("launch core (background)");
                // Once submitted to the lifecycle actor, reconciliation must
                // reach a terminal reply before the tracked task completes.
                // Dropping that future on cancellation could leave a queued
                // start racing the subsequent shutdown_core() command.
                match client.init_core().await {
                    Ok(()) => app_handle.state::<AppStartupStatus>().finish(true),
                    Err(error) => {
                        log::error!(target: "app", "core initialization failed: {error:#}");
                        app_handle.state::<AppStartupStatus>().finish(false);
                    }
                }
            }
            Err(error) => {
                log::error!(target: "app", "startup worker failed: {error}");
                app_handle.state::<AppStartupStatus>().finish(false);
            }
        }
    });
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
mod startup_shutdown_tests {
    use super::{prepare_startup_while_active, spawn_tracked_on_tauri_runtime};
    use crate::setup::AppShutdown;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::sync::oneshot;
    use tokio::time::{Duration, timeout};
    use tokio_util::{sync::CancellationToken, task::TaskTracker};

    #[test]
    fn tracked_startup_uses_tauri_runtime_outside_tokio_context() {
        // The real Tauri setup callback runs on the native event-loop thread.
        // The production startup path uses this same scheduling helper.
        assert!(tokio::runtime::Handle::try_current().is_err());
        let tasks = TaskTracker::new();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        spawn_tracked_on_tauri_runtime(&tasks, async move {
            done_tx.send(()).unwrap();
        });
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        tasks.close();
        tauri::async_runtime::block_on(tasks.wait());
    }

    #[tokio::test]
    async fn cancelled_startup_does_not_start_service_preparation() {
        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let touched = Arc::new(AtomicBool::new(false));
        let touched_in_worker = touched.clone();

        let result = prepare_startup_while_active(shutdown, move |_| {
            touched_in_worker.store(true, Ordering::SeqCst);
            42
        })
        .await
        .unwrap();

        assert_eq!(result, None);
        assert!(!touched.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn shutdown_drains_blocking_preparation_before_core_cleanup() {
        let boundary = AppShutdown {
            shutdown: CancellationToken::new(),
            tasks: TaskTracker::new(),
        };
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let core_started = Arc::new(AtomicBool::new(false));
        let core_started_in_task = core_started.clone();
        let cancellation = boundary.shutdown.child_token();

        // Simulate a Service or port-config write that was already underway
        // when the exit boundary requests shutdown.
        boundary.tasks.spawn(async move {
            let prepared = prepare_startup_while_active(cancellation, move |_| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
                42
            })
            .await
            .unwrap();
            if prepared.is_some() {
                core_started_in_task.store(true, Ordering::SeqCst);
            }
        });

        timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        boundary.request_shutdown();
        assert!(
            timeout(Duration::from_millis(75), boundary.wait_shutdown())
                .await
                .is_err(),
            "shutdown must not finish while the blocking write is in flight"
        );

        release_tx.send(()).unwrap();
        timeout(Duration::from_secs(2), boundary.wait_shutdown())
            .await
            .unwrap();
        assert!(
            !core_started.load(Ordering::SeqCst),
            "cancellation before preparation completes must suppress core startup"
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
