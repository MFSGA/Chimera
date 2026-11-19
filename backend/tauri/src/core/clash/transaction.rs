//! Result of a source-owned Clash override mutation.
//!
//! Commit-first updates never compensate the running Core after a successful
//! source commit. Failure to converge is reported separately from rejection.
use anyhow::{Result, anyhow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TransactionOutcome {
    /// Candidate accepted, committed, and no runtime degradation observed.
    Committed,
    /// Source committed, but the running core has not been confirmed in sync.
    CommittedDeferred { message: String },
    /// Candidate check or source commit failed before any runtime apply.
    Rejected { primary_error: String },
}

impl TransactionOutcome {
    /// A deferred runtime does not invalidate the committed source mutation.
    /// Callers that require *runtime convergence* must inspect the variant.
    pub(crate) fn into_result(self) -> Result<()> {
        match self {
            Self::Committed | Self::CommittedDeferred { .. } => Ok(()),
            Self::Rejected { primary_error } => Err(anyhow!(primary_error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TransactionOutcome;

    #[test]
    fn deferred_runtime_does_not_turn_committed_source_into_ipc_error() {
        assert!(
            TransactionOutcome::CommittedDeferred {
                message: "core temporarily unavailable".into(),
            }
            .into_result()
            .is_ok()
        );
    }

    #[test]
    fn rejected_candidate_is_an_error() {
        assert!(
            TransactionOutcome::Rejected {
                primary_error: "candidate rejected".into(),
            }
            .into_result()
            .unwrap_err()
            .to_string()
            .contains("candidate rejected")
        );
    }
}
