use anyhow::Result;
use tauri::{App, AppHandle};

use crate::{
    config::{chimera::ClashCore, core::Config},
    core::clash::core::CoreManager,
    log_err,
};

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    // 启动核心
    log::trace!("init config");
    log_err!(Config::init_config());

    log::trace!("launch core");
    log_err!(CoreManager::global().init());
}

/// resolve core version
pub async fn resolve_core_version(app_handle: &AppHandle, core_type: &ClashCore) -> Result<String> {
    todo!()
}
