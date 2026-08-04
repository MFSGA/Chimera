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
    let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);

    if plan.mixed_port_changed
        && !enable_random_port
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

struct VergePatchPlan {
    service_mode: Option<bool>,
    tun_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    enable_proxy_guard: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    refresh_systray: bool,
}

// Build a normalized view of the verge patch before runtime changes and side effects.
fn plan_verge_patch(patch: &IVerge) -> Result<VergePatchPlan> {
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::chimera::is_hex_color(theme_color)
    {
        bail!("Invalid theme color: {}", theme_color);
    }

    Ok(VergePatchPlan {
        service_mode: patch.enable_service_mode,
        tun_mode: patch.enable_tun_mode,
        auto_launch_changed: patch.enable_auto_launch.is_some(),
        system_proxy_changed: patch.enable_system_proxy.is_some(),
        proxy_bypass_changed: patch.system_proxy_bypass.is_some(),
        enable_proxy_guard: patch.enable_proxy_guard == Some(true),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        refresh_systray: patch.enable_system_proxy.is_some() || patch.enable_tun_mode.is_some(),
    })
}

async fn apply_verge_runtime_change(plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        Config::generate().await?;
        CoreManager::global().run_core().await?;
    }

    if plan.tun_mode.is_some() {
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

    Ok(())
}

fn run_verge_patch_side_effects(plan: &VergePatchPlan, patch: &IVerge) -> Result<()> {
    if plan.auto_launch_changed {
        sysopt::Sysopt::global().update_launch()?;
    }

    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        sysopt::Sysopt::global().update_sysproxy()?;
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.enable_proxy_guard {
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.log_level_changed || plan.log_max_files_changed {
        utils::init::refresh_logger((patch.app_log_level.clone(), patch.max_log_files))?;
    }

    if plan.refresh_systray {
        handle::Handle::update_systray_part()?;
    }

    debug!("todo: handle other fields");

    Ok(())
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
        Config::runtime().draft().patch_config(patch);
        Ok(plan)
    }
    .await;

    match result {
        Ok(plan) => {
            Config::clash().apply();
            Config::runtime().apply();
            Config::clash().data().save_config()?;
            if plan.mode_changed {
                log_err!(
                    crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change()
                        .await,
                    "failed to interrupt connections after mode change"
                );
            }
            Ok(())
        }
        Err(err) => {
            Config::clash().discard();
            Config::runtime().discard();
            Err(err)
        }
    }
}

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_verge_patch(&patch)?;
        apply_verge_runtime_change(&plan).await?;
        run_verge_patch_side_effects(&plan, &patch)?;
        Ok(())
    }
    .await;

    match result {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingMode {
    Rule,
    Global,
    Direct,
}

impl RoutingMode {
    const fn as_core_value(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }
}

fn routing_mode_patch(mode: RoutingMode) -> Mapping {
    let mut patch = Mapping::new();
    patch.insert("mode".into(), mode.as_core_value().into());
    patch
}

/// Apply an explicit routing-mode target to the running core and persisted Clash state.
pub async fn set_routing_mode(mode: RoutingMode) -> Result<()> {
    let patch = routing_mode_patch(mode);
    crate::core::clash::api::patch_configs(&patch).await?;
    patch_clash(patch).await
}

pub fn change_clash_mode(mode: String) {
    let mode = match mode.as_str() {
        "rule" => RoutingMode::Rule,
        "global" => RoutingMode::Global,
        "direct" => RoutingMode::Direct,
        _ => {
            log::error!(target: "app", "failed to change clash mode: unsupported mode");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        if let Err(err) = set_routing_mode(mode).await {
            log::error!(target: "app", "failed to change clash mode: {err:?}");
        }
    });
}

#[cfg(any(feature = "agent", test))]
fn service_mode_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_service_mode: Some(enabled),
        ..IVerge::default()
    }
}

/// Apply an explicit service-mode target through the normal Verge persistence and core path.
///
/// A matching target returns without regenerating configuration or restarting the core.
#[cfg(feature = "agent")]
pub async fn set_service_mode(enabled: bool) -> Result<()> {
    let current = Config::verge()
        .latest()
        .enable_service_mode
        .unwrap_or(false);
    if current == enabled {
        return Ok(());
    }
    apply_service_mode(enabled).await
}

#[cfg(feature = "agent")]
pub(crate) async fn restore_service_mode(enabled: bool) -> Result<()> {
    apply_service_mode(enabled).await
}

#[cfg(feature = "agent")]
async fn apply_service_mode(enabled: bool) -> Result<()> {
    patch_verge(service_mode_patch(enabled)).await
}

fn system_proxy_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_system_proxy: Some(enabled),
        ..IVerge::default()
    }
}

/// Apply an explicit system-proxy target through the normal Verge persistence and host update path.
pub async fn set_system_proxy_enabled(enabled: bool) -> Result<()> {
    patch_verge(system_proxy_patch(enabled)).await
}

