//! Application configuration client boundary.
//!
//! REF owns application configuration through a typed actor-backed client.
//! Chimera still persists the combined legacy `IVerge` model, so this
//! transitional client keeps the same persistence and side-effect ordering
//! while moving ownership and mutation serialization behind the ref-style
//! application boundary.

use anyhow::{Result, bail};

use super::ChimeraClient;
use crate::{
    config::{chimera::IVerge, core::Config},
    core::{handle, sysopt},
    utils,
};

#[derive(Default)]
pub(crate) struct ApplicationClient {
    patch_gate: tokio::sync::Mutex<()>,
}

impl ApplicationClient {
    pub(crate) fn legacy() -> Self {
        Self::default()
    }

    pub(crate) fn get(&self) -> IVerge {
        Config::verge().latest().clone()
    }

    async fn patch(&self, owner: &ChimeraClient, patch: IVerge) -> anyhow::Result<()> {
        let _guard = self.patch_gate.lock().await;
        patch_legacy_uncoordinated(owner, patch).await
    }
}

impl ChimeraClient {
    pub(crate) fn application_config(&self) -> IVerge {
        self.inner.application.get()
    }

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<()> {
        self.inner.application.patch(self, patch).await
    }
}

struct VergePatchPlan {
    service_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    enable_proxy_guard: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    refresh_systray: bool,
}

fn plan_verge_patch(patch: &IVerge) -> Result<VergePatchPlan> {
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::chimera::is_hex_color(theme_color)
    {
        bail!("Invalid theme color: {}", theme_color);
    }

    Ok(VergePatchPlan {
        service_mode: patch.enable_service_mode,
        auto_launch_changed: patch.enable_auto_launch.is_some(),
        system_proxy_changed: patch.enable_system_proxy.is_some(),
        proxy_bypass_changed: patch.system_proxy_bypass.is_some(),
        enable_proxy_guard: patch.enable_proxy_guard == Some(true),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        refresh_systray: patch.enable_system_proxy.is_some() || patch.enable_tun_mode.is_some(),
    })
}

async fn apply_verge_runtime_change(client: &ChimeraClient, plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = crate::core::service::ipc::get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        client.rebuild_running_config().await?;
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

    log::debug!("todo: handle other fields");
    Ok(())
}

async fn patch_legacy_uncoordinated(client: &ChimeraClient, patch: IVerge) -> Result<()> {
    Config::verge().draft().patch_config(patch.clone());
    let result = async {
        let plan = plan_verge_patch(&patch)?;
        apply_verge_runtime_change(client, &plan).await?;
        client
            .inner
            .clash_config
            .apply_legacy_verge_runtime_change(client, &patch)
            .await?;
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
