//! Transitional hotkey adapter for the shared application effect actor.
//!
//! The effects actor owns revision ordering and bounded retries. Other
//! application effect owners are not composed here yet, so requests are
//! filtered to hotkeys at the actor boundary before they reach this adapter.

use std::sync::Arc;

use super::{
    plan::{ApplicationEffect, ApplicationEffectPlan, EffectKind},
    ports::ApplicationEffectsPort,
    status::{EffectHealth, EffectRevision, EffectStatus},
};
use crate::client::hotkey::{
    HotkeyClient,
    ports::{AcceleratorValidator, HotkeyBindings},
};

pub(crate) struct HotkeyEffectExecutor {
    hotkeys: HotkeyClient,
    accelerators: Arc<dyn AcceleratorValidator>,
}

impl HotkeyEffectExecutor {
    pub(crate) fn new(hotkeys: HotkeyClient, accelerators: Arc<dyn AcceleratorValidator>) -> Self {
        Self {
            hotkeys,
            accelerators,
        }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectsPort for HotkeyEffectExecutor {
    async fn apply(
        &self,
        revision: EffectRevision,
        plan: ApplicationEffectPlan,
    ) -> Vec<EffectStatus> {
        let mut statuses = Vec::with_capacity(plan.effects().len());
        for effect in plan.effects() {
            let status = match effect {
                ApplicationEffect::Hotkeys(raw) => {
                    match HotkeyBindings::parse(raw, self.accelerators.as_ref()) {
                        Ok(bindings) => self.hotkeys.reconcile(revision, bindings).await,
                        Err(error) => EffectStatus {
                            kind: EffectKind::Hotkeys,
                            desired_revision: revision,
                            applied_revision: EffectRevision::default(),
                            health: EffectHealth::Degraded {
                                code: "hotkey_invalid_bindings",
                                message: error.to_string(),
                                retryable: false,
                            },
                        },
                    }
                }
                other => EffectStatus {
                    kind: other.kind(),
                    desired_revision: revision,
                    applied_revision: EffectRevision::default(),
                    health: EffectHealth::Unsupported {
                        code: "effect_not_owned_by_hotkey_executor",
                    },
                },
            };
            statuses.push(status);
        }
        statuses
    }

    async fn shutdown(&self) -> Vec<EffectStatus> {
        vec![self.hotkeys.unregister_all().await]
    }
}
