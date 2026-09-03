//! Clash desired-configuration client boundary.
//!
//! REF owns Clash desired state through a typed config client. Chimera still
//! persists the legacy Clash guard mapping plus typed runtime overrides, so
//! this transitional client keeps those storage semantics while centralizing
//! reads, validation, mutation, and running-core coordination here.

use anyhow::{Result, bail};
use chimera_config::clash::config::{
    ClashConfig,
    clash_strategy::{PortStrategy, PortStrategyKind},
};
use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;

use crate::{
    bridge::clash::clash_config_from_legacy,
    config::{chimera::IVerge, clash::ClashInfo, core::Config, runtime::ClashConfigOverrides},
    core::{
        clash::transaction::{RuntimePatchCoordinator, TransactionOutcome},
        handle, sysopt,
    },
    log_err,
};

use super::ChimeraClient;

#[derive(Default)]
pub(crate) struct ClashConfigClient {
    runtime_patch: RuntimePatchCoordinator,
}

struct ClashPatchPlan {
    mixed_port: Option<u16>,
    mixed_port_changed: bool,
    external_controller: Option<String>,
    external_controller_changed: bool,
    mode_changed: bool,
    requires_restart: bool,
}

impl ClashConfigClient {
    pub(crate) fn legacy() -> Self {
        Self::default()
    }

    fn get(&self) -> Result<ClashConfig> {
        let legacy_verge = Config::verge().data().clone();
        let legacy_clash = Config::clash().data().clone();
        clash_config_from_legacy(&legacy_verge, &legacy_clash.0)
    }

    pub(crate) fn get_info(&self) -> ClashInfo {
        Config::clash().latest().get_client_info()
    }

    pub(super) async fn apply_legacy_verge_runtime_change(
        &self,
        owner: &ChimeraClient,
        patch: &IVerge,
    ) -> Result<()> {
        if patch.enable_tun_mode.is_none() {
            return Ok(());
        }

        log::debug!(target: "app", "toggle tun mode");
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            use crate::utils::dirs::check_core_permission;
            let current_core = Config::verge().data().clash_core.unwrap_or_default();
            let current_core: chimera_utils::core::CoreType = (&current_core).into();
            let service_state = crate::core::service::ipc::get_ipc_state();
            if !service_state.is_connected()
                && check_core_permission(&current_core)
                    .inspect_err(|e| {
                        log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                    })
                    .is_ok_and(|v| !v)
            {
                log::debug!(target: "app", "clash core permission is missing, tun toggle will restart core and may still fail");
            };
        }

        update_core_config(owner).await
    }

    async fn patch(&self, owner: &ChimeraClient, patch: Mapping) -> Result<()> {
        let overrides = ClashConfigOverrides::from_mapping(&patch)?;
        self.patch_with_overrides(owner, patch, overrides).await
    }

    async fn patch_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        let patch = overrides.to_mapping();
        self.patch_with_overrides(owner, patch, overrides).await
    }

    async fn patch_running_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        let mapping = overrides.to_mapping();
        let persist_overrides = overrides.clone();
        let client = owner.clone();

        self.runtime_patch
            .apply(
                mapping,
                crate::core::clash::api::get_configs,
                |patch| async move { crate::core::clash::api::patch_configs(&patch).await },
                move |_patch| {
                    let overrides = persist_overrides.clone();
                    let client = client.clone();
                    async move { client.patch_clash_overrides(overrides).await }
                },
            )
            .await
    }

    async fn patch_with_overrides(
        &self,
        owner: &ChimeraClient,
        patch: Mapping,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        let current = self.get()?;
        Config::clash().draft().patch_config(patch.clone());
        let result = async {
            let plan = plan_clash_patch(&patch, &current)?;
            validate_mixed_port_change(&plan, &current)?;
            validate_external_controller_change(owner, &plan, &current).await?;
            apply_clash_runtime_change(owner, &plan).await?;
            run_clash_patch_side_effects(&plan);
            Config::runtime().draft().patch_config(&overrides);
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
}

impl ChimeraClient {
    pub(crate) fn clash_info(&self) -> ClashInfo {
        self.inner.clash_config.get_info()
    }

    pub(crate) async fn patch_clash(&self, patch: Mapping) -> Result<()> {
        self.inner.clash_config.patch(self, patch).await
    }

    pub(crate) async fn patch_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        self.inner
            .clash_config
            .patch_overrides(self, overrides)
            .await
    }

    pub(crate) async fn patch_running_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        self.inner
            .clash_config
            .patch_running_overrides(self, overrides)
            .await
    }
}

fn get_non_null_patch_value<'a>(patch: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    patch.get(key).filter(|value| !value.is_null())
}

fn plan_clash_patch(patch: &Mapping, current: &ClashConfig) -> Result<ClashPatchPlan> {
    let mixed_port = get_non_null_patch_value(patch, "mixed-port").and_then(|value| value.as_u64());
    let mixed_port = mixed_port
        .map(|port| u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port")))
        .transpose()?;
    let mixed_port_changed = mixed_port
        .map(|port| port != current.mixed_port.start_port)
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

fn validate_mixed_port_change(plan: &ClashPatchPlan, current: &ClashConfig) -> Result<()> {
    if plan.mixed_port_changed
        && current.mixed_port.kind != PortStrategyKind::Random
        && let Some(port) = plan.mixed_port
        && !port_scanner::local_port_available(port)
    {
        bail!("port already in use");
    }

    Ok(())
}

async fn validate_external_controller_change(
    client: &ChimeraClient,
    plan: &ClashPatchPlan,
    current: &ClashConfig,
) -> Result<()> {
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
    let strategy = PortStrategy {
        kind: current.external_controller.port.kind.clone(),
        start_port: port,
    };
    let core_state = client.core_status().await?;

    if matches!(&core_state.state, CoreState::Running) && strategy.pick_and_try_port().is_err() {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn apply_clash_runtime_change(client: &ChimeraClient, plan: &ClashPatchPlan) -> Result<()> {
    if !plan.requires_restart {
        return Ok(());
    }

    client.rebuild_running_config().await
}

async fn update_core_config(client: &ChimeraClient) -> Result<()> {
    match client.rebuild_running_config().await {
        Ok(_) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

fn run_clash_patch_side_effects(plan: &ClashPatchPlan) {
    if plan.mixed_port.is_some() {
        log_err!(sysopt::Sysopt::global().init_sysproxy());
    }

    if plan.mode_changed {
        crate::feat::update_proxies_buff(None);
        log::debug!("systray mode changed, update proxies buff");
        log_err!(handle::Handle::update_systray_part());
    }
}
