//! Shared source-committed mutation results.
//!
//! Port of the dependency-independent parts of pinned ref
//! `nyanpasu-runtime-values/src/runtime.rs`. Rich RuntimeError- and
//! ProfilesError-bearing reasons remain with their owning domains; Chimera's
//! additional runtime states and phases are retained for desktop wire parity.
use serde::{Deserialize, Serialize};

use crate::profiles::{command_failure::ProfileFailure, ports::ProfileDegradationCode};
use chimera_config::profile::ProfileId;

/// A source commit and its critical runtime outcome. Side effects settle
/// separately and cannot undo a committed source mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CommitReceipt {
    pub operation_id: Option<String>,
    pub domain: String,
    pub source_version: u64,
    pub runtime: RuntimeCommitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeCommitStatus {
    Applied,
    Deferred,
    SavedInactive,
    Unchanged,
    /// Chimera's explicitly unproven async runtime state.
    Pending,
    /// Chimera's durable recovery-required state.
    RecoveryRequired,
}

/// REF's committed/committed-degraded shape, independent of Tauri or IPC.
#[derive(Debug, Clone, Serialize, specta::Type)]
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

    pub fn value(&self) -> &T {
        match self {
            Self::Committed { value, .. } | Self::CommittedDegraded { value, .. } => value,
        }
    }

    pub fn into_value(self) -> T {
        self.into_parts().0
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
        let mut outcome = Self::from_parts(value, degradations);
        for receipt in commits {
            outcome = outcome.with_commit(receipt);
        }
        outcome
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Degradation {
    pub phase: DegradationPhase,
    pub reason: DegradationReason,
    pub message: String,
    pub retryable: bool,
}

/// Dependency-independent typed REF degradation reasons.
/// Variants requiring a complete RuntimeError/ProfilesError are deliberately
/// not represented with placeholder strings or fabricated error causes.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DegradationReason {
    ModeInterruptionFailed {
        cause: InterruptFailure,
    },
    ProfileInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyInterruptionFailed {
        cause: InterruptFailure,
    },
    ProxyCacheRefreshFailed,
    /// A durable source commit whose runtime has not converged.
    RuntimeDeferred {
        cause: std::sync::Arc<crate::runtime_error::RuntimeError>,
    },
    /// Chimera observes confirmed product publication debt by target.
    /// A pending target alone is not evidence of a particular IO error.
    RuntimeProductPublicationPending {
        target: String,
    },
    JournalInvalid,
    JobsJournalUnavailable,
    MaterializationDeferred,
    CleanupDeferred,
    /// Ref's post-commit profile activation result with a real, typed cause.
    /// Only source errors that can be projected without losing their identity
    /// are admitted; the older desktop wire retains its flat fallback.
    ProfileAutoActivationFailed {
        profile: ProfileId,
        cause: Box<ProfileFailure>,
    },
    HttpServerUnavailable,
    LocalSocketUnavailable,
}

impl DegradationReason {
    /// Existing desktop IPC still projects a flat `code` string. A typed
    /// reason owns this mapping, rather than each UI adapter inventing one.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ModeInterruptionFailed { .. } => "mode_interruption_failed",
            Self::ProfileInterruptionFailed { .. } => "profile_interruption_failed",
            Self::ProxyInterruptionFailed { .. } => "proxy_interruption_failed",
            Self::ProxyCacheRefreshFailed => "proxy_cache_refresh_failed",
            Self::RuntimeDeferred { .. } => "runtime_deferred",
            Self::RuntimeProductPublicationPending { .. } => "runtime_product_publish_failed",
            Self::JournalInvalid => "journal_invalid",
            Self::JobsJournalUnavailable => "jobs_journal_unavailable",
            Self::MaterializationDeferred => "materialization_deferred",
            Self::CleanupDeferred => "cleanup_deferred",
            Self::ProfileAutoActivationFailed { .. } => "profile_auto_activation_failed",
            Self::HttpServerUnavailable => "http_server_unavailable",
            Self::LocalSocketUnavailable => "local_socket_unavailable",
        }
    }
}

