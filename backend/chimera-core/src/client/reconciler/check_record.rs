/// How a critical Try ended, as typed by the adapter that could observe it.
///
/// `CoreErrorKind::Internal`, wait timeouts and lost receipts belong in
/// [`Unknown`], never in [`Transient`]: none of them says what the core did.
///
/// [`Unknown`]: TryCauseKind::Unknown
/// [`Transient`]: TryCauseKind::Transient
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryCauseKind {
    /// The outcome could not be observed. Never enters the retryable branch.
    Unknown,
    /// A check or the core itself rejected this candidate. The same desired
    /// value cannot succeed later.
    Deterministic,
    /// Observed, finished, and permanent for now: a refused elevation, a
    /// missing binary, an unsupported platform.
    Permanent,
    /// Observed, finished, and typed transient by the adapter that watched it.
    Transient,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckRecord {
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
pub struct RuntimeCheckFailure {
    pub outcome: super::ports::RuntimeCheckOutcome,
    pub message: String,
}

impl RuntimeCheckFailure {
    pub fn from_core_error(error: chimera_core_manager::CoreError) -> Self {
        use chimera_core_manager::CoreErrorKind;

        let kind = error.kind;
        let message = error.message.clone();
        let outcome = if error.retryable {
            super::ports::RuntimeCheckOutcome::Unavailable(
                chimera_contracts::runtime_error::RuntimeCheckUnavailable::Backend {
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
                chimera_contracts::runtime_error::RuntimeCheckUnavailable::Backend {
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

    pub fn candidate_unavailable(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            outcome: super::ports::RuntimeCheckOutcome::Unavailable(
                chimera_contracts::runtime_error::RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: message.clone(),
                },
            ),
            message,
        }
    }

    pub fn record(&self) -> CheckRecord {
        self.outcome.check_record()
    }

    pub fn cause(&self) -> TryCauseKind {
        use super::ports::RuntimeCheckOutcome;
        use chimera_contracts::runtime_error::RuntimeCheckUnavailable;

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

use chimera_core_manager::{CoreError, CoreErrorKind};
pub fn safe_to_defer_runtime_apply(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<CoreError>()
            .is_some_and(|error| error.kind == Some(CoreErrorKind::QueueFull) && error.retryable)
    })
}

pub fn safe_to_retry_deferred_runtime(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
            .is_some()
            || cause.downcast_ref::<CoreError>().is_some_and(|error| {
                error.kind == Some(CoreErrorKind::QueueFull) && error.retryable
            })
            || cause
                .downcast_ref::<super::check_record::RuntimeCheckFailure>()
                .is_some_and(|failure| failure.cause() == TryCauseKind::Transient)
    })
}

pub fn deferred_runtime_waiting_dependency(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<super::attempt::OwnershipTransitionPending>()
            .is_some()
            || cause
                .downcast_ref::<super::check_record::RuntimeCheckFailure>()
                .is_some_and(|failure| matches!(failure.record(), CheckRecord::Unserviceable(_)))
    })
}
