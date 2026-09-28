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
        digest: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CheckRecord {
    NotOwed,
    Passed,
    Skipped(String),
    Unserviceable(String),
    /// The core ran a check and rejected the candidate. This extra variant
    /// corrects ref's current `NotOwed` recording on this branch: the check
    /// did run, so diagnostics must not claim it was absent.
    Rejected(String),
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub(crate) struct RuntimeCheckFailure {
    pub(crate) outcome: super::ports::RuntimeCheckOutcome,
    pub(crate) message: String,
}

impl RuntimeCheckFailure {
    pub(crate) fn from_core_error(error: chimera_core_manager::CoreError) -> Self {
        use chimera_core_manager::CoreErrorKind;

        let kind = error.kind;
        let message = error.message.clone();
        let outcome = if error.retryable {
            super::ports::RuntimeCheckOutcome::Unavailable(
                super::ports::RuntimeCheckUnavailable::Backend {
                    kind,
                    message,
                    retryable: true,
                },
            )
        } else if matches!(
            kind,
            Some(CoreErrorKind::ConfigCheckFailed | CoreErrorKind::InvalidConfig)
        ) {
            super::ports::RuntimeCheckOutcome::Rejected { kind, message }
        } else {
            super::ports::RuntimeCheckOutcome::Unavailable(
                super::ports::RuntimeCheckUnavailable::Backend {
                    kind,
                    message,
                    retryable: false,
                },
            )
        };
        Self {
            outcome,
            message: error.to_string(),
        }
    }

    pub(crate) fn candidate_unavailable(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            outcome: super::ports::RuntimeCheckOutcome::Unavailable(
                super::ports::RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: message.clone(),
                },
            ),
            message,
        }
    }

    pub(crate) fn record(&self) -> CheckRecord {
        self.outcome.check_record()
    }

    pub(crate) fn cause(&self) -> TryCauseKind {
        use super::ports::{RuntimeCheckOutcome, RuntimeCheckUnavailable};

        match &self.outcome {
            RuntimeCheckOutcome::Passed => TryCauseKind::Permanent,
            RuntimeCheckOutcome::Rejected { .. } => TryCauseKind::Deterministic,
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::CandidateUnavailable {
                ..
            }) => TryCauseKind::Transient,
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                retryable: true,
                ..
            }) => TryCauseKind::Transient,
            RuntimeCheckOutcome::Unavailable(_) => TryCauseKind::Permanent,
        }
    }
}

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
}
