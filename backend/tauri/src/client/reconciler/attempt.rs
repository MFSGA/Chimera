//! Ref RuntimeReconciler attempt policy scoped to the committed target and host.
//!
//! The CoreLifecycle actor remains the sole runtime executor. This helper
//! doesn't submit, stop, or select Core hosts: it only remembers one attempted
//! target and prevents an ordinary notification from replaying a known refusal.
use std::sync::Arc;

use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{
        Attempt, AttemptOutcome, RuntimeError, Trigger, runtime_apply_error,
    },
};
use chimera_core_manager::{CoreError, CoreErrorKind};

/// The ref planner selected S2 ownership work or a dependency wait. A
/// committed source stays durable; the single lifecycle actor must complete
/// its existing Service transition before a Core apply can be attempted.
#[derive(Debug, thiserror::Error)]
#[error("runtime ownership is awaiting S2 reconciliation: {reason}")]
pub(crate) struct OwnershipTransitionPending {
    pub reason: String,
    /// Only a concrete ref S2 owner step authorizes scheduling a transition.
    /// A generic Waiting observation must not acquire a Service lease.
    pub plan: Option<chimera_core::client::reconciler::plan::OwnerPlan>,
}

impl OwnershipTransitionPending {
    pub(crate) fn waiting(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            plan: None,
        }
    }

    pub(crate) fn owner(plan: chimera_core::client::reconciler::plan::OwnerPlan) -> Self {
        Self {
            reason: format!(
                "ref S2 selected {plan:?}; the lifecycle actor must acquire the Service handoff lease"
            ),
            plan: Some(plan),
        }
    }
}

/// A rejected Service application can fall back only after the old owner
/// has been safely released. This is a dependency wait, not a refused Local
/// target and not a reason to undo a durable source commit.
#[derive(Debug, thiserror::Error)]
#[error("Service-to-Local fallback is waiting for ownership proof: {reason}")]
pub(crate) struct ServiceFallbackDeferred {
    pub reason: String,
}

#[derive(Debug, thiserror::Error)]
#[error("committed runtime target changed while releasing Service; reconcile the latest source")]
pub(crate) struct CommittedTargetAdvanced;

/// A target refusal is not a request to roll back its durable source commit.
#[derive(Debug, thiserror::Error)]
#[error(
    "committed runtime target was previously refused on {host:?}: {cause}; use explicit Retry/Start or change the target"
)]
pub(crate) struct PreviousRefusal {
    host: ExecutionHost,
    cause: Arc<RuntimeError>,
}

#[derive(Default)]
pub(crate) struct AttemptPolicy {
    last: Option<Attempt>,
}

impl AttemptPolicy {
    /// Snapshot the previous target attempt without holding the policy lock
    /// across Core status or owner-proof await points.
    pub(crate) fn snapshot(&self) -> Option<Attempt> {
        self.last.clone()
    }

    pub(crate) fn allow(
        &self,
        target: &str,
        host: ExecutionHost,
        trigger: Trigger,
    ) -> Result<(), PreviousRefusal> {
        if trigger.forced() {
            return Ok(());
        }
        let Some(previous) = self
            .last
            .as_ref()
            .filter(|previous| previous.target == target && previous.host == host)
        else {
            return Ok(());
        };
        if let AttemptOutcome::Refused(cause) = &previous.outcome {
            return Err(PreviousRefusal {
                host,
                cause: Arc::clone(cause),
            });
        }
        Ok(())
    }

    /// A transiently unavailable host isn't a refused target. The upstream
    /// planner waits for a changed dependency or user retry in that case.
    pub(crate) fn record(
        &mut self,
        target: String,
        host: ExecutionHost,
        result: &anyhow::Result<()>,
        uncertain: bool,
    ) {
        let outcome = match legacy_apply_verdict(result, uncertain, false) {
            LegacyApplyVerdict::Applied => AttemptOutcome::Applied,
            LegacyApplyVerdict::Refused(cause) => AttemptOutcome::Refused(cause),
            LegacyApplyVerdict::Unknown(cause) => AttemptOutcome::Unknown(cause),
            LegacyApplyVerdict::Waiting(_) => {
                // Ref Waiting does not poison the target with a refusal;
                // the next dependency observation can retry this source.
                self.last = None;
                return;
            }
        };
        self.last = Some(Attempt {
            target,
            host,
            outcome,
        });
    }
}

/// Ref ApplyOutcome's four-way verdict projected onto the old CoreLifecycle
/// port. Legacy commands return Result<()> rather than an AppliedCandidate;
/// `Applied` here means only that the submit call returned successfully.
/// The Actor still requires an independent Running/receipt proof before
/// publishing Converged. Never manufacture a product, Core revision or host.
#[derive(Debug, Clone)]
pub(crate) enum LegacyApplyVerdict {
    Applied,
    Refused(Arc<RuntimeError>),
    Waiting(Arc<RuntimeError>),
    Unknown(Arc<RuntimeError>),
}

