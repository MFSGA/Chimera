use std::sync::atomic::{AtomicBool, Ordering};

use atomic_enum::atomic_enum;
use chimera_ipc::types::ServiceStatus;
use chimera_utils::runtime::block_on;
use serde::Serialize;
use tracing::instrument;

use crate::{client::ChimeraClient, core::RunType, log_err};

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

pub(crate) struct ServiceBackendObservation {
    pub status: ServiceStatus,
    pub compat: ServiceCompat,
    pub runtime_owned: bool,
}

impl ServiceBackendObservation {
    pub fn is_ready(&self) -> bool {
        self.status == ServiceStatus::Running
            && self.compat.allows_service_backend()
            && self.runtime_owned
    }
}

/// Probe the daemon synchronously and make the process-wide IPC classification
/// match that observation without scheduling a second rebuild. Callers that
/// need an immediate host transition can then perform exactly one explicit
/// rebuild against the freshly classified backend.
pub(crate) async fn refresh_state_now() -> anyhow::Result<ServiceBackendObservation> {
    let info =
        match tokio::time::timeout(std::time::Duration::from_secs(5), super::control::status())
            .await
        {
            Ok(Ok(info)) => info,
            Ok(Err(error)) => return Err(error),
            Err(_) => anyhow::bail!("service status probe timed out"),
        };
    let runtime_owned = super::is_service_runtime_owned(&info);
    let (state, compat) = target_ipc_state(&info, runtime_owned);
    IPC_STATE.store(state, Ordering::SeqCst);
    Ok(ServiceBackendObservation {
        status: info.status,
        compat,
        runtime_owned,
    })
}

/// Wait for a just-started daemon to expose a compatible endpoint owned by
/// this Chimera runtime. Incompatible or foreign runtimes fail immediately;
/// startup/transient probe failures are retried within the bounded window.
pub(crate) async fn wait_until_ready(
    timeout: std::time::Duration,
) -> anyhow::Result<ServiceBackendObservation> {
    let started = std::time::Instant::now();
    loop {
        let last_error = match refresh_state_now().await {
            Ok(observation) if observation.is_ready() => return Ok(observation),
            Ok(observation) => {
                if matches!(
                    observation.compat,
                    ServiceCompat::Incompatible { .. } | ServiceCompat::Unparsable { .. }
                ) {
                    anyhow::bail!("service daemon is incompatible: {:?}", observation.compat);
                }
                if observation.status == ServiceStatus::Running
                    && observation.compat.allows_service_backend()
                    && !observation.runtime_owned
                {
                    anyhow::bail!("service daemon belongs to another runtime");
                }
                format!(
                    "service status is {:?}, compatibility is {:?}, runtime_owned={}",
                    observation.status, observation.compat, observation.runtime_owned
                )
            }
            Err(error) => error.to_string(),
        };

        if started.elapsed() >= timeout {
            anyhow::bail!("service backend did not become ready: {last_error}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

pub(crate) fn mark_disconnected_now() {
    IPC_STATE.store(IpcState::Disconnected, Ordering::SeqCst);
}

pub(super) fn set_ipc_state(state: IpcState, client: &ChimeraClient) {
    IPC_STATE.store(state, Ordering::Relaxed);
    on_ipc_state_changed(state, client);
}

fn dispatch_disconnected(client: &ChimeraClient) {
    if IPC_STATE
        .compare_exchange(
            IpcState::Connected,
            IpcState::Disconnected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Disconnected, client)
    }
}

fn dispatch_connected(client: &ChimeraClient) {
    if IPC_STATE
        .compare_exchange(
            IpcState::Disconnected,
            IpcState::Connected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Connected, client)
    }
}

fn should_rebuild_for_ipc_transition(state: IpcState, run_type: RunType) -> bool {
    matches!(
        (state, run_type),
        (IpcState::Connected, RunType::Normal) | (IpcState::Disconnected, RunType::Service)
    )
}

fn should_rebuild_for_ipc_event(
    observed_state: IpcState,
    current_state: IpcState,
    run_type: RunType,
) -> bool {
    observed_state == current_state && should_rebuild_for_ipc_transition(observed_state, run_type)
}

#[instrument(skip(client))]
fn on_ipc_state_changed(state: IpcState, client: &ChimeraClient) {
    tracing::info!("IPC state changed: {:?}", state);
    let client = client.clone();
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async move {
            let enabled_service = match client.get_app_config() {
                Ok(config) => config.enable_service_mode,
                Err(error) => {
                    tracing::warn!(
                        "failed to read Service Mode during service IPC transition: {error}"
                    );
                    return;
                }
            };
            if !enabled_service {
                return;
            }

            let _transition = super::HOST_TRANSITION_LOCK.lock().await;
            let current_state = get_ipc_state();
            if current_state != state {
                tracing::debug!(
                    observed = ?state,
                    current = ?current_state,
                    "discarding stale service IPC transition"
                );
                return;
            }

            let status = match client.core_status().await {
                Ok(status) => status,
                Err(err) => {
                    tracing::warn!(
                        "failed to read core status during service IPC transition: {err}"
                    );
                    return;
                }
            };

            if should_rebuild_for_ipc_event(state, current_state, status.run_type) {
                tracing::info!("Restarting core due to IPC state change");
                log_err!(client.rebuild_running_config().await);
            }
        })
    });
}

