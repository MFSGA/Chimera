//! Tauri compatibility bridge for the shared Service process contract.
//!
//! The upstream-aligned OS command implementation lives in `chimera-os`.
//! This shell still owns legacy paths, its health watcher, and an extra native
//! restart operation; it does not execute privileged commands itself.

use std::time::Duration;

use chimera_contracts::{
    elevation::ElevationConsent,
    service::{ServiceCommandError, ServiceProcessControl},
};
use chimera_core::actor_v2::service_process::parse_service_status;
use chimera_os::service;

use super::SERVICE_PATH;
use crate::utils::dirs::{app_config_dir, app_data_dir, app_install_dir};

const SERVICE_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ServiceOsProcess;

fn host_paths()
-> Result<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf), ServiceCommandError> {
    let data = app_data_dir().map_err(|error| ServiceCommandError::ResolveLegacyServiceDirs {
        message: error.to_string(),
    })?;
    let config =
        app_config_dir().map_err(|error| ServiceCommandError::ResolveLegacyServiceDirs {
            message: error.to_string(),
        })?;
    let install =
        app_install_dir().map_err(|error| ServiceCommandError::ResolveLegacyServiceDirs {
            message: error.to_string(),
        })?;
    Ok((data, config, install))
}

/// IPC compatibility: expose the same install-argument preview without
/// launching an elevated process or changing the Service CLI.
pub async fn get_service_install_args() -> anyhow::Result<Vec<std::ffi::OsString>> {
    let (data, config, install) = host_paths()?;
    Ok(service::get_service_install_args(&data, &config, &install).await?)
}

#[async_trait::async_trait]
impl ServiceProcessControl for ServiceOsProcess {
    async fn probe(&self) -> Result<String, ServiceCommandError> {
        tokio::time::timeout(
            SERVICE_STATUS_TIMEOUT,
            service::status(SERVICE_PATH.as_path()),
        )
        .await
        .map_err(|_| ServiceCommandError::TimedOut {
            limit_ms: SERVICE_STATUS_TIMEOUT.as_millis() as u64,
        })?
    }

    async fn install(&self) -> Result<(), ServiceCommandError> {
        service::install_service_with_args(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
            async {
                let (data, config, install) = host_paths()?;
                service::get_service_install_args(&data, &config, &install).await
            },
        )
        .await
    }

    async fn uninstall(&self) -> Result<(), ServiceCommandError> {
        service::uninstall_service(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
        )
        .await?;
        super::ipc::stop_health_check();
        Ok(())
    }

    async fn start_daemon(&self) -> Result<(), ServiceCommandError> {
        service::start_service(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
        )
        .await
    }

    async fn stop_daemon(&self) -> Result<(), ServiceCommandError> {
        service::stop_service(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
        )
        .await?;
        super::ipc::stop_health_check();
        Ok(())
    }

    async fn update(&self) -> Result<(), ServiceCommandError> {
        service::update_service(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
        )
        .await
    }

    async fn restart_daemon(&self) -> Result<(), ServiceCommandError> {
        service::restart_service(
            &crate::adapters::elevation::InteractiveElevationConsent,
            SERVICE_PATH.as_path(),
        )
        .await
    }
}

/// The independent CoreActor owns status decoding, not the shell.
pub(crate) async fn probe_service_status()
-> Result<chimera_ipc::types::StatusInfo<'static>, ServiceCommandError> {
    let raw = ServiceOsProcess.probe().await?;
    parse_service_status(&raw)
}
