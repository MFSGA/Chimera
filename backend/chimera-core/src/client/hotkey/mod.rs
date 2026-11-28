//! Shared owner of the actual OS shortcut registration lifecycle.
mod actor;
pub mod ports;
#[cfg(test)]
mod tests;
pub use self::actor::Args as HotkeyArgs;
use self::{
    actor::{HotkeyActor, Message},
    ports::{HotkeyAction, HotkeyBindings},
};
use super::{
    ChimeraClient,
    configuration_mutations::ConfigurationMutations,
    effects::{
        plan::EffectKind,
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
};
use chimera_config::{
    application::ChimeraAppConfigPatch,
    clash::config::{
        ClashConfig, ClashConfigPatch, TransparentProxyConfigPatch, TransparentProxyMode,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
        overrides::{ClashGuardOverridesPatch, Mode},
    },
};
use chimera_contracts::runtime::MutationOutcome;
use ractor::{Actor, ActorRef, rpc::CallResult};
use std::{collections::BTreeMap, time::Duration};

const HOTKEY_RPC_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct HotkeyStatus {
    pub applied_revision: EffectRevision,
    pub health: EffectHealth,
    pub registered: BTreeMap<String, HotkeyAction>,
}

#[derive(Clone)]
pub struct HotkeyClient {
    actor: ActorRef<Message>,
}

impl HotkeyClient {
    pub async fn spawn(args: HotkeyArgs) -> anyhow::Result<Self> {
        let (actor, _handle) = Actor::spawn(None, HotkeyActor, args).await?;
        Ok(Self { actor })
    }

    pub async fn reconcile(
        &self,
        revision: EffectRevision,
        desired: HotkeyBindings,
    ) -> EffectStatus {
        match self
            .actor
            .call(
                |reply| Message::Reconcile {
                    revision,
                    desired,
                    reply,
                },
                Some(HOTKEY_RPC_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!(
                    revision = revision.get(),
                    "the hotkey actor did not answer within its bound: {other:?}"
                );
                timed_out(revision)
            }
        }
    }

    pub async fn status(&self) -> HotkeyStatus {
        match self
            .actor
            .call(Message::Status, Some(HOTKEY_RPC_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the hotkey actor did not report its status: {other:?}");
                HotkeyStatus {
                    applied_revision: EffectRevision::default(),
                    health: timeout_health(),
                    registered: BTreeMap::new(),
                }
            }
        }
    }

    pub async fn unregister_all(&self) -> EffectStatus {
        match self
            .actor
            .call(Message::UnregisterAll, Some(HOTKEY_RPC_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => status,
            other => {
                tracing::warn!("the hotkey actor did not release its shortcuts in time: {other:?}");
                timed_out(EffectRevision::default())
            }
        }
    }
}

fn timed_out(revision: EffectRevision) -> EffectStatus {
    EffectStatus {
        kind: EffectKind::Hotkeys,
        desired_revision: revision,
        applied_revision: EffectRevision::default(),
        health: timeout_health(),
    }
}

fn timeout_health() -> EffectHealth {
    EffectHealth::Degraded {
        code: "hotkey_timeout",
        message: "the hotkey actor did not answer within its bound".to_owned(),
        retryable: true,
    }
}

impl ChimeraClient {
    /// Runs a shortcut or tray action through the same committed configuration
    /// transactions as settings edits. The shell supplies only its native ports.
    pub async fn dispatch_hotkey_action(
        &self,
        action: HotkeyAction,
        mutations: &dyn ConfigurationMutations,
        window: Option<&dyn ports::WindowControl>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.shutdown().is_cancelled(), "client is shutting down");
        match action {
            HotkeyAction::OpenOrCloseDashboard => {
                window
                    .ok_or_else(|| anyhow::anyhow!("dashboard toggle is unavailable"))?
                    .toggle_dashboard()
                    .await?;
                Ok(())
            }
            HotkeyAction::ClashModeRule => self.set_clash_mode(mutations, Mode::Rule).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(mutations, Mode::Global).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(mutations, Mode::Direct).await,
            HotkeyAction::ClashModeScript => self.set_clash_mode(mutations, Mode::Script).await,
            HotkeyAction::ToggleSystemProxy => {
                let enabled = self.get_app_config()?.enable_system_proxy;
                self.set_system_proxy(mutations, !enabled).await
            }
            HotkeyAction::EnableSystemProxy => self.set_system_proxy(mutations, true).await,
            HotkeyAction::DisableSystemProxy => self.set_system_proxy(mutations, false).await,
            HotkeyAction::ToggleTunMode => {
                let enabled = self.get_clash_config()?.enable_tun_mode;
                self.set_tun_mode(mutations, !enabled).await
            }
            HotkeyAction::EnableTunMode => self.set_tun_mode(mutations, true).await,
            HotkeyAction::DisableTunMode => self.set_tun_mode(mutations, false).await,
            HotkeyAction::ToggleTransparentProxy => {
                let enabled = self.get_clash_config()?.transparent_proxy.mode
                    != TransparentProxyMode::Disabled;
                self.set_transparent_proxy(mutations, !enabled).await
            }
            HotkeyAction::EnableTransparentProxy => {
                self.set_transparent_proxy(mutations, true).await
            }
            HotkeyAction::DisableTransparentProxy => {
                self.set_transparent_proxy(mutations, false).await
            }
        }
    }

    async fn set_clash_mode(
        &self,
        mutations: &dyn ConfigurationMutations,
        mode: Mode,
    ) -> anyhow::Result<()> {
        let outcome = mutations
            .patch_overrides(ClashGuardOverridesPatch {
                mode: Some(mode),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_system_proxy(
        &self,
        mutations: &dyn ConfigurationMutations,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let outcome = mutations
            .patch_application(ChimeraAppConfigPatch {
                enable_system_proxy: Some(enabled),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_transparent_proxy(
        &self,
        mutations: &dyn ConfigurationMutations,
        enabled: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            cfg!(target_os = "linux"),
            "transparent capture is supported only on Linux"
        );
        let config = self.get_clash_config()?;
        let patch = switch_patch(&config, enabled);
        // Check the candidate before querying the privileged execution host.
        self.validate_clash_candidate(&patch)?;
        if enabled && config.transparent_proxy.mode == TransparentProxyMode::Disabled {
            let status = self.get_transparent_proxy_status().await?;
            anyhow::ensure!(
                status.supported,
                "the service does not support transparent capture"
            );
            anyhow::ensure!(
                status.error.is_none(),
                "transparent capture status is unhealthy"
            );
        }
        let outcome = mutations.patch_clash(patch).await?;
        log_degradations(&outcome);
        Ok(())
    }

    async fn set_tun_mode(
        &self,
        mutations: &dyn ConfigurationMutations,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let outcome = mutations
            .patch_clash(ClashConfigPatch {
                enable_tun_mode: Some(enabled),
                ..Default::default()
            })
            .await?;
        log_degradations(&outcome);
        Ok(())
    }
}

/// The patch that switches transparent capture on, keeping a mode the user
/// already chose, or off.
fn switch_patch(config: &ClashConfig, enabled: bool) -> ClashConfigPatch {
    let mode = if enabled {
        match config.transparent_proxy.mode {
            TransparentProxyMode::Disabled => TransparentProxyMode::Tproxy,
            mode => mode,
        }
    } else {
        TransparentProxyMode::Disabled
    };
    ClashConfigPatch {
        transparent_proxy: TransparentProxyConfigPatch {
            mode: Some(mode),
            ..Default::default()
        },
        tproxy_port: (mode == TransparentProxyMode::Tproxy && config.tproxy_port.is_none())
            .then_some(Some(PortStrategy {
                kind: PortStrategyKind::Fixed,
                start_port: 7894,
            })),
        ..Default::default()
    }
}

/// A committed mutation with a degraded follow-up is not a rejected action.
fn log_degradations(outcome: &MutationOutcome<()>) {
    for degradation in outcome.degradations() {
        tracing::warn!(reason = ?degradation.reason, message = %degradation.message, "a hotkey action committed with a degraded side effect");
    }
}
