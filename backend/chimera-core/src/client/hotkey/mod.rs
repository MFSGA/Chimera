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
use super::effects::{
    plan::EffectKind,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
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
