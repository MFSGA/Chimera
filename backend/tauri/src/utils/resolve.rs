use tauri::App;

use crate::{config::core::Config, core::clash::core::CoreManager, log_err};

/// handle something when start app
pub fn resolve_setup(app: &mut App) {
    // 启动核心
    log::trace!("init config");
    log_err!(Config::init_config());

    log::trace!("launch core");
    log_err!(CoreManager::global().init());
}
