use std::path::PathBuf;

use anyhow::Context;
use chimera_core::actor_v2::{
    endpoint::ExecutionHost,
    router::{CoreStatusProjection, EndpointConnectivity},
};
use chimera_ipc::api::status::CoreStateDetail;
use chimera_ipc::types::StatusInfo;
use once_cell::sync::Lazy;

use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};

pub mod control;
pub mod ipc;

#[cfg(test)]
#[path = "service_host_tests.rs"]
mod host_tests;

const SERVICE_NAME: &str = "chimera-service";

static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});

/// Serialize explicit Local <-> Service core handoffs with health-check driven
/// reconciliation. This is intentionally narrower than upstream's ServiceActor:
/// Chimera only needs one transition owner here to keep the privileged Windows
/// TUN host stable while the legacy service backend remains in place.
pub(crate) static HOST_TRANSITION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn normalize_path(path: &std::path::Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn is_service_runtime_owned(status: &StatusInfo<'_>) -> bool {
    let expected_config_dir = match app_config_dir() {
        Ok(path) => normalize_path(&path),
        Err(err) => {
            tracing::warn!(
                "failed to resolve app config dir for service compatibility check: {err:?}"
            );
            return false;
        }
    };
    let expected_data_dir = match app_data_dir() {
        Ok(path) => normalize_path(&path),
        Err(err) => {
            tracing::warn!(
                "failed to resolve app data dir for service compatibility check: {err:?}"
            );
            return false;
        }
    };

    let Some(server) = status.server.as_ref() else {
        return false;
    };

    let service_config_dir = normalize_path(server.runtime_infos.nyanpasu_config_dir.as_ref());
    let service_data_dir = normalize_path(server.runtime_infos.nyanpasu_data_dir.as_ref());

    expected_config_dir == service_config_dir && expected_data_dir == service_data_dir
}

/// Windows TUN preflight is a privileged host proof, not a request to
/// prefer the Service run type. The ref router must report a connected and
/// running endpoint with a stable process identity.
fn actor_core_running(status: &CoreStatusProjection) -> bool {
    matches!(status.connectivity, EndpointConnectivity::Connected)
        && status.snapshot.as_ref().is_some_and(|snapshot| {
            matches!(snapshot.state, Some(CoreStateDetail::Running { .. }))
                && snapshot.healthy != Some(false)
                && snapshot
                    .instance_id
                    .as_deref()
                    .is_some_and(|id| !id.is_empty())
        })
}

fn actor_service_core_running(status: &CoreStatusProjection) -> bool {
    status.host == ExecutionHost::Service && actor_core_running(status)
}

async fn converge_core_to_service_host_locked(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
    require_running_before: bool,
) -> anyhow::Result<()> {
    ipc::wait_until_ready(ready_timeout)
        .await
        .context("Chimera Service backend did not become ready")?;

    let before = client
        .refreshed_routed_core_status()
        .await
        .context("failed to inspect the ref CoreActor before Service host convergence")?;
    if require_running_before && !actor_core_running(&before) {
        anyhow::bail!("TUN on Windows requires a confirmed running CoreActor instance");
    }
    anyhow::ensure!(
        !matches!(
            before.connectivity,
            EndpointConnectivity::HandingOff { .. } | EndpointConnectivity::ShutDown
        ),
        "CoreActor is changing ownership; cannot admit another Service handoff"
    );

    if !actor_service_core_running(&before)
        && let Err(error) = client.rebuild_running_config().await
    {
        ipc::request_reconcile(client);
        return Err(error).context("failed to hand off the core to Chimera Service");
    }

    // Re-probe after the handoff. A daemon that disappears during rebuild must
    // never be treated as a valid privileged host merely because the core start
    // call itself returned successfully.
    ipc::wait_until_ready(std::time::Duration::from_secs(5))
        .await
        .context("Chimera Service became unavailable during core handoff")?;
    let after = client
        .refreshed_routed_core_status()
        .await
        .context("failed to verify the ref CoreActor after Service host convergence")?;
    if !actor_service_core_running(&after) {
        anyhow::bail!("CoreActor did not confirm a running Chimera Service host");
    }

    Ok(())
}

/// On Windows, TUN must run from the privileged Service host. This performs a
/// synchronous, fail-closed preflight before the TUN desired state is committed:
/// the daemon must be compatible and owned by this runtime, and the currently
/// running core must actually be handed off to `RunType::Service`.
pub(crate) async fn ensure_tun_host_ready(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = client;
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let service_mode = client
            .get_app_config()
            .context("failed to read Service Mode before Windows TUN preflight")?
            .enable_service_mode;
        if !service_mode {
            anyhow::bail!("TUN on Windows requires Service Mode to be enabled");
        }

        let _transition = HOST_TRANSITION_LOCK.lock().await;
        let observation = ipc::refresh_state_now()
            .await
            .context("failed to probe Chimera Service for TUN")?;
        if !observation.is_ready()
            && observation.status != chimera_ipc::types::ServiceStatus::Running
        {
            anyhow::bail!(
                "Chimera Service must already be running before Windows TUN can be enabled"
            );
        }

        converge_core_to_service_host_locked(client, std::time::Duration::from_secs(3), true)
            .await
            .context("Windows TUN Service host preflight failed")
    }
}

