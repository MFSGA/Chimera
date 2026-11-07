//! OS-specific implementation of core-owned ServiceProcessControl.

use std::{ffi::OsString, time::Duration};

use chimera_core::actor_v2::service_process::{
    ServiceCommand, ServiceCommandError, ServiceProcessControl, parse_service_status,
};
use runas::Command as RunasCommand;

use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};

use super::SERVICE_PATH;

const SERVICE_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ServiceOsProcess;

fn process_exit(status: std::process::ExitStatus) -> (Option<i32>, Option<i32>) {
    (status.code(), {
        #[cfg(unix)]
        {
            status.signal()
        }
        #[cfg(not(unix))]
        {
            None
        }
    })
}

async fn run_service_command(
    command: ServiceCommand,
    args: Vec<OsString>,
) -> Result<(), ServiceCommandError> {
    let status = tokio::task::spawn_blocking(move || {
        #[cfg(not(target_os = "macos"))]
        {
            RunasCommand::new(SERVICE_PATH.as_path())
                .args(&args)
                .gui(true)
                .show(true)
                .status()
        }
        #[cfg(target_os = "macos")]
        {
            use crate::utils::sudo::sudo;
            let args = args.iter().map(|s| s.to_string_lossy()).collect::<Vec<_>>();
            match sudo(SERVICE_PATH.to_string_lossy(), &args) {
                Ok(()) => Ok(std::process::ExitStatus::from_raw(0)),
                Err(error) => {
                    tracing::error!(%error, %command, "service elevation failed");
                    Err(error)
                }
            }
        }
    })
    .await
    .map_err(|error| ServiceCommandError::RunElevated {
        command,
        source: std::io::Error::other(error),
    })?
    .map_err(|source| ServiceCommandError::RunElevated { command, source })?;

    if !status.success() {
        let (exit_code, signal) = process_exit(status);
        return Err(ServiceCommandError::ServiceCommandExit {
            command,
            exit_code,
            signal,
        });
    }
    Ok(())
}

pub async fn get_service_install_args() -> anyhow::Result<Vec<OsString>> {
    let user = {
        #[cfg(windows)]
        {
            chimera_utils::os::get_current_user_sid().await?
        }
        #[cfg(not(windows))]
        {
            whoami::username()?
        }
    };
    let data_dir = app_data_dir()?;
    let config_dir = app_config_dir()?;
    let app_dir = app_install_dir()?;
    Ok(vec![
        "install".into(),
        "--user".into(),
        user.into(),
        "--nyanpasu-data-dir".into(),
        data_dir.into(),
        "--nyanpasu-config-dir".into(),
        config_dir.into(),
        "--nyanpasu-app-dir".into(),
        app_dir.into(),
    ])
}

#[async_trait::async_trait]
impl ServiceProcessControl for ServiceOsProcess {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        let mut cmd = tokio::process::Command::new(SERVICE_PATH.as_path());
        cmd.args(["status", "--json"]);
        cmd.kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let output = tokio::time::timeout(SERVICE_STATUS_TIMEOUT, cmd.output())
            .await
            .map_err(|_| ServiceCommandError::TimedOut {
                limit_ms: SERVICE_STATUS_TIMEOUT.as_millis() as u64,
            })?
            .map_err(|source| ServiceCommandError::RunServiceStatus { source })?;
        if !output.status.success() {
            let (exit_code, signal) = process_exit(output.status);
            return Err(ServiceCommandError::ServiceStatusExit { exit_code, signal });
        }
        let raw = String::from_utf8(output.stdout)
            .map_err(|source| ServiceCommandError::ServiceStatusNotUtf8 { source })?;
        tracing::trace!("service status: {}", raw);
        Ok(raw)
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        let args = get_service_install_args().await.map_err(|error| {
            ServiceCommandError::ResolveServiceDirs {
                message: error.to_string(),
            }
        })?;
        run_service_command(ServiceCommand::Install, args).await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        run_service_command(ServiceCommand::Uninstall, vec!["uninstall".into()]).await?;
        super::ipc::KILL_FLAG.store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        run_service_command(ServiceCommand::Start, vec!["start".into()]).await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        run_service_command(ServiceCommand::Stop, vec!["stop".into()]).await?;
        super::ipc::KILL_FLAG.store(true, std::sync::atomic::Ordering::Release);
        Ok(())
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        run_service_command(ServiceCommand::Update, vec!["update".into()]).await
    }

    async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
        run_service_command(ServiceCommand::Restart, vec!["restart".into()]).await
    }
}

/// ServiceActor's typed probe-to-status boundary. The process adapter returns
/// raw UTF-8; decoding belongs to the independent core.
pub(crate) async fn probe_service_status()
-> Result<chimera_ipc::types::StatusInfo<'static>, ServiceCommandError> {
    let raw = ServiceOsProcess.probe().await?;
    parse_service_status(&raw)
}
