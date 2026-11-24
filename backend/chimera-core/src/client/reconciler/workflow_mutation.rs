//! Source-domain changes admitted to the application runtime workflow.
//!
//! This Profile slice follows ref's `DomainChange` / `MutationDomain` contract.
//! Chimera's application and clash writers remain on their current lifecycle
//! path until their migration slices move onto the shared participant.

use std::sync::Arc;

use crate::state::{Ack, DecisionHandle, StateChange};
use chimera_config::profile::Profiles;
use chimera_core_manager::OperationId;
use tokio::sync::oneshot;

use super::mutation_policy::{CommandPolicy, TryCauseKind};
use crate::client::{
    mutation_outcome::Degradation,
    runtime::{RuntimeApplyReceipt, RuntimeSnapshot},
};
use crate::state::mutation::impact::RuntimeImpact;

#[derive(Debug, Clone)]
pub enum DomainChange {
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

pub use chimera_contracts::mutation::ConfigDomain;

impl DomainChange {
    pub fn domain(&self) -> ConfigDomain {
        match self {
            Self::Profiles { .. } => ConfigDomain::Profiles,
        }
    }
}

pub trait MutationDomain: Clone + Send + Sync + 'static {
    fn domain_change(change: StateChange<Self>) -> DomainChange;
}

impl MutationDomain for Profiles {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

/// One Profile source mutation admitted to the tracked application workflow.
///
/// The source transaction owns the authoritative decision. The workflow owns
/// the runtime attempt and answers prepare before it waits for that decision.
pub struct MutationRequest {
    pub operation_id: OperationId,
    pub change: DomainChange,
    pub hints: crate::state::mutation::impact::MutationHints,
    pub class: super::mutation_policy::CommandClass,
    pub decision: DecisionHandle,
    pub ack: Option<oneshot::Sender<TryAck>>,
    pub completion: Option<oneshot::Sender<Result<(), String>>>,
}

impl MutationRequest {
    pub fn answer(&mut self, ack: TryAck) {
        if let Some(channel) = self.ack.take() {
            let _ = channel.send(ack);
        }
    }

