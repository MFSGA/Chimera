//! Adapt ref's pure `plan(Facts)` to the already-established Core host.
//!
//! This is a deliberately bounded migration of S3/S4 into Chimera's sole
//! CoreLifecycle executor. A host handoff is still owned by StartupWorkflow:
//! an unavailable or mismatched host does NOT become an invented ownership
//! proof merely so the planner can advance.
use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{
        self, Attempt, CoreFact, Facts, Ownership, RunIntent, Step, Trigger,
    },
};
use chimera_ipc::api::status::CoreState;

use crate::client::{
    core_lifecycle::ports::CoreStatusSnapshot, reconciler::proof::host_for,
    runtime::RuntimeApplyReceipt,
};

/// Only call the ref planner for a host whose selection is stably observed.
/// A Running instance additionally needs an authoritative active receipt:
/// otherwise Chimera's older host adapter must decide how to recover it.
pub(crate) fn plan_established_target(
    target: &str,
    desired: ExecutionHost,
    observed: &CoreStatusSnapshot,
    active_receipt: Option<&RuntimeApplyReceipt>,
    verified_applied: bool,
    attempt: Option<Attempt>,
    trigger: Trigger,
) -> Option<Step> {
    let owner = host_for(observed.run_type);
    if observed.state_changed_at == 0 || desired != owner {
        return None;
    }
    let core = match &observed.state {
        CoreState::Stopped(reason) => CoreFact::Stopped {
            failed: reason.is_some(),
        },
        CoreState::Running if active_receipt.is_some() => CoreFact::Running {
            verified: verified_applied,
        },
        CoreState::Running => return None,
    };
    let applied = verified_applied
        .then_some(target.to_owned())
        .filter(|_| matches!(core, CoreFact::Running { verified: true }));
    let facts = Facts {
        trigger,
        recovering: false,
        run: RunIntent::Running,
        desired_host: desired,
        owner,
        ownership: Ownership::Established { host: owner },
        service: None,
        core,
        target: target.to_owned(),
        applied,
        attempt,
    };
    Some(plan::plan(&facts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::client::reconciler::plan::{AttemptOutcome, Conclusion, RuntimeError};
    use std::sync::Arc;

    fn running(run_type: crate::core::RunType) -> CoreStatusSnapshot {
        CoreStatusSnapshot {
            instance_id: Some("active".into()),
            state: CoreState::Running,
            state_changed_at: 1,
            run_type,
        }
    }

    fn receipt() -> RuntimeApplyReceipt {
        crate::client::ports::test_runtime_receipt(Default::default())
    }

    #[test]
    fn source_notification_does_not_reapply_verified_committed_target() {
        let step = plan_established_target(
            "same-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            Some(&receipt()),
            true,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(step, Some(Step::Idle(Conclusion::Converged))));
        let retry = plan_established_target(
            "same-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            Some(&receipt()),
            true,
            None,
            Trigger::Retry,
        );
        assert!(matches!(retry, Some(Step::Apply)));
    }

    #[test]
    fn wrong_accepted_target_demands_stop_before_new_apply() {
        let step = plan_established_target(
            "new-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            Some(&receipt()),
            false,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(step, Some(Step::Stop { intent: false })));
    }

    #[test]
    fn host_handoff_or_missing_core_proof_is_left_to_existing_owner() {
        assert!(
            plan_established_target(
                "new",
                ExecutionHost::Service,
                &running(crate::core::RunType::Normal),
                Some(&receipt()),
                false,
                None,
                Trigger::SourceCommitted,
            )
            .is_none()
        );
        assert!(
            plan_established_target(
                "new",
                ExecutionHost::Local,
                &running(crate::core::RunType::Normal),
                None,
                false,
                None,
                Trigger::SourceCommitted,
            )
            .is_none()
        );
        let mut unproven = running(crate::core::RunType::Normal);
        unproven.state_changed_at = 0;
        assert!(
            plan_established_target(
                "new",
                ExecutionHost::Local,
                &unproven,
                Some(&receipt()),
                false,
                None,
                Trigger::SourceCommitted,
            )
            .is_none()
        );
    }

    #[test]
    fn ref_retains_refusal_and_service_fallback_precedence() {
        let rejected = Attempt {
            target: "target".into(),
            host: ExecutionHost::Service,
            outcome: AttemptOutcome::Refused(Arc::new(RuntimeError::new("refused"))),
        };
        let stopped = CoreStatusSnapshot {
            instance_id: Some("service-core".into()),
            state: CoreState::Stopped(None),
            state_changed_at: 3,
            run_type: crate::core::RunType::Service,
        };
        assert!(matches!(
            plan_established_target(
                "target",
                ExecutionHost::Service,
                &stopped,
                None,
                false,
                Some(rejected),
                Trigger::SourceCommitted
            ),
            Some(Step::FallBackToLocal)
        ));
    }
}
