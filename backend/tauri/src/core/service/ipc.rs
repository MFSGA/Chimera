use std::sync::atomic::{AtomicBool, Ordering};

use atomic_enum::atomic_enum;
use chimera_ipc::types::ServiceStatus;
use chimera_utils::runtime::block_on;
use serde::Serialize;
use tauri::Manager;
use tracing::instrument;

use crate::{
    core::{RunType, clash::client::NyanpasuClient, handle::Handle},
    log_err,
};

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
pub enum IpcState {
    Connected,
    Disconnected,
}

impl IpcState {
    pub fn is_connected(&self) -> bool {
        *self == IpcState::Connected
    }
}

static IPC_STATE: AtomicIpcState = AtomicIpcState::new(IpcState::Disconnected);
pub(super) static KILL_FLAG: AtomicBool = AtomicBool::new(false);
pub(super) static HEALTH_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn get_ipc_state() -> IpcState {
    IPC_STATE.load(Ordering::Relaxed)
}

pub(super) fn set_ipc_state(state: IpcState) {
    IPC_STATE.store(state, Ordering::Relaxed);
    on_ipc_state_changed(state);
}

fn dispatch_disconnected() {
    if IPC_STATE
        .compare_exchange_weak(
            IpcState::Connected,
            IpcState::Disconnected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Disconnected)
    }
}

fn dispatch_connected() {
    if IPC_STATE
        .compare_exchange_weak(
            IpcState::Disconnected,
            IpcState::Connected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Connected)
    }
}

fn should_rebuild_for_ipc_transition(state: IpcState, run_type: RunType) -> bool {
    matches!(
        (state, run_type),
        (IpcState::Connected, RunType::Normal) | (IpcState::Disconnected, RunType::Service)
    )
}

#[instrument]
fn on_ipc_state_changed(state: IpcState) {
    tracing::info!("IPC state changed: {:?}", state);
    let enabled_service = {
        *crate::config::core::Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false)
    };
    let app_handle = Handle::app_handle();
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async move {
            if !enabled_service {
                return;
            }

            let Some(app_handle) = app_handle else {
                tracing::warn!("app handle is unavailable during service IPC transition");
                return;
            };
            let Some(client) = app_handle.try_state::<NyanpasuClient>() else {
                tracing::warn!("NyanpasuClient is unavailable during service IPC transition");
                return;
            };
            let status = match client.core_status().await {
                Ok(status) => status,
                Err(err) => {
                    tracing::warn!(
                        "failed to read core status during service IPC transition: {err}"
                    );
                    return;
                }
            };

            if should_rebuild_for_ipc_transition(state, status.run_type) {
                tracing::info!("Restarting core due to IPC state change");
                log_err!(client.rebuild_running_config().await);
            }
        })
    });
}

pub(super) fn spawn_health_check() {
    KILL_FLAG.store(false, Ordering::Relaxed);
    std::thread::spawn(|| {
        HEALTH_CHECK_RUNNING.store(true, Ordering::Release);
        block_on(async {
            loop {
                if KILL_FLAG.load(Ordering::Acquire) {
                    set_ipc_state(IpcState::Disconnected);
                    HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                    break;
                }
                health_check().await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
    });
}

#[instrument]
async fn health_check() {
    match super::control::status().await {
        Ok(info) => match info.status {
            ServiceStatus::Running if super::is_service_runtime_compatible(&info) => {
                dispatch_connected();
            }
            ServiceStatus::Running => {
                tracing::debug!(
                    "service is running but version or runtime ownership is incompatible; keep service mode disconnected"
                );
                dispatch_disconnected();
            }
            ServiceStatus::Stopped | ServiceStatus::NotInstalled => {
                dispatch_disconnected();
            }
        },
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{IpcState, should_rebuild_for_ipc_transition};
    use crate::core::RunType;

    #[test]
    fn service_ipc_transition_rebuilds_only_when_runtime_owner_changes() {
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Connected,
            RunType::Normal
        ));
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Disconnected,
            RunType::Service
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Connected,
            RunType::Service
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Disconnected,
            RunType::Normal
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Connected,
            RunType::Elevated
        ));
    }
}
