use std::borrow::Borrow;

use anyhow::Result;
use nyanpasu_ipc::api::status::CoreState;

use crate::{
    config::{chimera::IVerge, core::Config, profile::item::remote::RemoteProfileOptionsBuilder},
    core::{clash::core::CoreManager, handle, service::ipc::get_ipc_state, sysopt},
};
use handle::Message;

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
    let proxy_bypass = patch.system_proxy_bypass;

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

                update_core_config().await?;
            }
        }

        if system_proxy.is_some() || proxy_bypass.is_some() {
            sysopt::Sysopt::global().update_sysproxy()?;
            sysopt::Sysopt::global().guard_proxy();
        }

        // todo!();
        <Result<()>>::Ok(())
    };
    match res().await {
        Ok(()) => {
            Config::verge().apply();
            Config::verge().data().save_file()?;
            Ok(())
        }
        Err(err) => {
            Config::verge().discard();
            Err(err)
        }
    }
}

/// 更新配置
async fn update_core_config() -> Result<()> {
    match CoreManager::global().update_config().await {
        Ok(_) => {
            handle::Handle::refresh_clash();
            handle::Handle::notice_message(&Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<()> {
    let uid = uid.borrow();
    let profile_item = Config::profiles().latest().get_item(uid)?.clone();
    let is_remote = profile_item.is_remote();

    todo!()
}