/// Retire the health monitor while holding the host lock used by its probe.
/// The Service lifecycle actor acquires that lock again for its own transition.
async fn retire_health_check_before_service_transition() -> u64 {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    ipc::retire_health_check()
}

/// Execute an explicit daemon start after retiring any prior health round, then
/// converge the core to the Service host when Service Mode is desired. The
/// lifecycle actor owns the actual host-transition lock for the Service command.
pub(crate) async fn start_service_and_converge(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let expected_epoch = retire_health_check_before_service_transition().await;
    let result = client.start_service(ready_timeout).await;
    if result.is_ok() {
        ipc::ensure_health_check_for_epoch(client.clone(), expected_epoch).await;
    } else {
        restore_health_check_if_running(client, expected_epoch).await;
    }
    result
}

/// Restarting the daemon is allowed while TUN is enabled, but the whole restart
/// and Service-host restoration must be one serialized transition. Otherwise a
/// transient Disconnected health event can race the restart and launch a local
/// core with a still-enabled Windows TUN configuration.
pub(crate) async fn restart_service_and_converge(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let expected_epoch = retire_health_check_before_service_transition().await;
    let result = client.restart_service(ready_timeout).await;
    if result.is_ok() {
        ipc::ensure_health_check_for_epoch(client.clone(), expected_epoch).await;
    } else {
        restore_health_check_if_running(client, expected_epoch).await;
    }
    result
}

pub(crate) async fn stop_service_and_converge(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let expected_epoch = retire_health_check_before_service_transition().await;
    let result = client.stop_service().await;
    if result.is_err() {
        restore_health_check_if_running(client, expected_epoch).await;
    }
    result
}

pub(crate) async fn uninstall_service_and_converge(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let expected_epoch = retire_health_check_before_service_transition().await;
    let result = client.uninstall_service().await;
    if result.is_err() {
        restore_health_check_if_running(client, expected_epoch).await;
    }
    result
}

pub(crate) async fn install_service_and_converge(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let expected_epoch = retire_health_check_before_service_transition().await;
    let result = client.install_service().await;
    if result.is_ok() {
        ipc::ensure_health_check_for_epoch(client.clone(), expected_epoch).await;
    } else {
        restore_health_check_if_running(client, expected_epoch).await;
    }
    result
}

pub(crate) async fn update_service_for_startup(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let _expected_epoch = retire_health_check_before_service_transition().await;
    client.update_service().await
}

async fn restore_health_check_if_running(
    client: &crate::client::ChimeraClient,
    expected_epoch: u64,
) {
    let should_start = {
        let _transition = HOST_TRANSITION_LOCK.lock().await;
        if ipc::service_transition_epoch() != expected_epoch {
            return;
        }
        match control::probe_service_status().await {
            Ok(status) => matches!(status.status, chimera_ipc::types::ServiceStatus::Running),
            Err(error) => {
                tracing::warn!(%error, "could not determine Service status after a failed transition");
                // Keep an observer alive after a transient probe error. Its
                // own fail-closed probe loop can retry without mutating host state.
                true
            }
        }
    };
    if should_start {
        ipc::ensure_health_check_for_epoch(client.clone(), expected_epoch).await;
    }
}

pub async fn init_service(client: crate::client::ChimeraClient) -> anyhow::Result<()> {
    let enable_service_mode = client
        .get_app_config()
        .context("failed to read typed Service Mode preference before monitor setup")?
        .enable_service_mode;
    if !enable_service_mode {
        return Ok(());
    }

    let monitor_epoch = {
        let _transition = HOST_TRANSITION_LOCK.lock().await;
        if let Ok(status) = control::probe_service_status().await
            && matches!(status.status, chimera_ipc::types::ServiceStatus::Running)
        {
            // The UI remains available while the daemon probe runs; re-read the
            // setting before starting a long-lived background monitor.
            let mode_enabled = client
                .get_app_config()
                .context("failed to re-read typed Service Mode preference after daemon probe")?
                .enable_service_mode;
            mode_enabled.then(ipc::service_transition_epoch)
        } else {
            None
        }
    };

    // The old monitor may need the host lock to finish retiring. Never wait for
    // it while this startup probe still owns that lock.
    if let Some(expected_epoch) = monitor_epoch {
        ipc::ensure_health_check_for_epoch(client, expected_epoch).await;
    }
    Ok(())
}
