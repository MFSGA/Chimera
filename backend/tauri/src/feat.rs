use std::borrow::Borrow;

use anyhow::{Result, bail};
use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;
use tracing::debug;

use crate::{
    config::{
        chimera::IVerge,
        core::Config,
        profile::item::remote::{RemoteProfileOptionsBuilder, RemoteProfileSubscription},
    },
    core::{clash::core::CoreManager, handle, service::ipc::get_ipc_state, sysopt},
    log_err,
    utils::{self, help::get_clash_external_port},
};
use handle::Message;

struct ClashPatchPlan {
    mixed_port: Option<u16>,
    mixed_port_changed: bool,
    external_controller: Option<String>,
    external_controller_changed: bool,
    mode_changed: bool,
    requires_restart: bool,
}

fn get_non_null_patch_value<'a>(patch: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    patch.get(key).filter(|value| !value.is_null())
}

// Build a normalized view of the clash patch before validation and side effects.
fn plan_clash_patch(patch: &Mapping) -> Result<ClashPatchPlan> {
    let mixed_port = get_non_null_patch_value(patch, "mixed-port").and_then(|value| value.as_u64());
    let mixed_port = mixed_port
        .map(|port| u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port")))
        .transpose()?;
    let mixed_port_changed = mixed_port
        .map(|port| {
            port != Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port())
        })
        .unwrap_or(false);

    let external_controller = get_non_null_patch_value(patch, "external-controller")
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow::anyhow!("external-controller must be a string"))
        })
        .transpose()?;
    let external_controller_changed = external_controller
        .as_ref()
        .map(|controller| controller != &Config::clash().data().get_client_info().server)
        .unwrap_or(false);

    Ok(ClashPatchPlan {
        mixed_port,
        mixed_port_changed,
        external_controller,
        external_controller_changed,
        mode_changed: get_non_null_patch_value(patch, "mode").is_some(),
        requires_restart: get_non_null_patch_value(patch, "mixed-port").is_some()
            || get_non_null_patch_value(patch, "secret").is_some()
            || get_non_null_patch_value(patch, "external-controller").is_some(),
    })
}

fn validate_mixed_port_change(plan: &ClashPatchPlan) -> Result<()> {
    if plan.mixed_port_changed
        && let Some(port) = plan.mixed_port
        && !port_scanner::local_port_available(port)
    {
        bail!("port already in use");
    }

    Ok(())
}

