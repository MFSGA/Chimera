//! One ref-shaped Facts assembly boundary for both S2 owner planning and
//! S3/S4 committed-target decisions. The existing CoreLifecycle mailbox is
//! still the sole executor; this module never acquires a lease or mutates Core.
use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{
        self, Attempt, CoreFact, Facts, OwnerPlan, Ownership, RunIntent, ServiceEvidence, Step,
        Trigger,
    },
};
use chimera_ipc::api::status::CoreState;

use crate::client::{
    application_workflow::startup::ServiceEvidence as ObservedServiceEvidence,
    core_lifecycle::ports::CoreStatusSnapshot, reconciler::proof::host_for,
    runtime::RuntimeApplyReceipt,
};

/// The same Core instance must still own the selected endpoint after an
/// asynchronous ServiceActor probe. Merely observing Running both times is
/// not a proof of the same selected Core.
pub(crate) fn same_core_status(left: &CoreStatusSnapshot, right: &CoreStatusSnapshot) -> bool {
    let same_state = match (&left.state, &right.state) {
        (CoreState::Running, CoreState::Running) => true,
        (CoreState::Stopped(left_reason), CoreState::Stopped(right_reason)) => {
            left_reason == right_reason
        }
        _ => false,
    };
    left.instance_id == right.instance_id
        && same_state
        && left.state_changed_at == right.state_changed_at
        && left.run_type == right.run_type
}

/// Ref stop_owner does not treat a successful Stop RPC as a settled
/// terminal. S4 may start only after observing the same selected host in
/// a real stopped state, not another Running Core or an uninitialized status.
pub(crate) fn confirmed_planned_stop(
    before: &CoreStatusSnapshot,
    after: &CoreStatusSnapshot,
) -> bool {
    matches!(before.state, CoreState::Running)
        && before.run_type == after.run_type
        && after.state_changed_at > 0
        && matches!(after.state, CoreState::Stopped(_))
}

/// One coherent ref Facts sample for S2 ownership and S3/S4 decisions.
/// ServiceActor's Ready generation is authoritative, but the same generation
/// alone does not prove unchanged ownership while a daemon transitions.
/// Compare the full typed evidence and the selected Core identity as well.
/// This is read-only; acquiring a transition lease and mutating hosts remain
/// responsibilities of the single CoreLifecycle actor.
pub(crate) fn stable_owner_facts(
    before_core: &CoreStatusSnapshot,
    after_core: &CoreStatusSnapshot,
    before_service: &ObservedServiceEvidence,
    before_generation: u64,
    after_service: &ObservedServiceEvidence,
    after_generation: u64,
) -> bool {
    same_core_status(before_core, after_core)
        && before_generation == after_generation
        && before_service == after_service
}

/// The reusable ref Facts frame. In S2 the Core fact stays Unreadable and
/// ownership Unproven; in a committed round Core facts are sampled only after
/// the selected endpoint has supplied a stable observation.
struct FactFrame {
    trigger: Trigger,
    recovering: bool,
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: ServiceEvidence,
}

impl FactFrame {
    fn plan(
        self,
        core: CoreFact,
        target: &str,
        applied: Option<&str>,
        attempt: Option<Attempt>,
    ) -> Step {
        plan::plan(&Facts {
            trigger: self.trigger,
            recovering: self.recovering,
            run: RunIntent::Running,
            desired_host: self.desired,
            owner: self.owner,
            ownership: Ownership::Unproven,
            service: Some(self.service),
            core,
            target: target.to_owned(),
            applied: applied.map(ToOwned::to_owned),
            attempt,
        })
    }
}

/// S2's sole ownership decision. Even when plan_owner returns Owned, this
/// S2-only call cannot cause Core Apply/Stop: the core is explicitly unreadable.
pub(crate) fn plan_owner_boundary(
    desired: ExecutionHost,
    owner: ExecutionHost,
    service: ServiceEvidence,
    recovering: bool,
) -> Step {
    FactFrame {
        trigger: Trigger::ServiceReadiness,
        recovering,
        desired,
        owner,
        service,
    }
    .plan(CoreFact::Unreadable, "", None, None)
}