    pub fn settle(&mut self, result: Result<(), String>) {
        if let Some(channel) = self.completion.take() {
            let _ = channel.send(result);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TryAck {
    Ok,
    Degraded(String),
    Rejected(String),
    Failed(String),
}

impl From<TryAck> for Ack {
    fn from(ack: TryAck) -> Self {
        match ack {
            TryAck::Ok => Ack::Ok,
            TryAck::Degraded(message) => Ack::Degraded(message),
            TryAck::Rejected(message) => Ack::Rejected(message),
            TryAck::Failed(message) => Ack::Failed(anyhow::anyhow!(message)),
        }
    }
}

pub use crate::client::reconciler::reestablish::{
    AppliedCandidate, ApplyFailure, DEFERRED_RETRY_BUDGET, DeferredTarget, EvidenceGap,
    KnownRuntimeState, MutationDeferredContext, MutationStage, ReestablishCause, RefusalCause,
    RetryableCause, TargetOrigin,
};

/// Result of the critical Profile runtime preparation phase. Keep this typed
/// through source settlement; the ACK string is only its transaction-facing
/// projection, as in ref.
#[derive(Debug, Clone)]
pub enum RuntimePrepareOutcome {
    Applied(AppliedCandidate),
    Deferred {
        baseline: KnownRuntimeState,
        digest: String,
        cause: RetryableCause,
    },
    SavedInactive,
    Saved,
    Rejected {
        cause: ApplyFailure,
        restored: Option<KnownRuntimeState>,
    },
    RecoveryRequired(Box<super::workflow_progress::RecoveryContext>),
}

impl RuntimePrepareOutcome {
    pub fn ack(&self) -> TryAck {
        match self {
            Self::Applied(_) | Self::SavedInactive | Self::Saved => TryAck::Ok,
            Self::Deferred { cause, .. } => TryAck::Degraded(cause.message.clone()),
            Self::Rejected { cause, .. } => TryAck::Rejected(cause.message.clone()),
            Self::RecoveryRequired(context) => TryAck::Failed(context.error.clone()),
        }
    }

    pub fn refusal(&self) -> Option<RefusalCause> {
        match self {
            Self::Rejected { cause, .. } => Some(cause.cause),
            _ => None,
        }
    }

    pub fn kind(&self) -> MutationOutcomeKind {
        match self {
            Self::Applied(_) => MutationOutcomeKind::Applied,
            Self::Deferred { .. } => MutationOutcomeKind::Deferred,
            Self::SavedInactive => MutationOutcomeKind::SavedInactive,
            Self::Saved => MutationOutcomeKind::Saved,
            Self::Rejected { .. } => MutationOutcomeKind::Rejected,
            Self::RecoveryRequired(_) => MutationOutcomeKind::RecoveryRequired,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationOutcomeKind {
    Applied,
    Deferred,
    SavedInactive,
    Saved,
    Rejected,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationConclusion {
    Confirmed,
    Cancelled,
    Withdrawn,
    RecoveryRequired,
}

pub use crate::client::reconciler::check_record::{CheckRecord, RuntimeCheckFailure};

/// Structured record for one Profile mutation, parallel to ref's
/// `MutationReceipt`. Its public wire projection remains `CommitReceipt`.
#[derive(Debug, Clone)]
pub struct MutationReceipt {
    pub degradations: Vec<Degradation>,
    pub operation_id: OperationId,
    pub domain: ConfigDomain,
    pub impact: RuntimeImpact,
    pub policy: CommandPolicy,
    pub check: CheckRecord,
    pub outcome: MutationOutcomeKind,
    pub refusal: Option<RefusalCause>,
    pub conclusion: MutationConclusion,
    pub detail: Option<String>,
}

impl MutationReceipt {
    pub fn runtime_status(&self) -> chimera_contracts::runtime::RuntimeCommitStatus {
        use chimera_contracts::runtime::RuntimeCommitStatus;

        match (self.conclusion, self.outcome) {
            (MutationConclusion::RecoveryRequired, _)
            | (_, MutationOutcomeKind::RecoveryRequired) => RuntimeCommitStatus::RecoveryRequired,
            (MutationConclusion::Confirmed, MutationOutcomeKind::Applied) => {
                RuntimeCommitStatus::Applied
            }
            (MutationConclusion::Confirmed, MutationOutcomeKind::Deferred) => {
                RuntimeCommitStatus::Deferred
            }
            (MutationConclusion::Confirmed, MutationOutcomeKind::SavedInactive) => {
                RuntimeCommitStatus::SavedInactive
            }
            (MutationConclusion::Confirmed, MutationOutcomeKind::Saved) => {
                RuntimeCommitStatus::Unchanged
            }
            (MutationConclusion::Cancelled | MutationConclusion::Withdrawn, _) => {
                RuntimeCommitStatus::Unchanged
            }
            _ => RuntimeCommitStatus::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ports::RuntimeCheckOutcome;
    use super::*;
    use chimera_contracts::runtime_error::RuntimeCheckUnavailable;

    #[test]
    fn config_check_rejection_keeps_its_typed_verdict() {
        let failure = RuntimeCheckFailure::from_core_error(chimera_core_manager::CoreError::new(
            chimera_core_manager::CoreErrorKind::ConfigCheckFailed,
            "invalid candidate",
            false,
        ));

        assert_eq!(
            failure.outcome,
            RuntimeCheckOutcome::Rejected {
                kind: Some(chimera_core_manager::CoreErrorKind::ConfigCheckFailed),
                message: "invalid candidate".into(),
            }
        );
        assert_eq!(
            failure.record(),
            CheckRecord::Rejected("invalid candidate".into())
        );
        assert_eq!(failure.cause(), TryCauseKind::Deterministic);
    }

    #[test]
    fn retryable_check_backend_error_is_unserviceable_and_transient() {
        let failure = RuntimeCheckFailure::from_core_error(chimera_core_manager::CoreError::new(
            chimera_core_manager::CoreErrorKind::BackendUnavailable,
            "service endpoint is reconnecting",
            true,
        ));

        assert!(matches!(
            failure.outcome,
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                retryable: true,
                ..
            })
        ));
        assert_eq!(
            failure.record(),
            CheckRecord::Unserviceable("service endpoint is reconnecting".into())
        );
        assert_eq!(failure.cause(), TryCauseKind::Transient);
    }

    #[test]
    fn candidate_staging_failure_is_unserviceable_and_transient() {
        let failure = RuntimeCheckFailure::candidate_unavailable(
            "could not stage Profile config for checking",
        );

        assert_eq!(
            failure.record(),
            CheckRecord::Unserviceable("could not stage Profile config for checking".into())
        );
        assert_eq!(failure.cause(), TryCauseKind::Transient);
    }

    #[test]
    fn reestablish_target_spends_one_attempt_and_begins_only_when_due() {
        let mut target = DeferredTarget::reestablish(
            chimera_core_manager::OperationId::generate(),
            "committed-runtime".into(),
            ReestablishCause::Startup,
        );
        let now = tokio::time::Instant::now();

        assert_eq!(
            target.schedule_application("transient runtime error".into(), true),
            crate::client::convergence::ConvergenceHealth::RetryScheduled
        );
        assert_eq!(target.attempts, 1);
        assert_eq!(target.attempts_remaining, DEFERRED_RETRY_BUDGET - 1);
        assert!(!target.begin("committed-runtime", now));

        let due = target.next_attempt.unwrap() + std::time::Duration::from_millis(1);
        assert!(target.begin("committed-runtime", due));
        assert_eq!(
            target.health,
            crate::client::convergence::ConvergenceHealth::Pending
        );
        assert!(target.next_attempt.is_none());
    }

    #[test]
    fn reestablish_dependency_waits_keep_the_retry_budget() {
        let mut target = DeferredTarget::reestablish(
            chimera_core_manager::OperationId::generate(),
            "committed-runtime".into(),
            ReestablishCause::Recovery,
        );

        assert_eq!(
            target.schedule_dependency("service is not ready".into()),
            crate::client::convergence::ConvergenceHealth::WaitingDependency
        );
        assert_eq!(target.attempts, 0);
        assert_eq!(target.attempts_remaining, DEFERRED_RETRY_BUDGET);
        assert_eq!(target.waits, 1);
        assert!(target.allows_receipt_reuse());
    }
}