async fn validate_external_controller_change(plan: &ClashPatchPlan) -> Result<()> {
    if !plan.external_controller_changed {
        return Ok(());
    }

    let external_controller = plan
        .external_controller
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("missing external-controller"))?;
    let (_, port) = external_controller
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("external-controller must be host:port"))?;
    let port = port.parse::<u16>()?;
    let strategy = Config::verge()
        .latest()
        .get_external_controller_port_strategy();
    let core_state = crate::core::CoreManager::global().status().await;

    if matches!(core_state.0.as_ref(), &CoreState::Running)
        && get_clash_external_port(&strategy, port).is_err()
    {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn apply_clash_runtime_change(plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    Config::generate().await?;
    CoreManager::global().run_core().await?;
    handle::Handle::refresh_clash();
    Ok(())
}

fn run_clash_patch_side_effects(plan: &ClashPatchPlan) {
    if plan.mixed_port.is_some() {
        log_err!(sysopt::Sysopt::global().init_sysproxy());
    }

    if plan.mode_changed {
        crate::feat::update_proxies_buff(None);
        debug!("systray mode changed, update proxies buff");
        log_err!(handle::Handle::update_systray_part());
    }
}

/// 修改clash的配置
pub async fn patch_clash(patch: Mapping) -> Result<()> {
    Config::clash().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_clash_patch(&patch)?;
        validate_mixed_port_change(&plan)?;
        validate_external_controller_change(&plan).await?;
        apply_clash_runtime_change(&plan).await?;
        run_clash_patch_side_effects(&plan);
        Config::runtime().latest().patch_config(patch);
        Ok(())
    }
    .await;

    match result {
        Ok(()) => {
            Config::clash().apply();
            Config::clash().data().save_config()?;
            Ok(())
        }
        Err(err) => {
            Config::clash().discard();
            Err(err)
        }
    }
}

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

    // let auto_launch = patch.enable_auto_launch;

    let system_proxy = patch.enable_system_proxy;
    let proxy_bypass = patch.system_proxy_bypass;
    let language = patch.language.clone();
    let log_level = patch.app_log_level;
    let log_max_files = patch.max_log_files;
    // let network_statistic_widget = patch.network_statistic_widget;
    let res = || async move {
        let service_mode = patch.enable_service_mode;
        let ipc_state = get_ipc_state();
        if service_mode.is_some() && ipc_state.is_connected() {
            log::debug!(target: "app", "change service mode to {}", service_mode.unwrap());

            Config::generate().await?;
            CoreManager::global().run_core().await?;
        }

        if tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: chimera_utils::core::CoreType = (&current_core).into();
                let service_state = crate::core::service::ipc::get_ipc_state();
                if !service_state.is_connected() && check_core_permission(&current_core).inspect_err(|e| {
                    log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                }).is_ok_and(|v| !v) {
                    log::debug!(target: "app", "clash core permission is missing, tun toggle will restart core and may still fail");
                };
            }
            update_core_config().await?;
        }

        if system_proxy.is_some() || proxy_bypass.is_some() {
            sysopt::Sysopt::global().update_sysproxy()?;
            sysopt::Sysopt::global().guard_proxy();
        }

        if let Some(true) = patch.enable_proxy_guard {
            sysopt::Sysopt::global().guard_proxy();
        }

        if log_level.is_some() || log_max_files.is_some() {
            utils::init::refresh_logger((log_level, log_max_files))?;
        }

        if system_proxy.or(tun_mode).is_some() {
            handle::Handle::update_systray_part()?;
        }

        debug!("todo: handle other fields");

        <Result<()>>::Ok(())
    };

    match res().await {
        Ok(()) => {
            Config::verge().apply();
            Config::verge().data().save_file()?;
            handle::Handle::refresh_verge();
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
    match CoreManager::global()
        .restart_core_with_generated_config()
        .await
    {
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
    let res = || async move {
        let mut item = profile_item.as_remote().unwrap().clone();
        item.subscribe(opts).await?;

        let should_update = {
            let mut profiles = Config::profiles().draft();
            profiles.replace_item(uid, item.into())?;
            profiles.get_current().iter().any(|current| current == uid)
        };

        if should_update {
            update_core_config().await?;
        }

        <Result<()>>::Ok(())
    };

    match res().await {
        Ok(()) => {
            Config::profiles().apply();
            Config::profiles().data().save_file()?;
            handle::Handle::refresh_profiles();
            Ok(())
        }
        Err(err) => {
            Config::profiles().discard();
            Err(err)
        }
    }
}

pub fn update_proxies_buff(rx: Option<tokio::sync::oneshot::Receiver<()>>) {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

    tauri::async_runtime::spawn(async move {
        if let Some(rx) = rx
            && let Err(e) = rx.await
        {
            log::error!(target: "app::clash::proxies", "update proxies buff by rx failed: {e}");
        }
        match ProxiesGuard::global().update().await {
            Ok(_) => {
                log::debug!(target: "app::clash::proxies", "update proxies buff success");
                handle::Handle::mutate_proxies();
            }
            Err(e) => {
                log::error!(target: "app::clash::proxies", "update proxies buff failed: {e}");
            }
        }
    });
}

pub fn change_clash_mode(mode: String) {
    tauri::async_runtime::spawn(async move {
        let mut patch = Mapping::new();
        patch.insert("mode".into(), mode.into());

        if let Err(err) = crate::core::clash::api::patch_configs(&patch).await {
            log::error!(target: "app", "failed to patch clash mode api: {err:?}");
            return;
        }

        if let Err(err) = patch_clash(patch).await {
            log::error!(target: "app", "failed to patch clash mode state: {err:?}");
        }
    });
}

pub fn toggle_system_proxy() {
    let enabled = Config::verge()
        .latest()
        .enable_system_proxy
        .unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_system_proxy: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle system proxy: {err:?}");
        }
    });
}

pub fn toggle_tun_mode() {
    let enabled = Config::verge().latest().enable_tun_mode.unwrap_or(false);
    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_tun_mode: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle tun mode: {err:?}");
        }
    });
}

pub fn restart_clash_core() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = CoreManager::global().run_core().await {
            log::error!(target: "app", "failed to restart clash core: {err:?}");
            return;
        }
        log_err!(handle::Handle::update_systray_part());
    });
}