/// S2 then S3/S4 in one ref Facts round. Returns None when there is no stable
/// current Core or an unverified Running Core would otherwise be misclassified
/// as a new target: the existing recovery adapter must handle that case.
///
/// A Service owner action or Waiting on ownership evidence is returned as an
/// explicit step and must NOT be treated as permission to submit Core.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_committed_target(
    target: &str,
    desired: ExecutionHost,
    observed: &CoreStatusSnapshot,
    service: ServiceEvidence,
    recovering: bool,
    active_receipt: Option<&RuntimeApplyReceipt>,
    verified_applied: bool,
    attempt: Option<Attempt>,
    trigger: Trigger,
) -> Option<Step> {
    if observed.state_changed_at == 0 {
        return None;
    }
    let owner = host_for(observed.run_type);
    let owner_plan = plan::plan_owner(desired, owner, &service);
    let core = match &observed.state {
        CoreState::Stopped(reason) => CoreFact::Stopped {
            failed: reason.is_some(),
        },
        CoreState::Running if active_receipt.is_some() => CoreFact::Running {
            verified: verified_applied,
        },
        // Unverified Running is never enough for an S3/S4 stop. We still
        // execute S2 to detect a distinct owned Service Core or handoff.
        CoreState::Running => CoreFact::Unreadable,
    };
    let applied = (verified_applied && matches!(core, CoreFact::Running { verified: true }))
        .then_some(target);
    let step = FactFrame {
        trigger,
        recovering,
        desired,
        owner,
        service,
    }
    .plan(core, target, applied, attempt);
    // S2 established ownership but S3 cannot safely infer the active Core
    // target: leave it to Chimera's existing Core receipt recovery path.
    if matches!(owner_plan, OwnerPlan::Owned) && matches!(core, CoreFact::Unreadable) && !recovering
    {
        None
    } else {
        Some(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::{
        actor_v2::service_policy::ServicePhase,
        client::reconciler::plan::{AttemptOutcome, Conclusion, RuntimeError},
    };
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

    fn absent() -> ServiceEvidence {
        ServiceEvidence::Absent {
            phase: ServicePhase::DaemonStopped,
        }
    }

    fn owned_service() -> ServiceEvidence {
        ServiceEvidence::CoreRunning {
            ready: true,
            phase: ServicePhase::Ready,
            instance: Some("owned".into()),
            revision: None,
        }
    }

    #[test]
    fn ref_facts_require_same_service_generation_owner_and_core_instance() {
        let original = running(crate::core::RunType::Normal);
        let service = ObservedServiceEvidence::CoreStopped {
            ready: true,
            phase: ServicePhase::Ready,
            runtime_owned: true,
        };
        assert!(stable_owner_facts(
            &original, &original, &service, 7, &service, 7
        ));
        assert!(!stable_owner_facts(
            &original, &original, &service, 7, &service, 8
        ));

        let replacement = CoreStatusSnapshot {
            instance_id: Some("another-instance".into()),
            ..original.clone()
        };
        assert!(!stable_owner_facts(
            &original,
            &replacement,
            &service,
            7,
            &service,
            7
        ));
        let different_generation = CoreStatusSnapshot {
            state_changed_at: 2,
            ..original.clone()
        };
        assert!(!stable_owner_facts(
            &original,
            &different_generation,
            &service,
            7,
            &service,
            7
        ));
        let foreign = ObservedServiceEvidence::ForeignCore {
            ready: true,
            phase: ServicePhase::Ready,
        };
        assert!(!stable_owner_facts(
            &original, &original, &service, 7, &foreign, 7
        ));
        let unreadable = ObservedServiceEvidence::Unreadable {
            phase: ServicePhase::Unknown,
        };
        assert!(!stable_owner_facts(
            &original,
            &original,
            &service,
            7,
            &unreadable,
            7
        ));
    }

    #[test]
    fn ref_s3_requires_a_confirmed_terminal_before_s4_apply() {
        let before = running(crate::core::RunType::Normal);
        let stopped = CoreStatusSnapshot {
            state: CoreState::Stopped(None),
            state_changed_at: 2,
            ..before.clone()
        };
        assert!(confirmed_planned_stop(&before, &stopped));
        assert!(!confirmed_planned_stop(&before, &before));
        assert!(!confirmed_planned_stop(
            &before,
            &CoreStatusSnapshot {
                state_changed_at: 0,
                ..stopped.clone()
            }
        ));
        assert!(!confirmed_planned_stop(
            &before,
            &CoreStatusSnapshot {
                run_type: crate::core::RunType::Service,
                ..stopped.clone()
            }
        ));
        assert!(!confirmed_planned_stop(&stopped, &stopped));
    }

    #[test]
    fn source_notification_does_not_reapply_verified_committed_target() {
        let step = plan_committed_target(
            "same-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            absent(),
            false,
            Some(&receipt()),
            true,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(step, Some(Step::Idle(Conclusion::Converged))));
        let retry = plan_committed_target(
            "same-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            absent(),
            false,
            Some(&receipt()),
            true,
            None,
            Trigger::Retry,
        );
        assert!(matches!(retry, Some(Step::Apply)));
    }

    #[test]
    fn wrong_accepted_target_demands_stop_before_new_apply() {
        let step = plan_committed_target(
            "new-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            absent(),
            false,
            Some(&receipt()),
            false,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(step, Some(Step::Stop { intent: false })));
    }

    #[test]
    fn source_round_first_checks_s2_before_s3_core_mutations() {
        let current = running(crate::core::RunType::Normal);
        let handoff = plan_committed_target(
            "new",
            ExecutionHost::Service,
            &current,
            ServiceEvidence::CoreStopped {
                ready: true,
                phase: ServicePhase::Ready,
            },
            false,
            Some(&receipt()),
            false,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(
            handoff,
            Some(Step::Owner(OwnerPlan::AdoptService))
        ));
        let second_owner = plan_committed_target(
            "new",
            ExecutionHost::Local,
            &current,
            owned_service(),
            false,
            Some(&receipt()),
            false,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(
            second_owner,
            Some(Step::Owner(OwnerPlan::RetireServiceThenLocal))
        ));
        let unreadable = ServiceEvidence::Unreadable {
            phase: ServicePhase::Unknown,
        };
        let wait = plan_committed_target(
            "new",
            ExecutionHost::Local,
            &current,
            unreadable,
            false,
            Some(&receipt()),
            false,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(wait, Some(Step::Idle(Conclusion::Waiting { .. }))));
    }

    #[test]
    fn recovery_and_unverified_owner_cannot_authorize_apply_or_stop() {
        let current = running(crate::core::RunType::Normal);
        assert!(matches!(
            plan_committed_target(
                "next",
                ExecutionHost::Local,
                &current,
                owned_service(),
                true,
                Some(&receipt()),
                false,
                None,
                Trigger::SourceCommitted,
            ),
            Some(Step::Idle(Conclusion::Waiting { .. }))
        ));
        assert!(
            plan_committed_target(
                "next",
                ExecutionHost::Local,
                &current,
                absent(),
                false,
                None,
                false,
                None,
                Trigger::SourceCommitted,
            )
            .is_none()
        );
        let mut unproven = current;
        unproven.state_changed_at = 0;
        assert!(
            plan_committed_target(
                "next",
                ExecutionHost::Local,
                &unproven,
                owned_service(),
                false,
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
            plan_committed_target(
                "target",
                ExecutionHost::Service,
                &stopped,
                ServiceEvidence::CoreStopped {
                    ready: true,
                    phase: ServicePhase::Ready
                },
                false,
                None,
                false,
                Some(rejected),
                Trigger::SourceCommitted,
            ),
            Some(Step::FallBackToLocal)
        ));
    }
}
