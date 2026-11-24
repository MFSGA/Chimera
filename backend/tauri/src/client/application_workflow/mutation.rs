//! Source-domain changes admitted to the application runtime workflow.
//!
//! This Profile slice follows ref's `DomainChange` / `MutationDomain` contract.
//! Chimera's application and clash writers remain on their current lifecycle
//! path until their migration slices move onto the shared participant.

use std::sync::Arc;

use chimera_config::profile::Profiles;
use chimera_core::state::{Ack, DecisionHandle, StateChange};
use chimera_core_manager::OperationId;
use tokio::sync::oneshot;

use super::{
    impact::RuntimeImpact,
    policy::{CommandPolicy, TryCauseKind},
};
use crate::client::runtime::{Degradation, RuntimeApplyReceipt, RuntimeSnapshot};

#[derive(Debug, Clone)]
pub(crate) enum DomainChange {
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfigDomain {
    Profiles,
}

impl DomainChange {
    pub(crate) fn domain(&self) -> ConfigDomain {
        match self {
            Self::Profiles { .. } => ConfigDomain::Profiles,
        }
    }
}

pub(crate) trait MutationDomain: Clone + Send + Sync + 'static {
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
pub(crate) struct MutationRequest {
    pub(crate) operation_id: OperationId,
    pub(crate) change: DomainChange,
    pub(crate) hints: super::impact::MutationHints,
    pub(crate) class: super::policy::CommandClass,
    pub(crate) decision: DecisionHandle,
    pub(crate) ack: Option<oneshot::Sender<TryAck>>,
    pub(crate) completion: Option<oneshot::Sender<Result<(), String>>>,
}

impl MutationRequest {
    pub(crate) fn answer(&mut self, ack: TryAck) {
        if let Some(channel) = self.ack.take() {
            let _ = channel.send(ack);
        }
    }

