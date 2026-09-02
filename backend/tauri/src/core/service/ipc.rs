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

use super::compat::ServiceCompat;

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
        .compare_exchange(
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
        .compare_exchange(
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
            let mut warned_ineligible = false;
            loop {
                if KILL_FLAG.load(Ordering::Acquire) {
                    set_ipc_state(IpcState::Disconnected);
                    HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                    break;
                }
                warned_ineligible = health_check(warned_ineligible).await;
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        })
    });
}

#[derive(Debug, PartialEq, Eq)]
enum WarnLevel {
    Warn,
    Debug,
    Silent,
}

fn next_ineligible_warning_state(warned: bool, ineligible_but_running: bool) -> (WarnLevel, bool) {
    match (ineligible_but_running, warned) {
        (true, false) => (WarnLevel::Warn, true),
        (true, true) => (WarnLevel::Debug, true),
        (false, _) => (WarnLevel::Silent, false),
    }
}

fn target_ipc_state(
    info: &chimera_ipc::types::StatusInfo<'_>,
    runtime_owned: bool,
) -> (IpcState, ServiceCompat) {
    let compat = ServiceCompat::classify(info);
    let state = match info.status {
        ServiceStatus::Running if compat.allows_service_backend() && runtime_owned => {
            IpcState::Connected
        }
        _ => IpcState::Disconnected,
    };
    (state, compat)
}

#[instrument]
async fn health_check(warned: bool) -> bool {
    match super::control::status().await {
        Ok(info) => {
            let runtime_owned = super::is_service_runtime_owned(&info);
            let (state, compat) = target_ipc_state(&info, runtime_owned);
            let ineligible_but_running =
                info.status == ServiceStatus::Running && (state == IpcState::Disconnected);
            let (level, next_warned) =
                next_ineligible_warning_state(warned, ineligible_but_running);

            match level {
                WarnLevel::Warn => tracing::warn!(
                    ?compat,
                    runtime_owned,
                    "service daemon is ineligible; core will continue on local backend"
                ),
                WarnLevel::Debug => tracing::debug!(
                    ?compat,
                    runtime_owned,
                    "service daemon remains ineligible; core will continue on local backend"
                ),
                WarnLevel::Silent => {}
            }

            match state {
                IpcState::Connected => dispatch_connected(),
                IpcState::Disconnected => dispatch_disconnected(),
            }
            next_warned
        }
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected();
            let (_, next_warned) = next_ineligible_warning_state(warned, false);
            next_warned
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{borrow::Cow, path::PathBuf};

    use chimera_ipc::{
        api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody},
        types::{ServiceStatus, StatusInfo},
    };

    use super::{
        IpcState, WarnLevel, next_ineligible_warning_state, should_rebuild_for_ipc_transition,
        target_ipc_state,
    };
    use crate::core::{RunType, service::compat::ServiceCompat};

    fn running_status(server_version: &str) -> StatusInfo<'static> {
        StatusInfo {
            name: Cow::Borrowed("chimera-service"),
            version: Cow::Borrowed("1.9.0"),
            status: ServiceStatus::Running,
            server: Some(StatusResBody {
                version: Cow::Owned(server_version.to_owned()),
                core_infos: CoreInfos {
                    r#type: None,
                    state: CoreState::Stopped(None),
                    state_changed_at: 0,
                    config_path: None,
                },
                runtime_infos: RuntimeInfos {
                    service_data_dir: Cow::Owned(PathBuf::new()),
                    service_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_data_dir: Cow::Owned(PathBuf::new()),
                },
            }),
        }
    }

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

    #[test]
    fn compatible_owned_daemon_reaches_service_backend() {
        let info = running_status("1.9.0");
        let (state, compat) = target_ipc_state(&info, true);
        assert_eq!(state, IpcState::Connected);
        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: "1.9.0".to_owned(),
            }
        );
    }

    #[test]
    fn incompatible_daemon_never_reaches_service_backend() {
        let info = running_status("2.0.0");
        let (state, compat) = target_ipc_state(&info, true);
        assert_eq!(state, IpcState::Disconnected);
        assert!(matches!(compat, ServiceCompat::Incompatible { .. }));
    }

    #[test]
    fn foreign_runtime_never_reaches_service_backend() {
        let info = running_status("1.9.0");
        let (state, compat) = target_ipc_state(&info, false);
        assert_eq!(state, IpcState::Disconnected);
        assert!(compat.allows_service_backend());
    }

    #[test]
    fn ineligible_warning_is_latched() {
        assert_eq!(
            next_ineligible_warning_state(false, true),
            (WarnLevel::Warn, true)
        );
        assert_eq!(
            next_ineligible_warning_state(true, true),
            (WarnLevel::Debug, true)
        );
        assert_eq!(
            next_ineligible_warning_state(true, false),
            (WarnLevel::Silent, false)
        );
    }
}
