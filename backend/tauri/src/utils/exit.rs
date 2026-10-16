//! The exit boundary: every `ExitRequested` Tauri raises is decided here, on
//! the main thread, without blocking it. Shutdown runs separately and asks
//! Tauri to exit again after application-owned work has finished.

use tauri::{AppHandle, ExitRequestApi, Manager, RESTART_EXIT_CODE, process::current_binary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitIntent {
    Exit(i32),
    Restart,
}

impl ExitIntent {
    fn code(self) -> i32 {
        match self {
            Self::Exit(code) => code,
            Self::Restart => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDecision {
    Prevent,
    Shutdown,
    Allow { relaunch: bool },
    AllowUncleaned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExitPhase {
    Running,
    Stopping,
    Finished,
}

#[derive(Debug)]
pub struct ExitGate {
    phase: ExitPhase,
    code: i32,
    restart: bool,
}

impl Default for ExitGate {
    fn default() -> Self {
        Self {
            phase: ExitPhase::Running,
            code: 0,
            restart: false,
        }
    }
}

impl ExitGate {
    pub fn on_exit_requested(&mut self, code: Option<i32>) -> ExitDecision {
        match (self.phase, code) {
            (ExitPhase::Finished, Some(RESTART_EXIT_CODE)) => {
                ExitDecision::Allow { relaunch: false }
            }
            (ExitPhase::Finished, Some(_)) => ExitDecision::Allow {
                relaunch: std::mem::take(&mut self.restart),
            },
            (_, None) => ExitDecision::Prevent,
            (_, Some(RESTART_EXIT_CODE)) => ExitDecision::AllowUncleaned,
            (ExitPhase::Stopping, Some(_)) => ExitDecision::Prevent,
            (ExitPhase::Running, Some(code)) => {
                self.phase = ExitPhase::Stopping;
                self.code = code;
                ExitDecision::Shutdown
            }
        }
    }

    pub fn request_restart(&mut self) {
        self.restart = true;
    }

    pub fn finish(&mut self) -> ExitIntent {
        self.phase = ExitPhase::Finished;
        match std::mem::take(&mut self.restart) {
            true => ExitIntent::Restart,
            false => ExitIntent::Exit(self.code),
        }
    }

    pub fn cleaned_up(&mut self) {
        self.phase = ExitPhase::Finished;
    }

    pub fn has_shut_down(&self) -> bool {
        self.phase == ExitPhase::Finished
    }
}

/// Tauri-managed state for coordinated application shutdown.
#[derive(Debug, Default)]
pub struct ExitBoundary {
    gate: parking_lot::Mutex<ExitGate>,
}

impl ExitBoundary {
    pub fn request_restart(&self) {
        self.gate.lock().request_restart();
    }
}

/// Handles an exit request on Tauri's event thread and starts shutdown
/// asynchronously so the event loop remains available while owners drain.
pub fn on_exit_requested(app_handle: &AppHandle, code: Option<i32>, api: &ExitRequestApi) {
    let decision = app_handle
        .state::<ExitBoundary>()
        .gate
        .lock()
        .on_exit_requested(code);
    match decision {
        ExitDecision::Prevent => api.prevent_exit(),
        ExitDecision::Shutdown => {
            api.prevent_exit();
            start_shutdown(app_handle);
        }
        ExitDecision::Allow { relaunch } => {
            if relaunch {
                spawn_relauncher(app_handle);
            }
        }
        ExitDecision::AllowUncleaned => {
            tracing::warn!("restarting before shutdown finishes: Tauri cannot hold a restart")
        }
    }
}

fn start_shutdown(app_handle: &AppHandle) {
    let shutdown = request_shutdown(app_handle);
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(shutdown) = shutdown {
            shutdown.wait_shutdown().await;
        }
        unregister_hotkeys(&app_handle).await;
        if let Err(error) = crate::utils::help::cleanup_processes_async(&app_handle).await {
            tracing::error!(%error, "application cleanup failed during exit");
        }
        let intent = app_handle.state::<ExitBoundary>().gate.lock().finish();
        #[cfg(windows)]
        crate::shutdown_hook::set_ready_for_shutdown();
        if intent == ExitIntent::Restart {
            spawn_relauncher(&app_handle);
        }
        app_handle.exit(intent.code());
    });
}

/// Drains application-owned work for a caller such as the updater installer,
/// which may end the process without producing a Tauri exit event.
pub async fn shutdown_before_exit(app_handle: &AppHandle) -> anyhow::Result<()> {
    if let Some(shutdown) = request_shutdown(app_handle) {
        shutdown.wait_shutdown().await;
    }
    unregister_hotkeys(app_handle).await;
    let result = crate::utils::help::cleanup_processes_async(app_handle).await;
    app_handle.state::<ExitBoundary>().gate.lock().cleaned_up();
    #[cfg(windows)]
    crate::shutdown_hook::set_ready_for_shutdown();
    result
}

/// Synchronous counterpart for callers that must block before ending the
/// process. Prefer [`shutdown_before_exit`] when the caller can await.
pub fn shutdown_before_exit_blocking(app_handle: &AppHandle) -> anyhow::Result<()> {
    let shutdown = shutdown_before_exit(app_handle);
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| tauri::async_runtime::block_on(shutdown))
    } else {
        tauri::async_runtime::block_on(shutdown)
    }
}

pub fn has_shut_down(app_handle: &AppHandle) -> bool {
    app_handle
        .state::<ExitBoundary>()
        .gate
        .lock()
        .has_shut_down()
}

fn request_shutdown(app_handle: &AppHandle) -> Option<crate::setup::AppShutdown> {
    let shutdown = app_handle
        .try_state::<crate::setup::AppShutdown>()
        .map(|state| state.inner().clone())?;
    shutdown.request_shutdown();
    Some(shutdown)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn unregister_hotkeys(app_handle: &AppHandle) {
    if let Some(client) = app_handle.try_state::<crate::client::ChimeraClient>() {
        client.unregister_hotkeys().await;
    }
}

#[cfg(any(target_os = "android", target_os = "ios"))]
async fn unregister_hotkeys(_app_handle: &AppHandle) {}

fn spawn_relauncher(app_handle: &AppHandle) {
    let path = current_binary(&app_handle.env()).unwrap();
    let args = std::env::args().skip(1);
    let mut relaunch_args = vec!["launch".to_string(), "--".to_string()];
    relaunch_args.extend(args);
    tracing::info!("restart app: {:#?} with args: {:#?}", path, relaunch_args);
    #[allow(clippy::zombie_processes)]
    std::process::Command::new(path)
        .args(relaunch_args)
        .spawn()
        .expect("application failed to start");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stopping() -> ExitGate {
        let mut gate = ExitGate::default();
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Shutdown);
        gate
    }

    fn finished() -> ExitGate {
        let mut gate = stopping();
        assert_eq!(gate.finish(), ExitIntent::Exit(0));
        gate
    }

    #[test]
    fn each_request_goes_to_the_expected_exit_phase() {
        let mut running = ExitGate::default();
        assert_eq!(running.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            running.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::AllowUncleaned
        );
        assert_eq!(running.on_exit_requested(Some(3)), ExitDecision::Shutdown);

        let mut stopping = stopping();
        assert_eq!(stopping.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            stopping.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::AllowUncleaned
        );
        assert_eq!(stopping.on_exit_requested(Some(1)), ExitDecision::Prevent);

        let mut finished = finished();
        assert_eq!(finished.on_exit_requested(None), ExitDecision::Prevent);
        assert_eq!(
            finished.on_exit_requested(Some(RESTART_EXIT_CODE)),
            ExitDecision::Allow { relaunch: false }
        );
        assert_eq!(
            finished.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );
    }

    #[test]
    fn shutdown_ends_with_the_code_that_started_it() {
        let mut gate = ExitGate::default();
        assert_eq!(gate.on_exit_requested(Some(1)), ExitDecision::Shutdown);
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Prevent);
        assert_eq!(gate.finish(), ExitIntent::Exit(1));
    }

    #[test]
    fn a_restart_requested_before_shutdown_relaunches_once() {
        let mut gate = ExitGate::default();
        gate.request_restart();
        assert_eq!(gate.on_exit_requested(Some(0)), ExitDecision::Shutdown);
        assert_eq!(gate.finish(), ExitIntent::Restart);
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );

        let mut stopping = stopping();
        stopping.request_restart();
        assert_eq!(stopping.on_exit_requested(Some(0)), ExitDecision::Prevent);
        assert_eq!(stopping.finish(), ExitIntent::Restart);
    }

    #[test]
    fn a_restart_after_external_cleanup_relaunches_on_the_next_exit() {
        let mut gate = ExitGate::default();
        assert!(!gate.has_shut_down());
        gate.cleaned_up();
        assert!(gate.has_shut_down());
        assert_eq!(gate.on_exit_requested(None), ExitDecision::Prevent);
        gate.request_restart();
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: true }
        );
        assert_eq!(
            gate.on_exit_requested(Some(0)),
            ExitDecision::Allow { relaunch: false }
        );
    }
}
