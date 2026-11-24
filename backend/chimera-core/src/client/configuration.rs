//! Ref source convergence receipts. The ProfilesActor is their only writer.
use super::convergence::ConvergenceHealth;
use chimera_config::profile::ProfileId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SourceOrigin {
    ScheduledRefresh,
    ManualRefresh,
    ExternalFile,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceOutcome {
    Committed {
        operation_id: Option<String>,
    },
    /// Its result no longer applied to the profile and was dropped (V31).
    Superseded {
        reason: String,
    },
    Failed {
        message: String,
    },
    /// The content reached the source transaction and was refused; nothing
    /// was committed and an external file is never rewritten (V33).
    Rejected {
        code: String,
        message: String,
    },
}

impl SourceOutcome {
    pub const SUBSCRIPTION_REJECTED: &'static str = "subscription_rejected";
    pub const EXTERNAL_SOURCE_REJECTED: &'static str = "external_source_rejected";

    pub fn rejected(code: &str, error: impl std::fmt::Display) -> Self {
        Self::Rejected {
            code: code.into(),
            message: error.to_string(),
        }
    }

    pub fn health(&self) -> ConvergenceHealth {
        match self {
            Self::Committed { .. } | Self::Superseded { .. } => ConvergenceHealth::Healthy,
            Self::Failed { .. } | Self::Rejected { .. } => ConvergenceHealth::Blocked,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SourceStatus {
    pub profile: ProfileId,
    pub name: String,
    pub origin: SourceOrigin,
    pub outcome: SourceOutcome,
    pub health: ConvergenceHealth,
    /// Unix milliseconds.
    pub at: i64,
}
