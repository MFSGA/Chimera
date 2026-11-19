//! Ref RuntimeReconciler attempt policy scoped to the committed target and host.
//!
//! The CoreLifecycle actor remains the sole runtime executor. This helper
//! doesn't submit, stop, or select Core hosts: it only remembers one attempted
//! target and prevents an ordinary notification from replaying a known refusal.
use std::sync::Arc;

use chimera_core::{
    actor_v2::endpoint::ExecutionHost,
    client::reconciler::plan::{Attempt, AttemptOutcome, RuntimeError, Trigger},
};
use chimera_core_manager::{CoreError, CoreErrorKind};

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
        let outcome = match result {
            Ok(()) => AttemptOutcome::Applied,
            Err(error) if uncertain => {
                AttemptOutcome::Unknown(Arc::new(RuntimeError::new(error.to_string())))
            }
            Err(error) if waiting_dependency(error) => {
                self.last = None;
                return;
            }
            Err(error) => AttemptOutcome::Refused(Arc::new(RuntimeError::new(error.to_string()))),
        };
        self.last = Some(Attempt {
            target,
            host,
            outcome,
        });
    }
}

pub(crate) fn waiting_dependency(error: &anyhow::Error) -> bool {
    if error.downcast_ref::<ServiceFallbackDeferred>().is_some()
        || error.downcast_ref::<CommittedTargetAdvanced>().is_some()
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
