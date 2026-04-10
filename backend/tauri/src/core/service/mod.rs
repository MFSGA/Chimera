use std::path::PathBuf;

use chimera_ipc::types::StatusInfo;
use once_cell::sync::Lazy;

use crate::{
    config::core::Config,
    utils::dirs::{app_config_dir, app_data_dir, app_install_dir},
};

/// 1
pub mod control;
/// 2
pub mod ipc;

/// todo: service changed to a new service
/// const SERVICE_NAME: &str = "nyanpasu-service";
const SERVICE_NAME: &str = "chimera-service";

pub static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});

fn normalize_path(path: &std::path::Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn is_service_runtime_compatible(status: &StatusInfo<'_>) -> bool {
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

pub async fn init_service() {
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
        if is_service_runtime_compatible(&status) {
            ipc::spawn_health_check();
            while !ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        } else {
            tracing::warn!(
                "service is running but bound to a different app runtime; ignore service mode for this instance"
            );
        }
    }
}