pub fn toggle_system_proxy() {
    let enabled = Config::verge()
        .latest()
        .enable_system_proxy
        .unwrap_or(false);
    if enabled {
        disable_system_proxy();
    } else {
        enable_system_proxy();
    }
}

pub fn enable_system_proxy() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_system_proxy_enabled(true).await {
            log::error!(target: "app", "failed to enable system proxy: {err:?}");
        }
    });
}

pub fn disable_system_proxy() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_system_proxy_enabled(false).await {
            log::error!(target: "app", "failed to disable system proxy: {err:?}");
        }
    });
}

fn tun_mode_patch(enabled: bool) -> IVerge {
    IVerge {
        enable_tun_mode: Some(enabled),
        ..IVerge::default()
    }
}

/// Apply an explicit TUN target through the same Verge persistence and runtime path as the UI.
///
/// Unlike `toggle_tun_mode`, this operation is idempotent and awaitable so controlled callers can
/// verify the final runtime state before reporting success.
pub async fn set_tun_enabled(enabled: bool) -> Result<()> {
    patch_verge(tun_mode_patch(enabled)).await
}

pub fn toggle_tun_mode() {
    let enabled = Config::verge().latest().enable_tun_mode.unwrap_or(false);
    if enabled {
        disable_tun_mode();
    } else {
        enable_tun_mode();
    }
}

pub fn enable_tun_mode() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_tun_enabled(true).await {
            log::error!(target: "app", "failed to enable tun mode: {err:?}");
        }
    });
}

pub fn disable_tun_mode() {
    tauri::async_runtime::spawn(async {
        if let Err(err) = set_tun_enabled(false).await {
            log::error!(target: "app", "failed to disable tun mode: {err:?}");
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

#[cfg(test)]
mod tests {
    use serde_yaml::Value;

    use super::{
        RoutingMode, routing_mode_patch, service_mode_patch, system_proxy_patch, tun_mode_patch,
    };

    #[test]
    fn routing_mode_patch_is_closed_and_scoped() {
        for (mode, expected) in [
            (RoutingMode::Rule, "rule"),
            (RoutingMode::Global, "global"),
            (RoutingMode::Direct, "direct"),
        ] {
            let patch = routing_mode_patch(mode);
            assert_eq!(patch.len(), 1);
            assert_eq!(
                patch.get(Value::String("mode".into())),
                Some(&Value::String(expected.into()))
            );
        }
    }

    #[test]
    fn service_mode_patch_is_explicit_and_scoped() {
        for (enabled, expected) in [(true, true), (false, false)] {
            let patch = service_mode_patch(enabled);
            assert_eq!(patch.enable_service_mode, Some(expected));

            let serialized = serde_yaml::to_value(patch).expect("service patch should serialize");
            let mapping = serialized
                .as_mapping()
                .expect("verge patch should serialize as a mapping");
            let non_null = mapping
                .iter()
                .filter(|(_, value)| !value.is_null())
                .collect::<Vec<_>>();

            assert_eq!(
                non_null.len(),
                1,
                "service mode setter must not patch unrelated fields"
            );
            assert_eq!(
                mapping.get(Value::String("enable_service_mode".into())),
                Some(&Value::Bool(expected))
            );
        }
    }

    #[test]
    fn system_proxy_patch_is_explicit_and_scoped() {
        for (enabled, expected) in [(true, true), (false, false)] {
            let patch = system_proxy_patch(enabled);
            assert_eq!(patch.enable_system_proxy, Some(expected));

            let serialized = serde_yaml::to_value(patch).expect("proxy patch should serialize");
            let mapping = serialized
                .as_mapping()
                .expect("verge patch should serialize as a mapping");
            let non_null = mapping
                .iter()
                .filter(|(_, value)| !value.is_null())
                .collect::<Vec<_>>();

            assert_eq!(
                non_null.len(),
                1,
                "system proxy setter must not patch unrelated fields"
            );
            assert_eq!(
                mapping.get(Value::String("enable_system_proxy".into())),
                Some(&Value::Bool(expected))
            );
        }
    }

    #[test]
    fn tun_mode_patch_is_explicit_and_scoped() {
        for (enabled, expected) in [(true, true), (false, false)] {
            let patch = tun_mode_patch(enabled);
            assert_eq!(patch.enable_tun_mode, Some(expected));

            let serialized = serde_yaml::to_value(patch).expect("tun patch should serialize");
            let mapping = serialized
                .as_mapping()
                .expect("verge patch should serialize as a mapping");
            let non_null = mapping
                .iter()
                .filter(|(_, value)| !value.is_null())
                .collect::<Vec<_>>();

            assert_eq!(
                non_null.len(),
                1,
                "tun setter must not patch unrelated fields"
            );
            assert_eq!(
                mapping.get(Value::String("enable_tun_mode".into())),
                Some(&Value::Bool(expected))
            );
        }
    }
}