/// Preserve the health-check recovery behavior after a synchronous Service
/// transition has already updated `IPC_STATE` but the immediate core handoff
/// failed. This deliberately reuses the same guarded reconcile path rather
/// than introducing a second host-switch implementation.
pub(crate) fn request_reconcile(client: &ChimeraClient) {
    on_ipc_state_changed(get_ipc_state(), client);
}

pub(super) fn spawn_health_check(client: ChimeraClient) {
    KILL_FLAG.store(false, Ordering::Relaxed);
    std::thread::spawn(move || {
        HEALTH_CHECK_RUNNING.store(true, Ordering::Release);
        block_on(async {
            let mut warned_ineligible = false;
            loop {
                if KILL_FLAG.load(Ordering::Acquire) {
                    set_ipc_state(IpcState::Disconnected, &client);
                    HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                    break;
                }
                warned_ineligible = health_check(warned_ineligible, &client).await;
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

#[instrument(skip(client))]
async fn health_check(warned: bool, client: &ChimeraClient) -> bool {
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
                IpcState::Connected => dispatch_connected(client),
                IpcState::Disconnected => dispatch_disconnected(client),
            }
            next_warned
        }
        Err(e) => {
            tracing::error!("IPC health check failed: {}", e);
            dispatch_disconnected(client);
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
        IpcState, ServiceBackendObservation, WarnLevel, next_ineligible_warning_state,
        should_rebuild_for_ipc_event, should_rebuild_for_ipc_transition, target_ipc_state,
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
    fn stale_ipc_transition_is_discarded_after_serialized_service_restart() {
        assert!(!should_rebuild_for_ipc_event(
            IpcState::Disconnected,
            IpcState::Connected,
            RunType::Service
        ));
        assert!(should_rebuild_for_ipc_event(
            IpcState::Disconnected,
            IpcState::Disconnected,
            RunType::Service
        ));
        assert!(!should_rebuild_for_ipc_event(
            IpcState::Connected,
            IpcState::Disconnected,
            RunType::Normal
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
        assert!(
            ServiceBackendObservation {
                status: info.status,
                compat,
                runtime_owned: true,
            }
            .is_ready()
        );
    }

    #[test]
    fn tun_service_readiness_requires_running_compatible_owned_daemon() {
        let compatible = ServiceCompat::Compatible {
            server_version: "1.9.0".to_owned(),
        };
        assert!(
            ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: compatible.clone(),
                runtime_owned: true,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Stopped,
                compat: compatible.clone(),
                runtime_owned: true,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: compatible,
                runtime_owned: false,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: ServiceCompat::Incompatible {
                    server_version: "2.0.0".to_owned(),
                    required_major: 1,
                },
                runtime_owned: true,
            }
            .is_ready()
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
