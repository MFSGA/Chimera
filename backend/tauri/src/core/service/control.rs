use std::ffi::OsString;

use runas::Command as RunasCommand;

use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};

use super::SERVICE_PATH;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

fn format_exit_failure(action: &str, status: std::process::ExitStatus) -> anyhow::Error {
    anyhow::anyhow!(
        "failed to {action}, exit code: {}, signal: {:?}",
        status.code().unwrap_or(-1),
        {
            #[cfg(unix)]
            {
                status.signal().unwrap_or(0)
            }
            #[cfg(not(unix))]
            {
                0
            }
        }
    )
}

async fn run_service_command(action: &str, args: Vec<OsString>) -> anyhow::Result<()> {
    let action = action.to_owned();
    let status = tokio::task::spawn_blocking({
        #[cfg(target_os = "macos")]
        let action = action.clone();
        move || {
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
                    Err(e) => {
                        tracing::error!("failed to {}: {}", &action, e);
                        Err(e)
                    }
                }
            }
        }
    })
    .await??;

    if !status.success() {
        return Err(format_exit_failure(&action, status));
    }

    Ok(())
}

pub async fn get_service_install_args() -> Result<Vec<OsString>, anyhow::Error> {
    let user = {
        #[cfg(windows)]
        {
            nyanpasu_utils::os::get_current_user_sid().await?
        }
        #[cfg(not(windows))]
        {
            whoami::username()
        }
    };
    let data_dir = app_data_dir()?;
    let config_dir = app_config_dir()?;
    let app_dir = app_install_dir()?;

    #[cfg(not(windows))]
    let args: Vec<OsString> = vec![
        "install".into(),
        "--user".into(),
        user.into(),
        "--nyanpasu-data-dir".into(),
        format!("\"{}\"", data_dir.to_string_lossy()).into(),
        "--nyanpasu-config-dir".into(),
        format!("\"{}\"", config_dir.to_string_lossy()).into(),
        "--nyanpasu-app-dir".into(),
        format!("\"{}\"", app_dir.to_string_lossy()).into(),
    ];

    #[cfg(windows)]
    let args: Vec<OsString> = vec![
        "install".into(),
        "--user".into(),
        user.into(),
        "--nyanpasu-data-dir".into(),
        data_dir.into(),
        "--nyanpasu-config-dir".into(),
        config_dir.into(),
        "--nyanpasu-app-dir".into(),
        app_dir.into(),
    ];

    Ok(args)
}

pub async fn install_service() -> anyhow::Result<()> {
    run_service_command("install service", get_service_install_args().await?).await?;
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

pub async fn update_service() -> anyhow::Result<()> {
    run_service_command("update service", vec!["update".into()]).await
}

pub async fn uninstall_service() -> anyhow::Result<()> {
    run_service_command("uninstall service", vec!["uninstall".into()]).await?;
    let _ = super::ipc::KILL_FLAG.compare_exchange(
        false,
        true,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

pub async fn start_service() -> anyhow::Result<()> {
    run_service_command("start service", vec!["start".into()]).await?;
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

pub async fn stop_service() -> anyhow::Result<()> {
    run_service_command("stop service", vec!["stop".into()]).await?;
    let _ = super::ipc::KILL_FLAG.compare_exchange_weak(
        false,
        true,
        std::sync::atomic::Ordering::Acquire,
        std::sync::atomic::Ordering::Relaxed,
    );
    Ok(())
}

pub async fn restart_service() -> anyhow::Result<()> {
    run_service_command("restart service", vec!["restart".into()]).await?;
    if !super::ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
        super::ipc::spawn_health_check();
    }
    Ok(())
}

#[tracing::instrument]
pub async fn status<'a>() -> anyhow::Result<chimera_ipc::types::StatusInfo<'a>> {
    let mut cmd = tokio::process::Command::new(SERVICE_PATH.as_path());
    cmd.args(["status", "--json"]);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    let output = cmd.output().await?;
    if !output.status.success() {
        return Err(format_exit_failure("query service status", output.status));
    }

    let status = String::from_utf8(output.stdout)?;
    tracing::trace!("service status: {}", status);
    Ok(serde_json::from_str(&status)?)
}
