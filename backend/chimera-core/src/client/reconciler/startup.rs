use super::plan::{Conclusion, ServiceEvidence, Trigger};
use crate::{actor_v2::CoreStatusProjection, client::convergence::ConvergenceHealth};
use chimera_contracts::{execution_host::ExecutionHost, runtime_error::RuntimeError};
use chimera_core_manager::OperationId;
use std::sync::Arc;
/// The first startup round, returned unchanged on subsequent startup calls.
#[derive(Debug, Clone)]
pub struct StartupReport {
    pub operation_id: OperationId,
    pub observation: Option<StartupObservation>,
    pub outcome: StartupOutcome,
}

/// Startup's projection of the first round's conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupOutcome {
    Ready,
    ReadyDegraded {
        health: ConvergenceHealth,
        reason: String,
    },
    RecoveryRequired {
        reason: String,
    },
    Unsettled {
        reason: String,
    },
}
impl StartupOutcome {
    pub fn failure_reason(&self) -> Option<&str> {
        match self {
            Self::Ready => None,
            Self::ReadyDegraded { reason, .. }
            | Self::RecoveryRequired { reason }
            | Self::Unsettled { reason } => Some(reason),
        }
    }
    pub fn from_conclusion(conclusion: &Conclusion) -> Self {
        match conclusion {
            Conclusion::Converged | Conclusion::Inactive => Self::Ready,
            Conclusion::Waiting { cause } => Self::ReadyDegraded {
                health: ConvergenceHealth::WaitingDependency,
                reason: cause.to_string(),
            },
            Conclusion::NotInEffect { cause } => Self::ReadyDegraded {
                health: ConvergenceHealth::Blocked,
                reason: cause.to_string(),
            },
            Conclusion::Unknown { cause } => Self::RecoveryRequired {
                reason: cause.to_string(),
            },
            Conclusion::Closed => Self::RecoveryRequired {
                reason: RuntimeError::ShuttingDown.to_string(),
            },
        }
    }
}

/// The first observed runtime and service evidence, with the final effective desired host.
#[derive(Debug, Clone)]
pub struct StartupObservation {
    pub desired: ExecutionHost,
    pub service: Option<ServiceEvidence>,
    pub runtime: Option<CoreStatusProjection>,
}

pub fn cause(reason: String) -> Arc<RuntimeError> {
    Arc::new(RuntimeError::new(reason))
}

/// Ref workflow::execute routes Start/Retry and notifications into converge.
/// Compatibility fixtures still lack canonical candidate preparation and
/// retain their historical trigger routing until the old port is removed.
pub fn uses_committed_convergence(trigger: Trigger, canonical_session: bool) -> bool {
    matches!(trigger, Trigger::SourceCommitted | Trigger::Retry)
        || (canonical_session
            && matches!(
                trigger,
                Trigger::Start | Trigger::CoreChanged | Trigger::Startup
            ))
}

/// Ref plan(Facts) has only one S4 admission: Step::Apply. A concrete
/// Idle/Owner/Stop step must never silently become a legacy Core submission.
/// LegacyRecovery is reserved for compatibility sessions and triggers not
/// yet migrated to ref's committed-round admission.
pub enum CommittedRound {
    Apply,
    LegacyRecovery,
    Settled(anyhow::Result<()>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::convergence::ConvergenceHealth;

    #[test]
    fn ref_start_and_core_change_join_the_canonical_convergence_round() {
        for trigger in [Trigger::Start, Trigger::CoreChanged, Trigger::Startup] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(!uses_committed_convergence(trigger, false));
        }
        for trigger in [Trigger::SourceCommitted, Trigger::Retry] {
            assert!(uses_committed_convergence(trigger, true));
            assert!(uses_committed_convergence(trigger, false));
        }
        for trigger in [
            Trigger::Stop,
            Trigger::ServiceOperation,
            Trigger::PrepareService,
        ] {
            assert!(!uses_committed_convergence(trigger, true));
        }
    }

    #[test]
    fn ref_startup_projection_distinguishes_all_reconciler_conclusions() {
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Converged),
            StartupOutcome::Ready
        ));
        assert!(matches!(
            StartupOutcome::from_conclusion(&Conclusion::Inactive),
            StartupOutcome::Ready
        ));
        let waiting = Arc::new(RuntimeError::new("service is not ready"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Waiting {cause: waiting}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::WaitingDependency, reason} if reason.contains("service is not ready"))
        );
        let refused = Arc::new(RuntimeError::CoreRejectedConfig {
            core_kind: None,
            message: "candidate invalid".into(),
        });
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::NotInEffect {cause: refused}),
            StartupOutcome::ReadyDegraded {health: ConvergenceHealth::Blocked, reason} if reason.contains("candidate invalid"))
        );
        let unknown = Arc::new(RuntimeError::new("result could not be observed"));
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Unknown {cause: unknown}),
            StartupOutcome::RecoveryRequired {reason} if reason.contains("result could not be observed"))
        );
        assert!(
            matches!(StartupOutcome::from_conclusion(&Conclusion::Closed),
            StartupOutcome::RecoveryRequired {reason} if reason == RuntimeError::ShuttingDown.to_string())
        );
    }

    #[test]
    fn ref_startup_cause_keeps_the_exact_typed_reason() {
        let cause = cause("service owner not established".into());
        assert!(matches!(
            cause.as_ref(),
            RuntimeError::ConfigurationNotInEffect { .. }
        ));
        assert!(cause.to_string().contains("service owner not established"));
    }
}
