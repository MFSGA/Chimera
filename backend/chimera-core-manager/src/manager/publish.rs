use crate::{
    epoch::Epoch,
    error::Error,
    runtime::RuntimeInstance,
    state::{
        CoreState, CoreStatus, HealthStatus, InstanceState, InstanceStatus, SpecSummary,
        StopReason, now_ms,
    },
};

use super::{Active, CoreManager, Ctrl, EpochPlan, Inner};

impl Inner {
    /// `None` publishes a state with no epoch behind it (stopped, quarantined).
    pub(super) fn publish(&self, state: CoreState, plan: Option<&EpochPlan>) {
        self.status_tx.send_modify(|status| {
            let lifecycle_changed = status.state != state;
            let health = default_health_for_state(status.health.as_ref(), &state);
            status.state = state;
            status.instance_id = None;
            status.health = health;
            status.spec = plan.map(spec_summary);
            status.controller = plan.map(|plan| plan.controller.host.clone());
            status.revision = plan.map(|plan| plan.revision.clone());
            if lifecycle_changed {
                status.changed_at = now_ms();
            }
        });
    }

    pub(super) fn publish_active(&self, active: &Active, state: CoreState) {
        self.publish_instance(active.instance.as_ref(), state, &active.plan);
    }

    /// Health and controller still come from the live instance; the epoch's own
    /// description comes from its plan.
    pub(super) fn publish_instance(
        &self,
        instance: &dyn RuntimeInstance,
        state: CoreState,
        plan: &EpochPlan,
    ) {
        let snapshot = instance.state().borrow().clone();
        if matches!(state, CoreState::Running { .. })
            && let Some(instance_id) = snapshot.instance_id
        {
            self.config_commits
                .send_replace(Some(crate::EffectiveConfigSnapshot {
                    instance_id,
                    revision: plan.revision.clone(),
                    config: std::sync::Arc::new(plan.effective_document.clone()),
                }));
        }
        let health = snapshot.health;
        self.status_tx.send_modify(|status| {
            let lifecycle_changed = status.state != state;
            status.state = state;
            status.instance_id = snapshot.instance_id;
            status.health = health;
            status.spec = Some(spec_summary(plan));
            status.controller = Some(instance.controller().host.clone());
            status.revision = Some(plan.revision.clone());
            if lifecycle_changed {
                status.changed_at = now_ms();
            }
        });
    }

    pub(super) fn publish_epoch_status(&self, epoch: Epoch, instance: InstanceStatus) {
        self.status_tx
            .send_if_modified(|status| apply_epoch_status(status, epoch, &instance));
    }
}

fn default_health_for_state(
    previous: Option<&HealthStatus>,
    state: &CoreState,
) -> Option<HealthStatus> {
    let target = match state {
        CoreState::Starting { .. } | CoreState::Restarting { .. } | CoreState::Switching { .. } => {
            crate::state::HealthState::Starting
        }
        CoreState::Running { .. } => crate::state::HealthState::Healthy,
        CoreState::Stopping { .. } | CoreState::Stopped { .. } => return None,
    };
    let mut health = HealthStatus::starting();
    health.state = target;
    if let Some(previous) = previous.filter(|status| status.state == target) {
        health.changed_at = previous.changed_at;
        health.consecutive_failures = previous.consecutive_failures;
        health.last_error.clone_from(&previous.last_error);
        health.last_success_at = previous.last_success_at;
    }
    Some(health)
}

fn apply_epoch_status(status: &mut CoreStatus, epoch: Epoch, instance: &InstanceStatus) -> bool {
    if status.revision.as_ref().map(|revision| revision.epoch) != Some(epoch) {
        return false;
    }
    let state = instance_core_state(epoch, &instance.state);
    let lifecycle_changed = status.state != state;
    let health_changed = status.health != instance.health;
    if !lifecycle_changed && !health_changed && status.instance_id == instance.instance_id {
        return false;
    }
    status.state = state;
    status.health = instance.health.clone();
    status.instance_id = instance.instance_id;
    if lifecycle_changed {
        status.changed_at = now_ms();
    }
    true
}

fn spec_summary(plan: &EpochPlan) -> SpecSummary {
    SpecSummary {
        kind: plan.source_spec.core.kind,
        config_path: plan.source_spec.config_path.clone(),
        capabilities: plan.capabilities.iter().collect(),
        runtime_features: plan.runtime_features.iter().collect(),
    }
}

impl CoreManager {
    pub(super) fn publish_terminal_error(&self, error: &Error) {
        self.inner.publish(
            CoreState::Stopped {
                reason: Some(StopReason::Error(error.to_string())),
            },
            None,
        );
    }

    pub(super) fn republish_retained(&self, ctrl: &Ctrl) {
        let Some(active) = ctrl.current.as_ref() else {
            return;
        };
        let state = instance_core_state(
            active.instance.epoch(),
            &active.instance.state().borrow().state,
        );
        self.inner.publish_active(active, state);
    }
}

pub(super) fn instance_core_state(epoch: Epoch, state: &InstanceState) -> CoreState {
    match state {
        InstanceState::Starting => CoreState::Starting { epoch },
        InstanceState::Running { pid } => CoreState::Running { epoch, pid: *pid },
        InstanceState::Restarting { attempt } => CoreState::Restarting {
            epoch,
            attempt: *attempt,
        },
        InstanceState::Stopping => CoreState::Stopping { epoch },
        InstanceState::Stopped(reason) => CoreState::Stopped {
            reason: Some(reason.clone()),
        },
    }
}