/// Classify an already-observed error without stripping the original Core
/// Manager kind or turning an ownership-dependency wait into a refusal.
pub(crate) fn legacy_error_verdict(
    error: &anyhow::Error,
    uncertain: bool,
    source_advanced: bool,
) -> LegacyApplyVerdict {
    if uncertain {
        LegacyApplyVerdict::Unknown(Arc::new(runtime_apply_error(error, true)))
    } else if source_advanced || waiting_dependency(error) {
        LegacyApplyVerdict::Waiting(Arc::new(runtime_apply_error(error, false)))
    } else {
        LegacyApplyVerdict::Refused(Arc::new(runtime_apply_error(error, false)))
    }
}

pub(crate) fn legacy_apply_verdict(
    result: &anyhow::Result<()>,
    uncertain: bool,
    source_advanced: bool,
) -> LegacyApplyVerdict {
    match result {
        Err(error) => legacy_error_verdict(error, uncertain, source_advanced),
        Ok(()) if uncertain => LegacyApplyVerdict::Unknown(Arc::new(RuntimeError::new(
            "Core submission returned success while the execution owner still reports an uncertain outcome",
        ))),
        Ok(()) => LegacyApplyVerdict::Applied,
    }
}

pub(crate) fn waiting_dependency(error: &anyhow::Error) -> bool {
    if error.downcast_ref::<ServiceFallbackDeferred>().is_some()
        || error.downcast_ref::<CommittedTargetAdvanced>().is_some()
        || error.downcast_ref::<OwnershipTransitionPending>().is_some()
    {
        return true;
    }
    error.downcast_ref::<CoreError>().is_some_and(|error| {
        matches!(
            error.kind,
            Some(
                CoreErrorKind::RevisionConflict
                    | CoreErrorKind::OperationConflict
                    | CoreErrorKind::QueueFull
                    | CoreErrorKind::BackendUnavailable
            )
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_result_verdict_matches_ref_waiting_refused_and_unknown_semantics() {
        for kind in [
            CoreErrorKind::RevisionConflict,
            CoreErrorKind::OperationConflict,
            CoreErrorKind::QueueFull,
            CoreErrorKind::BackendUnavailable,
        ] {
            let failed = Err(CoreError::new(kind, "await owner", false).into());
            let LegacyApplyVerdict::Waiting(cause) = legacy_apply_verdict(&failed, false, false)
            else {
                panic!("{kind:?} is a dependency wait even with retryable=false");
            };
            assert_eq!(cause.core_kind(), Some(kind));
            assert!(matches!(
                legacy_apply_verdict(&failed, true, false),
                LegacyApplyVerdict::Unknown(_)
            ));
        }

        let refused =
            Err(CoreError::new(CoreErrorKind::ConfigCheckFailed, "invalid YAML", false).into());
        let LegacyApplyVerdict::Refused(cause) = legacy_apply_verdict(&refused, false, false)
        else {
            panic!("configuration rejection must not be treated as waiting");
        };
        assert_eq!(cause.core_kind(), Some(CoreErrorKind::ConfigCheckFailed));
        assert!(matches!(
            legacy_apply_verdict(&refused, false, true),
            LegacyApplyVerdict::Waiting(_)
        ));
        assert!(matches!(
            legacy_apply_verdict(&refused, true, true),
            LegacyApplyVerdict::Unknown(_)
        ));
        assert!(matches!(
            legacy_apply_verdict(&Ok(()), false, false),
            LegacyApplyVerdict::Applied
        ));
        assert!(matches!(
            legacy_apply_verdict(&Ok(()), true, false),
            LegacyApplyVerdict::Unknown(_)
        ));
    }

    #[test]
    fn waiting_does_not_poison_attempt_and_unknown_does_not_become_refused() {
        let mut policy = AttemptPolicy::default();
        policy.record(
            "source-a".into(),
            ExecutionHost::Service,
            &Err(CoreError::new(CoreErrorKind::BackendUnavailable, "offline", false).into()),
            false,
        );
        assert!(policy.snapshot().is_none());
        policy.record(
            "source-a".into(),
            ExecutionHost::Service,
            &Err(CoreError::new(CoreErrorKind::ConfigCheckFailed, "bad", false).into()),
            true,
        );
        assert!(matches!(
            policy.snapshot().unwrap().outcome,
            AttemptOutcome::Unknown(_)
        ));
        policy
            .allow("source-a", ExecutionHost::Service, Trigger::SourceCommitted)
            .unwrap();
    }

    #[test]
    fn refused_target_is_scoped_to_host_and_forceable_by_explicit_request() {
        let mut policy = AttemptPolicy::default();
        policy.record(
            "source-a".into(),
            ExecutionHost::Local,
            &Err(anyhow::anyhow!("configuration refused")),
            false,
        );
        for trigger in [
            Trigger::Startup,
            Trigger::SourceCommitted,
            Trigger::ServiceReadiness,
            Trigger::CoreChanged,
            Trigger::Tick,
            Trigger::ServiceOperation,
        ] {
            assert!(
                policy
                    .allow("source-a", ExecutionHost::Local, trigger)
                    .unwrap_err()
                    .to_string()
                    .contains("configuration refused"),
                "{trigger:?}"
            );
        }
        for trigger in [
            Trigger::Start,
            Trigger::Retry,
            Trigger::BinaryReplaced { restart: true },
        ] {
            policy
                .allow("source-a", ExecutionHost::Local, trigger)
                .unwrap();
        }
        policy
            .allow("source-b", ExecutionHost::Local, Trigger::SourceCommitted)
            .unwrap();
        policy
            .allow(
                "source-a",
                ExecutionHost::Service,
                Trigger::ServiceReadiness,
            )
            .unwrap();
    }

    #[test]
    fn transient_unavailability_remains_retryable_and_unknown_is_not_a_refusal() {
        for kind in [
            CoreErrorKind::RevisionConflict,
            CoreErrorKind::OperationConflict,
            CoreErrorKind::QueueFull,
            CoreErrorKind::BackendUnavailable,
        ] {
            let mut policy = AttemptPolicy::default();
            let result = Err(CoreError::new(kind, "wait for runtime host", false).into());
            policy.record("current".into(), ExecutionHost::Service, &result, false);
            policy
                .allow("current", ExecutionHost::Service, Trigger::SourceCommitted)
                .unwrap();
        }
        let mut policy = AttemptPolicy::default();
        policy.record(
            "current".into(),
            ExecutionHost::Local,
            &Err(anyhow::anyhow!("unobserved submission")),
            true,
        );
        policy
            .allow("current", ExecutionHost::Local, Trigger::SourceCommitted)
            .unwrap();
    }

    #[test]
    fn refused_and_unobserved_core_errors_keep_their_original_typed_causes() {
        let core_error =
            CoreError::new(CoreErrorKind::ConfigCheckFailed, "bad runtime yaml", false);
        let refusal = Err(anyhow::Error::new(core_error.clone()));
        let mut policy = AttemptPolicy::default();
        policy.record("same".into(), ExecutionHost::Local, &refusal, false);
        let AttemptOutcome::Refused(cause) = &policy.snapshot().unwrap().outcome else {
            panic!("an actually refused core apply must remain refused");
        };
        assert_eq!(cause.core_kind(), Some(CoreErrorKind::ConfigCheckFailed));
        assert!(!cause.retryable());
        assert_eq!(
            serde_json::to_value(cause.as_ref()).unwrap()["kind"],
            "apply_runtime"
        );

        policy.record("same".into(), ExecutionHost::Local, &refusal, true);
        let AttemptOutcome::Unknown(cause) = &policy.snapshot().unwrap().outcome else {
            panic!("an unobserved core apply must remain unknown");
        };
        assert_eq!(cause.core_kind(), Some(CoreErrorKind::ConfigCheckFailed));
        assert_eq!(
            serde_json::to_value(cause.as_ref()).unwrap()["kind"],
            "submission_unobserved"
        );
    }

    #[test]
    fn only_concrete_s2_owner_steps_authorize_a_handoff_continuation() {
        use chimera_core::client::reconciler::plan::OwnerPlan;

        let waiting = OwnershipTransitionPending::waiting("Service probe is unreadable");
        assert!(waiting.plan.is_none());
        assert!(waiting_dependency(&anyhow::Error::new(waiting)));

        let handoff = OwnershipTransitionPending::owner(OwnerPlan::RetireServiceThenLocal);
        assert_eq!(handoff.plan, Some(OwnerPlan::RetireServiceThenLocal));
        assert!(waiting_dependency(&anyhow::Error::new(handoff)));
    }

    #[test]
    fn service_release_and_source_supersession_are_waiting_dependencies() {
        let owner_error = anyhow::Error::new(ServiceFallbackDeferred {
            reason: "the shared Service lease is busy".into(),
        });
        assert!(waiting_dependency(&owner_error));
        assert!(owner_error.to_string().contains("ownership proof"));
        assert!(waiting_dependency(&anyhow::Error::new(
            CommittedTargetAdvanced
        )));
    }

    #[test]
    fn successful_explicit_retry_clears_the_previous_refusal() {
        let mut policy = AttemptPolicy::default();
        policy.record(
            "current".into(),
            ExecutionHost::Local,
            &Err(anyhow::anyhow!("initial refusal")),
            false,
        );
        assert!(
            policy
                .allow("current", ExecutionHost::Local, Trigger::SourceCommitted)
                .is_err()
        );
        policy.record("current".into(), ExecutionHost::Local, &Ok(()), false);
        policy
            .allow("current", ExecutionHost::Local, Trigger::SourceCommitted)
            .unwrap();
    }
}
