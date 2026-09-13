use std::path::PathBuf;

use anyhow::Context;
use chimera_ipc::api::status::CoreState;
use chimera_ipc::types::StatusInfo;
use once_cell::sync::Lazy;

use crate::{
    config::core::Config,
    utils::dirs::{app_config_dir, app_data_dir, app_install_dir},
};

pub mod compat;
pub mod control;
pub mod ipc;

const SERVICE_NAME: &str = "chimera-service";

static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});

/// Serialize explicit Local <-> Service core handoffs with health-check driven
/// reconciliation. This is intentionally narrower than upstream's ServiceActor:
/// Chimera only needs one transition owner here to keep the privileged Windows
/// TUN host stable while the legacy service backend remains in place.
pub(super) static HOST_TRANSITION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

pub fn is_service_runtime_compatible(status: &StatusInfo<'_>) -> bool {
    compat::ServiceCompat::classify(status).allows_service_backend()
        && is_service_runtime_owned(status)
}

async fn converge_core_to_service_host_locked(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
    require_running_before: bool,
) -> anyhow::Result<()> {
    use crate::core::RunType;
    ipc::wait_until_ready(ready_timeout)
        .await
        .context("Chimera Service backend did not become ready")?;

    let before = client
        .core_status()
        .await
        .context("failed to inspect the core before Service host convergence")?;
    if require_running_before && !matches!(before.state, CoreState::Running) {
        anyhow::bail!("TUN on Windows requires a running core");
    }

    if (!matches!(before.state, CoreState::Running) || before.run_type != RunType::Service)
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
        .core_status()
        .await
        .context("failed to verify the core after Service host convergence")?;
    if !matches!(after.state, CoreState::Running) || after.run_type != RunType::Service {
        anyhow::bail!("core did not reach the Chimera Service host");
    }

    Ok(())
}

/// Converge the running core onto the Service host after an explicit daemon
/// start/restart. The same serialized path is used on every platform; Windows
/// TUN additionally requires this convergence before enabling the tunnel.
pub(crate) async fn ensure_service_host_ready(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    converge_core_to_service_host_locked(client, ready_timeout, false).await
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

/// After an explicit Service stop, converge the core back to the local host.
/// The stop command has already completed at this point, so a failed status
/// probe is treated as an error rather than guessing that the daemon is gone.
async fn ensure_local_host_after_service_stop_locked(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    use crate::core::RunType;

    let observation = ipc::refresh_state_now()
        .await
        .context("failed to verify Chimera Service after stop")?;
    if observation.status == chimera_ipc::types::ServiceStatus::Running {
        anyhow::bail!("Chimera Service still reports running after stop");
    }
    ipc::mark_disconnected_now();

    let before = client
        .core_status()
        .await
        .context("failed to inspect the core after Service stop")?;
    if (!matches!(before.state, CoreState::Running) || before.run_type == RunType::Service)
        && let Err(error) = client.rebuild_running_config().await
    {
        ipc::request_reconcile(client);
        return Err(error).context("failed to restore the core to the local host");
    }

    let after = client
        .core_status()
        .await
        .context("failed to verify the local core after Service stop")?;
    if !matches!(after.state, CoreState::Running) || after.run_type == RunType::Service {
        anyhow::bail!("core did not recover to the local host after Service stop");
    }
    Ok(())
}

pub(crate) async fn ensure_local_host_after_service_stop(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    ensure_local_host_after_service_stop_locked(client).await
}

/// Execute an explicit daemon start under the same transition lock used by the
/// health loop, then converge the core to the Service host when Service Mode is
/// desired. This prevents a health observation from interleaving a second host
/// switch with the explicit command.
pub(crate) async fn start_service_and_converge(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    control::start_service(client.clone()).await?;
    if client.get_app_config()?.enable_service_mode {
        converge_core_to_service_host_locked(client, ready_timeout, false).await?;
    }
    Ok(())
}

/// Restarting the daemon is allowed while TUN is enabled, but the whole restart
/// and Service-host restoration must be one serialized transition. Otherwise a
/// transient Disconnected health event can race the restart and launch a local
/// core with a still-enabled Windows TUN configuration.
pub(crate) async fn restart_service_and_converge(
    client: &crate::client::ChimeraClient,
    ready_timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    control::restart_service(client.clone()).await?;
    if client.get_app_config()?.enable_service_mode {
        converge_core_to_service_host_locked(client, ready_timeout, false).await?;
    }
    Ok(())
}

pub(crate) async fn stop_service_and_converge(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    control::stop_service().await?;
    if client.get_app_config()?.enable_service_mode {
        ensure_local_host_after_service_stop_locked(client).await?;
    } else {
        ipc::mark_disconnected_now();
    }
    Ok(())
}

pub(crate) async fn uninstall_service_and_converge(
    client: &crate::client::ChimeraClient,
) -> anyhow::Result<()> {
    let _transition = HOST_TRANSITION_LOCK.lock().await;
    control::uninstall_service().await?;
    if client.get_app_config()?.enable_service_mode {
        ensure_local_host_after_service_stop_locked(client).await?;
    } else {
        ipc::mark_disconnected_now();
    }
    Ok(())
}

pub async fn init_service(client: crate::client::ChimeraClient) {
    let enable_service = {
        *Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false)
    };
    if !enable_service {
        return;
    }

    if let Ok(status) = control::status().await
        && matches!(status.status, chimera_ipc::types::ServiceStatus::Running)
    {
        // Monitor any running daemon, even when it is currently incompatible or
        // owned by another runtime. The health loop remains fail-closed, but it
        // can observe a later service update/reinstall and reconnect without an
        // app restart.
        ipc::spawn_health_check(client);
        while !ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
