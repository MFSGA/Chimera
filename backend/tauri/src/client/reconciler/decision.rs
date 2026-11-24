//! One ref-shaped Facts assembly boundary for both S2 owner planning and
//! S3/S4 committed-target decisions. The existing CoreLifecycle mailbox is
//! still the sole executor; this module never acquires a lease or mutates Core.
use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{
        self, Attempt, Conclusion, CoreFact, Facts, OwnerPlan, Ownership, RunIntent, RuntimeError,
        ServiceEvidence, Step, Trigger,
    },
};
use chimera_ipc::api::status::CoreState;
use std::sync::Arc;

use crate::client::{
    application_workflow::startup::ServiceEvidence as ObservedServiceEvidence,
    core_lifecycle::ports::CoreStatusSnapshot, reconciler::proof::host_for,
    runtime::RuntimeApplyReceipt,
};

/// A fully observed ref S2/S3/S4 step must not become a legacy Core submit
/// merely because the old workflow does not implement the step. Only Apply
/// explicitly authorizes S4, and StopForReplacement must first prove a
/// terminal Stopped Core. Legacy recovery is reserved for a *missing* plan.
#[derive(Debug)]
pub(crate) enum CommittedStepAction {
    Apply,
    Converged,
    Wait(Arc<RuntimeError>),
    Refused(Arc<RuntimeError>),
    Owner(OwnerPlan),
    StopForReplacement,
    FallBackToLocal,
    Blocked(&'static str),
}

pub(crate) fn classify_committed_step(step: Step) -> CommittedStepAction {
    match step {
        Step::Apply => CommittedStepAction::Apply,
        Step::Idle(Conclusion::Converged) => CommittedStepAction::Converged,
        Step::Idle(Conclusion::Waiting { cause }) => CommittedStepAction::Wait(cause),
        Step::Idle(Conclusion::NotInEffect { cause }) => CommittedStepAction::Refused(cause),
        Step::Owner(plan) => CommittedStepAction::Owner(plan),
        Step::Stop { intent: false } => CommittedStepAction::StopForReplacement,
        Step::FallBackToLocal => CommittedStepAction::FallBackToLocal,
        // The committed planner always requests Running. These cases
        // indicate a stale/unsupported plan, not permission for S4.
        Step::Stop { intent: true } => CommittedStepAction::Blocked(
            "an explicit Stop decision cannot authorize a committed Runtime Apply",
        ),
        Step::Idle(Conclusion::Inactive) => CommittedStepAction::Blocked(
            "an inactive Core plan cannot authorize a committed Runtime Apply",
        ),
        Step::Idle(Conclusion::Unknown { .. }) => CommittedStepAction::Blocked(
            "an unknown Core plan requires recovery before Runtime Apply",
        ),
        Step::Idle(Conclusion::Closed) => CommittedStepAction::Blocked(
            "a closed reconciler cannot authorize a committed Runtime Apply",
        ),
    }
}

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

/// The same accepted receipt (including its absence) must survive the
/// observation round. A newly installed receipt is a different Core fact.
pub(crate) fn same_active_receipt(
    before: Option<&Arc<RuntimeApplyReceipt>>,
    after: Option<&Arc<RuntimeApplyReceipt>>,
) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => Arc::ptr_eq(before, after),
        (None, None) => true,
        _ => false,
    }
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

