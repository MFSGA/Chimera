use anyhow::Result;
use nyanpasu_ipc::api::status::CoreState;

use crate::{
    config::{chimera::IVerge, core::Config},
    core::{clash::core::CoreManager, service::ipc::get_ipc_state},
};

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    // Validate theme_color if it's being updated
    if let Some(ref theme_color) = patch.theme_color {
        if !theme_color.is_empty() && !crate::config::chimera::is_hex_color(theme_color) {
            anyhow::bail!("Invalid theme color: {}", theme_color);
        }
    }
    Config::verge().draft().patch_config(patch.clone());
    let tun_mode = patch.enable_tun_mode;

    let system_proxy = patch.enable_system_proxy;
    let log_level = patch.app_log_level;
    let log_max_files = patch.max_log_files;

    let res = || async move {
        // let service_mode = patch.enable_service_mode;
        let ipc_state = get_ipc_state();
        /*  if service_mode.is_some() && ipc_state.is_connected() {
            log::debug!(target: "app", "change service mode to {}", service_mode.unwrap());

            Config::generate().await?;
            CoreManager::global().run_core().await?;
        } */

        if tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[allow(unused_mut)]
            let mut flag = false;
            /* #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: nyanpasu_utils::core::CoreType = (&current_core).into();
                let service_state = crate::core::service::ipc::get_ipc_state();
                if !service_state.is_connected() && check_core_permission(&current_core).inspect_err(|e| {
                    log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                }).is_ok_and(|v| !v) {
                    log::debug!(target: "app", "grant core permission, and restart core");
                    flag = true;
                }
            } */
            let (state, _, _) = CoreManager::global().status().await;
            if flag || matches!(state.as_ref(), CoreState::Stopped(_)) {
                log::debug!(target: "app", "core is stopped, restart core");
                Config::generate().await?;
                CoreManager::global().run_core().await?;
            } else {
                log::debug!(target: "app", "update core config");
                #[cfg(target_os = "macos")]
                log::debug!("todo");
                todo!()
                // update_core_config().await?;
            }
        }
        todo!();
        <Result<()>>::Ok(())
    };
    match res().await {
        Ok(()) => {
            todo!()
        }
        Err(err) => {
            Config::verge().discard();
            Err(err)
        }
    }
}
