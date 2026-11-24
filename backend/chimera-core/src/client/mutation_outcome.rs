pub use chimera_contracts::runtime::{CommitReceipt, DegradationPhase, RuntimeCommitStatus};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl Degradation {
    /// The application retains the existing flat desktop IPC DTO while the
    /// independent Core, Profiles, and Runtime owners use REF's typed reason.
    /// Keep this the only place that discards a structured `reason.cause`.
    pub fn from_shared(degradation: chimera_contracts::runtime::Degradation) -> Self {
        Self {
            phase: degradation.phase,
            code: degradation.reason.code().into(),
            message: degradation.message,
            retryable: degradation.retryable,
        }
    }
}

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