    pub(crate) fn settle(&mut self, result: Result<(), String>) {
        if let Some(channel) = self.completion.take() {
            let _ = channel.send(result);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TryAck {
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

/// Runtime state a cancelled Profile mutation must restore. An explicit stop
/// is distinct from a missing receipt: the last successful apply can remain in
/// memory after the user has stopped the core.
#[derive(Debug, Clone)]
pub(crate) enum KnownRuntimeState {
    Applied(Arc<RuntimeApplyReceipt>),
    Stopped,
    NeverApplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationStage {
    Preparing,
    TryingCritical,
    AwaitDecision,
    Confirming,
    Cancelling,
}

/// Evidence required before submitting a Profile runtime mutation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvidenceGap {
    BaselineUnconfirmed,
    NoRestorableBaseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefusalCause {
    Try(TryCauseKind),
    Evidence(EvidenceGap),
}

#[derive(Debug, Clone)]
pub(crate) struct ApplyFailure {
    pub(crate) stage: MutationStage,
    pub(crate) cause: RefusalCause,
    pub(crate) message: String,
}

#[derive(Debug, Clone)]
pub(crate) struct RetryableCause {
    pub(crate) stage: MutationStage,
    pub(crate) message: String,
}

pub(crate) const DEFERRED_RETRY_BUDGET: u8 = 3;

/// Where a committed runtime target came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetOrigin {
    Mutation,
    Reestablish(ReestablishCause),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReestablishCause {
    Startup,
    Recovery,
    ExplicitStart,
    ServiceReadiness,
}

/// Chimera's Profile TCC keeps the source decision and rollback baseline with
/// a mutation target until the runtime has settled. Ref's shared target does
/// not need this transaction-only evidence.
#[derive(Debug, Clone)]
pub(crate) struct MutationDeferredContext {
    pub(crate) digest: String,
    pub(crate) baseline: KnownRuntimeState,
    pub(crate) domain: ConfigDomain,
    pub(crate) decision: DecisionHandle,
}

#[derive(Debug, Clone)]
pub(crate) struct DeferredTarget {
    pub(crate) operation_id: OperationId,
    pub(crate) origin: TargetOrigin,
    pub(crate) identity: String,
    pub(crate) cause: RetryableCause,
    pub(crate) attempts_remaining: u8,
    pub(crate) attempts: u32,
    pub(crate) waits: u8,
    pub(crate) health: crate::client::convergence::ConvergenceHealth,
    pub(crate) next_attempt: Option<tokio::time::Instant>,
    pub(crate) mutation: Option<MutationDeferredContext>,
}

impl DeferredTarget {
    pub(crate) fn reestablish(
        operation_id: OperationId,
        identity: String,
        cause: ReestablishCause,
    ) -> Self {
        Self {
            operation_id,
            origin: TargetOrigin::Reestablish(cause),
            identity,
            cause: RetryableCause {
                stage: MutationStage::Preparing,
                message: "the runtime is being re-established".into(),
            },
            attempts_remaining: crate::client::convergence::RETRY_DELAYS.len() as u8,
            attempts: 0,
            waits: 0,
            health: crate::client::convergence::ConvergenceHealth::Pending,
            next_attempt: None,
            mutation: None,
        }
    }

    pub(crate) fn is_reestablish(&self) -> bool {
        matches!(self.origin, TargetOrigin::Reestablish(_))
    }

    pub(crate) fn is_mutation(&self) -> bool {
        self.origin == TargetOrigin::Mutation
            && self
                .mutation
                .as_ref()
                .is_some_and(|mutation| mutation.digest == self.identity)
    }

    pub(crate) fn allows_receipt_reuse(&self) -> bool {
        self.origin == TargetOrigin::Reestablish(ReestablishCause::Recovery)
    }

    pub(crate) fn begin(&mut self, identity: &str, now: tokio::time::Instant) -> bool {
        if !self.is_reestablish()
            || self.identity != identity
            || self.attempts_remaining == 0
            || !matches!(
                self.health,
                crate::client::convergence::ConvergenceHealth::RetryScheduled
                    | crate::client::convergence::ConvergenceHealth::WaitingDependency
            )
            || self.next_attempt.is_none_or(|deadline| deadline > now)
        {
            return false;
        }
        self.next_attempt = None;
        self.health = crate::client::convergence::ConvergenceHealth::Pending;
        true
    }

    pub(crate) fn schedule_application(
        &mut self,
        reason: String,
        automatic: bool,
    ) -> crate::client::convergence::ConvergenceHealth {
        self.waits = 0;
        self.cause.message = reason;
        let mut budget = crate::client::convergence::RetryBudget {
            remaining: self.attempts_remaining,
            attempts: self.attempts,
        };
        let delay = if automatic {
            budget.record_automatic_attempt()
        } else {
            budget.next_delay()
        };
        self.attempts_remaining = budget.remaining;
        self.attempts = budget.attempts;
        if let Some(delay) = delay {
            self.health = crate::client::convergence::ConvergenceHealth::RetryScheduled;
            self.next_attempt = Some(tokio::time::Instant::now() + delay);
        } else {
            self.health = crate::client::convergence::ConvergenceHealth::Blocked;
            self.next_attempt = None;
        }
        self.health
    }

    pub(crate) fn schedule_dependency(
        &mut self,
        reason: String,
    ) -> crate::client::convergence::ConvergenceHealth {
        if self.attempts_remaining == 0 {
            self.cause.message = format!(
                "{reason}; automatic retries are spent, so retry explicitly once the dependency is available"
            );
            self.health = crate::client::convergence::ConvergenceHealth::Blocked;
            self.next_attempt = None;
            return self.health;
        }
        self.cause.message = reason;
        let (delay, waits) = crate::client::convergence::next_wait(
            self.waits,
            crate::client::convergence::OutcomeClass::Dependency,
        );
        self.waits = waits;
        self.health = crate::client::convergence::ConvergenceHealth::WaitingDependency;
        self.next_attempt = Some(tokio::time::Instant::now() + delay);
        self.health
    }

    pub(crate) fn block(
        &mut self,
        reason: String,
    ) -> crate::client::convergence::ConvergenceHealth {
        self.cause.message = reason;
        self.health = crate::client::convergence::ConvergenceHealth::Blocked;
        self.next_attempt = None;
        self.health
    }
}

/// A core-confirmed Profile candidate held until the source transaction
/// publishes its decision. Chimera keeps the private candidate file with the
/// reference-shaped receipt so Confirm can promote it and Cancel can restore
/// the pre-Try baseline without exposing the uncommitted snapshot.
#[derive(Debug, Clone)]
pub(crate) struct AppliedCandidate {
    pub(crate) replaced: bool,
    pub(crate) receipt: Arc<RuntimeApplyReceipt>,
    pub(crate) product: Arc<RuntimeSnapshot>,
}

/// Result of the critical Profile runtime preparation phase. Keep this typed
/// through source settlement; the ACK string is only its transaction-facing
/// projection, as in ref.
#[derive(Debug, Clone)]
pub(crate) enum RuntimePrepareOutcome {
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
    RecoveryRequired(Box<super::workflow::RecoveryContext>),
}

impl RuntimePrepareOutcome {
    pub(crate) fn ack(&self) -> TryAck {
        match self {
            Self::Applied(_) | Self::SavedInactive | Self::Saved => TryAck::Ok,
            Self::Deferred { cause, .. } => TryAck::Degraded(cause.message.clone()),
            Self::Rejected { cause, .. } => TryAck::Rejected(cause.message.clone()),
            Self::RecoveryRequired(context) => TryAck::Failed(context.error.clone()),
        }
    }

    pub(crate) fn refusal(&self) -> Option<RefusalCause> {
        match self {
            Self::Rejected { cause, .. } => Some(cause.cause),
            _ => None,
        }
    }

    pub(crate) fn kind(&self) -> MutationOutcomeKind {
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
pub(crate) enum MutationOutcomeKind {
    Applied,
    Deferred,
    SavedInactive,
    Saved,
    Rejected,
    RecoveryRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MutationConclusion {
    Confirmed,
    Cancelled,
    Withdrawn,
    RecoveryRequired,
}

pub use chimera_core::client::reconciler::check_record::{CheckRecord, RuntimeCheckFailure};

/// Structured record for one Profile mutation, parallel to ref's
/// `MutationReceipt`. Its public wire projection remains `CommitReceipt`.
#[derive(Debug, Clone)]
pub(crate) struct MutationReceipt {
    pub(crate) degradations: Vec<Degradation>,
    pub(crate) operation_id: OperationId,
    pub(crate) domain: ConfigDomain,
    pub(crate) impact: RuntimeImpact,
    pub(crate) policy: CommandPolicy,
    pub(crate) check: CheckRecord,
    pub(crate) outcome: MutationOutcomeKind,
    pub(crate) refusal: Option<RefusalCause>,
    pub(crate) conclusion: MutationConclusion,
    pub(crate) detail: Option<String>,
}

impl MutationReceipt {
    pub(crate) fn runtime_status(&self) -> crate::client::runtime::RuntimeCommitStatus {
        use crate::client::runtime::RuntimeCommitStatus;

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
    use super::*;
    use crate::client::application_workflow::ports::{
        RuntimeCheckOutcome, RuntimeCheckUnavailable,
    };

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
