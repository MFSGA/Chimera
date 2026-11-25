//! Legacy flat degradation used by compatibility transaction ports.
//! New shared outcomes belong to Runtime Values.
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
