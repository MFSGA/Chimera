//! Global shortcuts, owned by one actor behind the application client.
//!
//! The registrar only handles OS registration. Pressed actions return through
//! a channel and are applied by the shared Chimera client, outside the OS
//! callback.

mod actor;
pub mod adapters;
pub mod ports;

use std::{collections::BTreeMap, time::Duration};

use anyhow::Result;
use chimera_config::clash::config::overrides::Mode;
use ractor::{Actor, ActorRef, rpc::CallResult};

use self::{
    actor::{HotkeyActor, Message},
    ports::{HotkeyAction, HotkeyBindings},
};
use super::{
    ChimeraClient,
    effects::{
        plan::{ApplicationEffectInputs, EffectKind},
        status::{EffectHealth, EffectRevision, EffectStatus},
    },
};

pub use self::actor::Args as HotkeyArgs;

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

pub(crate) fn validate_bindings(
    raw: &[String],
    accelerators: &dyn ports::AcceleratorValidator,
) -> Result<HotkeyBindings> {
    Ok(HotkeyBindings::parse(raw, accelerators)?)
}

impl ChimeraClient {
    pub(crate) async fn set_hotkeys(
        &self,
        hotkeys: Vec<String>,
    ) -> Result<super::MutationOutcome<()>> {
        let _guard = self.inner.hotkey_mutation.lock().await;
        validate_bindings(&hotkeys, self.inner.accelerators.as_ref())?;
        self.inner
            .application
            .patch_typed(chimera_config::application::ChimeraAppConfigPatch {
                hotkeys: Some(hotkeys),
                ..Default::default()
            })
            .await?;
        let mut degradations = Vec::new();
        if let Err(error) = crate::config::core::Config::verge().data().save_file() {
            degradations.push(super::Degradation {
                phase: super::DegradationPhase::LegacyMirror,
                code: "legacy_hotkey_mirror_save_failed".into(),
                message: error.to_string(),
                retryable: false,
            });
        }
        crate::core::handle::Handle::refresh_verge();

        let status = self.reconcile_hotkeys().await;
        degradations.extend(super::effects::status::degradation_of(&status));
        Ok(super::MutationOutcome::from_parts((), degradations))
    }

    /// Runs a full hotkey reconcile through the shared application effect actor.
    pub(crate) async fn reconcile_hotkeys(&self) -> EffectStatus {
        let inputs = self.get_app_config().and_then(|application| {
            self.get_clash_config()
                .map(|clash| ApplicationEffectInputs::project(&application, &clash, None))
        });
        let status = match inputs {
            Ok(inputs) => self.inner.effects.reconcile_hotkeys(inputs).await,
            Err(error) => EffectStatus {
                kind: EffectKind::Hotkeys,
                desired_revision: EffectRevision::default(),
                applied_revision: EffectRevision::default(),
                health: EffectHealth::Degraded {
                    code: "hotkey_config_read_failed",
                    message: format!("{error:#}"),
                    retryable: false,
                },
            },
        };
        if let EffectHealth::Degraded { code, message, .. } = &status.health {
            tracing::warn!(%code, %message, "saved global shortcuts did not fully register");
        }
        status
    }

    /// Applies a pressed shortcut through the same Chimera mutation paths used
    /// by the settings UI. The OS callback never invokes this method directly.
    pub(crate) async fn dispatch_hotkey_action(&self, action: HotkeyAction) -> Result<()> {
        match action {
            HotkeyAction::OpenOrCloseDashboard => {
                self.inner.window.toggle_dashboard().await?;
                Ok(())
            }
            HotkeyAction::ClashModeRule => self.set_clash_mode(Mode::Rule).await,
            HotkeyAction::ClashModeGlobal => self.set_clash_mode(Mode::Global).await,
            HotkeyAction::ClashModeDirect => self.set_clash_mode(Mode::Direct).await,
            HotkeyAction::ClashModeScript => self.set_clash_mode(Mode::Script).await,
            HotkeyAction::ToggleSystemProxy => {
                let enabled = self.get_app_config()?.enable_system_proxy;
                self.set_system_proxy(!enabled).await
            }
            HotkeyAction::EnableSystemProxy => self.set_system_proxy(true).await,
            HotkeyAction::DisableSystemProxy => self.set_system_proxy(false).await,
            HotkeyAction::ToggleTunMode => {
                let enabled = self.get_clash_config()?.enable_tun_mode;
                self.set_tun_mode(!enabled).await
            }
            HotkeyAction::EnableTunMode => self.set_tun_mode(true).await,
            HotkeyAction::DisableTunMode => self.set_tun_mode(false).await,
        }
    }

    async fn set_clash_mode(&self, mode: Mode) -> Result<()> {
        self.patch_clash_overrides(crate::config::runtime::ClashConfigOverrides {
            mode: Some(mode.to_string()),
            ..Default::default()
        })
        .await
    }

    async fn set_system_proxy(&self, enabled: bool) -> Result<()> {
        self.patch_verge(crate::config::chimera::IVerge {
            enable_system_proxy: Some(enabled),
            ..Default::default()
        })
        .await
    }

    async fn set_tun_mode(&self, enabled: bool) -> Result<()> {
        self.patch_verge(crate::config::chimera::IVerge {
            enable_tun_mode: Some(enabled),
            ..Default::default()
        })
        .await
    }
}