/// After an S3 terminal, ref converge obtains fresh Core facts before
/// authorizing S4. A stopped RPC reply is insufficient: the selected host
/// must remain stopped, its status generation initialized, and the accepted
/// Running receipt must have been retired. The following S2/plan probe is
/// separate and must also pass; this is only the local S3 terminal boundary.
pub(crate) fn s3_replanned_stopped_owner_ready(
    observed: &CoreStatusSnapshot,
    active_receipt: Option<&RuntimeApplyReceipt>,
    desired_host: ExecutionHost,
) -> bool {
    matches!(observed.state, CoreState::Stopped(_))
        && observed.state_changed_at > 0
        && active_receipt.is_none()
        && host_for(observed.run_type) == desired_host
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

/// S2-proven Service ownership must also describe the *same* selected Core
/// lifecycle state before a prepared product is admitted. A synthetic
/// timestamp-zero Stopped status or a ready but foreign daemon is not proof.
pub(crate) fn service_prepared_owner_proven(
    observed: &CoreStatusSnapshot,
    service: &ObservedServiceEvidence,
) -> bool {
    if observed.run_type != crate::core::RunType::Service || observed.state_changed_at == 0 {
        return false;
    }
    match (service, &observed.state) {
        (
            ObservedServiceEvidence::CoreStopped {
                ready: true,
                runtime_owned: true,
                ..
            },
            CoreState::Stopped(_),
        ) => true,
        (
            ObservedServiceEvidence::CoreRunning {
                ready: true,
                instance,
                ..
            },
            CoreState::Running,
        ) => instance
            .as_ref()
            .is_none_or(|expected| observed.instance_id.as_deref() == Some(expected.as_str())),
        _ => false,
    }
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

/// S2 then S3/S4 in one ref Facts round. Returns None only when the
/// compatibility status has not been initialized. As in ref, an observed
/// Running Core without a confirmed receipt is Running { verified: false },
/// not Unreadable: S2 must establish ownership before S3 can stop it.
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
    let core = match &observed.state {
        CoreState::Stopped(reason) => CoreFact::Stopped {
            failed: reason.is_some(),
        },
        CoreState::Running => CoreFact::Running {
            verified: active_receipt.is_some() && verified_applied,
        },
    };
    // A verified Running receipt establishes its *own* applied target,
    // never the desired source target merely because the Core is healthy.
    // Otherwise an older/newer active configuration can be reported as
    // Converged for a different committed document without any S4 apply.
    let applied = if verified_applied && matches!(core, CoreFact::Running { verified: true }) {
        active_receipt.and_then(|receipt| receipt.target.as_deref())
    } else {
        None
    };
    Some(
        FactFrame {
            trigger,
            recovering,
            desired,
            owner,
            service,
        }
        .plan(core, target, applied, attempt),
    )
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
    fn explicit_ref_plan_actions_never_fall_through_to_core_apply() {
        let cause = Arc::new(RuntimeError::new("committed target was refused"));
        assert!(matches!(
            classify_committed_step(Step::Apply),
            CommittedStepAction::Apply
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::Converged)),
            CommittedStepAction::Converged
        ));
        assert!(matches!(
            classify_committed_step(Step::Stop { intent: false }),
            CommittedStepAction::StopForReplacement
        ));
        assert!(matches!(
            classify_committed_step(Step::FallBackToLocal),
            CommittedStepAction::FallBackToLocal
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::NotInEffect {
                cause: cause.clone(),
            })),
            CommittedStepAction::Refused(actual) if Arc::ptr_eq(&actual, &cause)
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::Waiting {
                cause: cause.clone(),
            })),
            CommittedStepAction::Wait(actual) if Arc::ptr_eq(&actual, &cause)
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::Unknown {
                cause: cause.clone(),
            })),
            CommittedStepAction::Blocked(_)
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::Inactive)),
            CommittedStepAction::Blocked(_)
        ));
        assert!(matches!(
            classify_committed_step(Step::Idle(Conclusion::Closed)),
            CommittedStepAction::Blocked(_)
        ));
        assert!(matches!(
            classify_committed_step(Step::Stop { intent: true }),
            CommittedStepAction::Blocked(_)
        ));
    }

    #[test]
    fn observed_refusal_is_a_non_submittable_committed_step() {
        let refused = Arc::new(RuntimeError::new("original ref refusal"));
        let current = CoreStatusSnapshot {
            instance_id: Some("local-runtime".into()),
            state: CoreState::Stopped(None),
            state_changed_at: 18,
            run_type: crate::core::RunType::Normal,
        };
        let planned = plan_committed_target(
            "refused-target",
            ExecutionHost::Local,
            &current,
            absent(),
            false,
            None,
            false,
            Some(Attempt {
                target: "refused-target".into(),
                host: ExecutionHost::Local,
                outcome: AttemptOutcome::Refused(refused.clone()),
            }),
            Trigger::SourceCommitted,
        )
        .expect("stable stopped Local runtime must give a definite ref plan");
        assert!(matches!(
            classify_committed_step(planned),
            CommittedStepAction::Refused(actual) if Arc::ptr_eq(&actual, &refused)
        ));
        let retry = plan_committed_target(
            "refused-target",
            ExecutionHost::Local,
            &current,
            absent(),
            false,
            None,
            false,
            Some(Attempt {
                target: "refused-target".into(),
                host: ExecutionHost::Local,
                outcome: AttemptOutcome::Refused(refused),
            }),
            Trigger::Retry,
        );
        assert!(matches!(
            retry.map(classify_committed_step),
            Some(CommittedStepAction::Apply)
        ));
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
    fn stopped_core_after_s3_requires_retired_receipt_and_same_owner() {
        let stopped = CoreStatusSnapshot {
            instance_id: Some("same-instance".into()),
            state: CoreState::Stopped(None),
            state_changed_at: 2,
            run_type: crate::core::RunType::Normal,
        };
        assert!(s3_replanned_stopped_owner_ready(
            &stopped,
            None,
            ExecutionHost::Local,
        ));
        assert!(!s3_replanned_stopped_owner_ready(
            &stopped,
            Some(&receipt()),
            ExecutionHost::Local,
        ));
        assert!(!s3_replanned_stopped_owner_ready(
            &stopped,
            None,
            ExecutionHost::Service,
        ));
        assert!(!s3_replanned_stopped_owner_ready(
            &CoreStatusSnapshot {
                state: CoreState::Running,
                ..stopped.clone()
            },
            None,
            ExecutionHost::Local,
        ));
        assert!(!s3_replanned_stopped_owner_ready(
            &CoreStatusSnapshot {
                state_changed_at: 0,
                ..stopped
            },
            None,
            ExecutionHost::Local,
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
    fn receipt_observation_fences_both_presence_and_absence() {
        let accepted = Arc::new(receipt());
        let replacement = Arc::new(receipt());
        assert!(same_active_receipt(None, None));
        assert!(same_active_receipt(Some(&accepted), Some(&accepted)));
        assert!(!same_active_receipt(None, Some(&accepted)));
        assert!(!same_active_receipt(Some(&accepted), None));
        assert!(!same_active_receipt(Some(&accepted), Some(&replacement)));
    }

    #[test]
    fn source_notification_does_not_reapply_verified_committed_target() {
        let mut accepted = receipt();
        accepted.target = Some("same-target".into());
        let step = plan_committed_target(
            "same-target",
            ExecutionHost::Local,
            &running(crate::core::RunType::Normal),
            absent(),
            false,
            Some(&accepted),
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
            Some(&accepted),
            true,
            None,
            Trigger::Retry,
        );
        assert!(matches!(retry, Some(Step::Apply)));
    }

    #[test]
    fn verified_core_does_not_converge_a_different_committed_target() {
        let mut accepted = receipt();
        accepted.target = Some("old-target".into());
        let current = running(crate::core::RunType::Normal);
        let changed = plan_committed_target(
            "new-target",
            ExecutionHost::Local,
            &current,
            absent(),
            false,
            Some(&accepted),
            true,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(changed, Some(Step::Apply)));
        accepted.target = None;
        let unbound = plan_committed_target(
            "new-target",
            ExecutionHost::Local,
            &current,
            absent(),
            false,
            Some(&accepted),
            true,
            None,
            Trigger::SourceCommitted,
        );
        assert!(matches!(unbound, Some(Step::Apply)));
    }

    #[test]
    fn ref_startup_start_and_core_changed_use_the_same_running_receipt_facts() {
        use crate::client::reconciler::proof::receipt_matches_owner;
        let mut accepted = receipt();
        accepted.target = Some("previous-target".into());
        let observed = running(crate::core::RunType::Normal);
        let verified = receipt_matches_owner(&accepted, ExecutionHost::Local);
        assert!(verified);
        for trigger in [
            Trigger::Startup,
            Trigger::Start,
            Trigger::CoreChanged,
            Trigger::SourceCommitted,
            Trigger::Retry,
        ] {
            assert!(matches!(
                plan_committed_target(
                    "new-target",
                    ExecutionHost::Local,
                    &observed,
                    absent(),
                    false,
                    Some(&accepted),
                    verified,
                    None,
                    trigger
                ),
                Some(Step::Apply)
            ));
        }
        // Startup recovery and CoreChanged do not replay an already
        // converged target. Explicit Start remains forced.
        for trigger in [Trigger::Startup, Trigger::CoreChanged] {
            assert!(matches!(
                plan_committed_target(
                    "previous-target",
                    ExecutionHost::Local,
                    &observed,
                    absent(),
                    false,
                    Some(&accepted),
                    verified,
                    None,
                    trigger,
                ),
                Some(Step::Idle(Conclusion::Converged))
            ));
        }
        // Explicit Start keeps the upstream forced-reconcile semantics.
        assert!(matches!(
            plan_committed_target(
                "previous-target",
                ExecutionHost::Local,
                &observed,
                absent(),
                false,
                Some(&accepted),
                verified,
                None,
                Trigger::Start
            ),
            Some(Step::Apply)
        ));
        // Only an actually unverified running instance requires S3 retirement.
        assert!(matches!(
            plan_committed_target(
                "new-target",
                ExecutionHost::Local,
                &observed,
                absent(),
                false,
                None,
                false,
                None,
                Trigger::CoreChanged
            ),
            Some(Step::Stop { intent: false })
        ));
    }

    #[test]
    fn unverified_accepted_receipt_demands_stop_before_new_apply() {
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
    fn recovery_blocks_apply_but_observed_unverified_running_requires_stop() {
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
        assert!(matches!(
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
            ),
            Some(Step::Stop { intent: false })
        ));
        assert!(matches!(
            plan_committed_target(
                "next",
                ExecutionHost::Local,
                &current,
                owned_service(),
                false,
                None,
                false,
                None,
                Trigger::SourceCommitted,
            ),
            Some(Step::Owner(OwnerPlan::RetireServiceThenLocal))
        ));
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
