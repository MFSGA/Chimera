use std::path::PathBuf;

use once_cell::sync::Lazy;

use crate::utils::dirs::app_install_dir;

/// 1
pub mod control;
/// 2
pub mod ipc;

/// todo: service changed to a new service
const SERVICE_NAME: &str = "nyanpasu-service";
// const SERVICE_NAME: &str = "chimera-service";

pub static SERVICE_PATH: Lazy<PathBuf> = Lazy::new(|| {
    let app_path = app_install_dir().unwrap();
    app_path.join(format!("{}{}", SERVICE_NAME, std::env::consts::EXE_SUFFIX))
});
