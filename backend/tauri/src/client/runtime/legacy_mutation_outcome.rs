//! Compatibility projection for the old desktop IPC only.
use super::{CommitReceipt, Degradation};
use chimera_contracts::runtime as shared;
use serde::{Deserialize, Serialize};

/// Public mutation wire aligned with REF: desired state is committed first;
/// post-commit side-effect failures degrade instead of turning the mutation
/// into an error that would imply the commit was rolled back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationOutcome<T> {
    Committed {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
    },
    CommittedDegraded {
        value: T,
        commits: Vec<CommitReceipt>,
        notifications_pending: bool,
        degradations: Vec<Degradation>,
    },
}

impl<T> MutationOutcome<T> {
    pub fn from_parts(value: T, degradations: Vec<Degradation>) -> Self {
        if degradations.is_empty() {
            Self::Committed {
                value,
                commits: Vec::new(),
                notifications_pending: true,
            }
        } else {
            Self::CommittedDegraded {
                value,
                commits: Vec::new(),
                notifications_pending: true,
                degradations,
            }
        }
    }

    pub fn with_commit(mut self, receipt: CommitReceipt) -> Self {
        match &mut self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.push(receipt)
            }
        }
        self
    }

    pub fn value(&self) -> &T {
        match self {
            Self::Committed { value, .. } | Self::CommittedDegraded { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        self.into_parts().0
    }

    pub fn append_commit_result(self, result: MutationOutcome<()>) -> Self {
        let commits = match &result {
            MutationOutcome::Committed { commits, .. }
            | MutationOutcome::CommittedDegraded { commits, .. } => commits.clone(),
        };
        let mut outcome = self.extend_degradations(result.into_parts().1);
        for commit in commits {
            outcome = outcome.with_commit(commit);
        }
        outcome
    }

    pub fn degradations(&self) -> &[Degradation] {
        match self {
            Self::Committed { .. } => &[],
            Self::CommittedDegraded { degradations, .. } => degradations,
        }
    }

    pub fn into_parts(self) -> (T, Vec<Degradation>) {
        match self {
            Self::Committed { value, .. } => (value, Vec::new()),
            Self::CommittedDegraded {
                value,
                degradations,
                ..
            } => (value, degradations),
        }
    }

    pub fn extend_degradations(self, extra: Vec<Degradation>) -> Self {
        let commits = match &self {
            Self::Committed { commits, .. } | Self::CommittedDegraded { commits, .. } => {
                commits.clone()
            }
        };
        let (value, mut degradations) = self.into_parts();
        degradations.extend(extra);
        commits
            .into_iter()
            .fold(Self::from_parts(value, degradations), |outcome, receipt| {
                outcome.with_commit(receipt)
            })
    }
}

impl<T> MutationOutcome<T> {
    /// Preserve the original transaction receipt and flat evidence when a
    /// shared RPC calls a desktop configuration transaction.
    pub(crate) fn into_shared_configuration_outcome(self) -> shared::MutationOutcome<T> {
        match self {
            Self::Committed {
                value,
                commits,
                notifications_pending,
            } => shared::MutationOutcome::Committed {
                value,
                commits,
                notifications_pending,
            },
            Self::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations,
            } => shared::MutationOutcome::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations: degradations
                    .into_iter()
                    .map(|item| shared::Degradation {
                        phase: item.phase,
                        reason: shared::DegradationReason::LegacyConfigurationFailure {
                            legacy_code: item.code,
                        },
                        message: item.message,
                        retryable: item.retryable,
                    })
                    .collect(),
            },
        }
    }
}

impl<T> From<shared::MutationOutcome<T>> for MutationOutcome<T> {
    fn from(outcome: shared::MutationOutcome<T>) -> Self {
        match outcome {
            shared::MutationOutcome::Committed {
                value,
                commits,
                notifications_pending,
            } => Self::Committed {
                value,
                commits,
                notifications_pending,
            },
            shared::MutationOutcome::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations,
            } => Self::CommittedDegraded {
                value,
                commits,
                notifications_pending,
                degradations: degradations
                    .into_iter()
                    .map(Degradation::from_shared)
                    .collect(),
            },
        }
    }
}
