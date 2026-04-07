use std::path::PathBuf;

use chimera_ipc::types::StatusInfo;
use once_cell::sync::Lazy;

use crate::{config::core::Config, utils::dirs::app_install_dir};

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

pub async fn init_service() {
    let enable_service = {
        *Config::verge()
            .latest()
            .enable_service_mode
            .as_ref()
            .unwrap_or(&false)
    };
    if let Ok(StatusInfo {
        status: chimera_ipc::types::ServiceStatus::Running,
        ..
    }) = control::status().await
        && enable_service
    {
        ipc::spawn_health_check();
        while !ipc::HEALTH_CHECK_RUNNING.load(std::sync::atomic::Ordering::Acquire) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
}