/// Source-of-truth mapping: the same shared profile port owns both its
/// retryability and the public structured reason. Tauri only projects the
/// existing flat `code` field at its IPC boundary.
impl From<ProfileDegradationCode> for DegradationReason {
    fn from(code: ProfileDegradationCode) -> Self {
        match code {
            ProfileDegradationCode::JournalInvalid => Self::JournalInvalid,
            ProfileDegradationCode::MaterializationDeferred => Self::MaterializationDeferred,
            ProfileDegradationCode::CleanupDeferred => Self::CleanupDeferred,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InterruptFailure {
    Stale,
    Unavailable,
    Timeout,
    Protocol,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DegradationPhase {
    /// Chimera keeps legacy config mirror errors separate from runtime apply.
    LegacyMirror,
    ProfileMaterialization,
    RuntimeBuild,
    /// Chimera's distinct validate/check/promote/publish transaction stages.
    RuntimeCheck,
    RuntimePromote,
    RuntimePublish,
    RuntimeApply,
    CoreRollback,
    SystemEffect,
    UiEffect,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_mutation_has_the_ref_committed_wire_shape() {
        let applied = MutationOutcome::from_parts((), Vec::new());
        assert!(matches!(applied, MutationOutcome::Committed { .. }));
        let applied = serde_json::to_value(applied).unwrap();
        assert_eq!(applied["status"], "committed");
        let degraded = MutationOutcome::from_parts(
            (),
            vec![Degradation {
                phase: DegradationPhase::SystemEffect,
                reason: DegradationReason::ProxyInterruptionFailed {
                    cause: InterruptFailure::Timeout,
                },
                message: "controller timeout".into(),
                retryable: false,
            }],
        );
        assert_eq!(degraded.degradations().len(), 1);
        let degraded = serde_json::to_value(degraded).unwrap();
        assert_eq!(degraded["status"], "committed_degraded");
        assert_eq!(
            degraded["degradations"][0]["reason"]["code"],
            "proxy_interruption_failed"
        );
        assert_eq!(degraded["degradations"][0]["reason"]["cause"], "timeout");
        assert_eq!(degraded["degradations"][0]["phase"], "system_effect");
    }

    #[test]
    fn profile_reason_mapping_matches_ref_codes_and_retryable_port() {
        for (source, expected) in [
            (ProfileDegradationCode::JournalInvalid, "journal_invalid"),
            (
                ProfileDegradationCode::MaterializationDeferred,
                "materialization_deferred",
            ),
            (ProfileDegradationCode::CleanupDeferred, "cleanup_deferred"),
        ] {
            let reason = DegradationReason::from(source);
            assert_eq!(reason.code(), expected);
            assert_eq!(serde_json::to_value(&reason).unwrap()["code"], expected);
        }
    }

    #[test]
    fn chimera_runtime_states_and_phases_keep_legacy_serde_values() {
        assert_eq!(
            serde_json::to_value(RuntimeCommitStatus::RecoveryRequired).unwrap(),
            "recovery_required"
        );
        assert_eq!(
            serde_json::to_value(RuntimeCommitStatus::Pending).unwrap(),
            "pending"
        );
        assert_eq!(
            serde_json::to_value(DegradationPhase::RuntimePublish).unwrap(),
            "runtime_publish"
        );
        assert_eq!(
            serde_json::to_value(DegradationPhase::LegacyMirror).unwrap(),
            "legacy_mirror"
        );
    }

    #[test]
    fn append_degradations_preserves_commit_receipts() {
        let receipt = CommitReceipt {
            operation_id: Some("probe".into()),
            domain: "proxy".into(),
            source_version: 3,
            runtime: RuntimeCommitStatus::Applied,
        };
        let outcome = MutationOutcome::from_parts((), Vec::new())
            .with_commit(receipt.clone())
            .extend_degradations(vec![Degradation {
                phase: DegradationPhase::UiEffect,
                reason: DegradationReason::ProxyCacheRefreshFailed,
                message: "read failed".into(),
                retryable: true,
            }]);
        match outcome {
            MutationOutcome::CommittedDegraded { commits, .. } => assert_eq!(commits, [receipt]),
            MutationOutcome::Committed { .. } => panic!("degradation must remain visible"),
        }
    }
}
