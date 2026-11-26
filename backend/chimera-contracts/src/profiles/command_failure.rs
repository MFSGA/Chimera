//! Typed, source-derived profile failure details for shared runtime outcomes.
//!
//! This is a deliberately narrow projection of the pinned upstream
//! `nyanpasu-runtime-values/src/profiles.rs::ProfilesError`. The document
//! owner retains its actual error and source chain; category-only cases
//! keep their real local owner category. Never synthesize an
//! upstream file/commit cause from a display string.

use chimera_config::profile::{ProfileId, ProfileValidationError};
use serde::Serialize;
use snafu::Snafu;

/// Source-bearing content validation errors from upstream `profiles.rs`.
/// Preserve the exact YAML parser error in the actor; omit it from IPC, as
/// the upstream domain does. Chimera still parses content with serde_yaml 0.9.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileContentError {
    #[snafu(display("the document is not a YAML mapping"))]
    NotYamlMapping {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("could not reserialize the YAML mapping"))]
    ReserializeYaml {
        #[serde(skip)]
        source: serde_yaml::Error,
    },
    #[snafu(display("the subscription contains neither `proxies` nor `proxy-providers`"))]
    MissingProxies,
    #[snafu(display("the script is empty"))]
    EmptyScript,
}

/// Source materialization phase. The durable transaction still owns the
/// actual journal, commit, and compensation sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationOperation {
    PrepareFileFirst,
    Compensate,
    Complete,
    PrepareCleanup,
    ActivateCleanup,
    CancelCleanup,
    RetryCleanup,
    Reconcile,
}

/// Why the source transaction failed before a confirmed document commit.
/// `WriteConfig` and `RecoverAfterWriteFailure` are the pinned ref's
/// CommitAborted classes. The others distinguish Chimera's actual local
/// write, preflight ACK, and post-write recovery ownership boundaries.
/// The original ReplaceIfVersionError remains the #[source] in ProfilesActor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CommitFailureReason {
    WriteConfig,
    RecoverAfterWriteFailure,
    LocalWrite,
    PrepareAck,
    StateValidation,
    StateRecovery,
    /// The Core actually validated and rejected the candidate before commit.
    RuntimeRefused,
    /// Candidate validation did not return an authoritative verdict.
    RuntimeFailed,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileFailure {
    ProfileNotFound {
        uid: ProfileId,
    },
    ProfileInUse {
        uid: ProfileId,
        referrers: Vec<ProfileId>,
        current: bool,
        global_transforms: bool,
    },
    ReorderListSizeMismatch {
        expected: usize,
        got: usize,
    },
    ReorderListDuplicate {
        uid: ProfileId,
    },
    ValidationFailed {
        errors: Vec<ProfileValidationError>,
    },
    VersionConflict {
        expected: u64,
        actual: u64,
        /// Chimera retains a failed pre-commit staging cleanup separately.
        #[serde(skip_serializing_if = "Option::is_none")]
        cleanup: Option<String>,
    },
    RevisionOverflow,
    /// Only returned when an actual conditional-write error supplied a
    /// concrete source category; no source or cleanup object is fabricated.
    Commit {
        reason: CommitFailureReason,
    },
    ProfileHasNoFile,
    NotARemoteProfile,
    ProfileFileNotWritable {
        uid: ProfileId,
    },
    ProfileIdCollision {
        uid: ProfileId,
    },
    RefreshInProgress {
        uid: ProfileId,
    },
    ProfileDeletedDuringRefresh {
        uid: ProfileId,
    },
    ProfileChangedDuringRefresh {
        uid: ProfileId,
    },
    FingerprintDefinition {
        uid: ProfileId,
    },
    // Actual Chimera owner failure categories, without reconstructing a
    // specific operational source from the old diagnostic strings.
    RefreshFailed,
    ImportFailed,
    Persist,
    RuntimeCheck,
    SourceTransaction,
    MaterializationFailed,
    FetchSubscriptionFailed,
    ProfileContentRejected,
    Rpc,
    ProfileSyncJobs,
    ShuttingDown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_errors_serialize_without_leaking_yaml_parser_source() {
        let source = serde_yaml::from_str::<serde_yaml::Mapping>("[").unwrap_err();
        let failure = ProfileContentError::NotYamlMapping { source };
        assert_eq!(
            serde_json::to_value(&failure).unwrap(),
            serde_json::json!({"kind": "not_yaml_mapping"}),
        );
        assert_eq!(
            serde_json::to_value(ProfileContentError::MissingProxies).unwrap(),
            serde_json::json!({"kind": "missing_proxies"}),
        );
        assert_eq!(
            serde_json::to_value(MaterializationOperation::PrepareFileFirst).unwrap(),
            "prepare_file_first"
        );
    }

    #[test]
    fn commit_failure_reason_serializes_as_a_concrete_source_class() {
        let error = ProfileFailure::Commit {
            reason: CommitFailureReason::RecoverAfterWriteFailure,
        };
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "commit",
                "reason": "recover_after_write_failure"
            })
        );
        assert_eq!(
            serde_json::to_value(CommitFailureReason::LocalWrite).unwrap(),
            "local_write"
        );
    }

    #[test]
    fn version_conflict_retains_both_versions_and_cleanup_evidence() {
        let error = ProfileFailure::VersionConflict {
            expected: 7,
            actual: 8,
            cleanup: Some("staging cleanup fenced".into()),
        };
        let wire = serde_json::to_value(error).unwrap();
        assert_eq!(wire["kind"], "version_conflict");
        assert_eq!(wire["expected"], 7);
        assert_eq!(wire["actual"], 8);
        assert_eq!(wire["cleanup"], "staging cleanup fenced");
    }

    #[test]
    fn denied_deletion_preserves_profile_identity_and_referrers() {
        let error = ProfileFailure::ProfileInUse {
            uid: ProfileId("selected".into()),
            referrers: vec![ProfileId("child".into())],
            current: true,
            global_transforms: false,
        };
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({
                "kind": "profile_in_use",
                "uid": "selected",
                "referrers": ["child"],
                "current": true,
                "global_transforms": false,
            })
        );
    }

    #[test]
    fn reorder_failure_distinguishes_size_and_duplicate() {
        assert_eq!(
            serde_json::to_value(ProfileFailure::ReorderListSizeMismatch {
                expected: 3,
                got: 2
            })
            .unwrap(),
            serde_json::json!({"kind": "reorder_list_size_mismatch", "expected": 3, "got": 2}),
        );
        assert_eq!(
            serde_json::to_value(ProfileFailure::ReorderListDuplicate {
                uid: ProfileId("same".into())
            })
            .unwrap(),
            serde_json::json!({"kind": "reorder_list_duplicate", "uid": "same"}),
        );
    }

    #[test]
    fn missing_profile_preserves_original_uid() {
        let error = ProfileFailure::ProfileNotFound {
            uid: ProfileId("p5".into()),
        };
        let wire = serde_json::to_value(error).unwrap();
        assert_eq!(wire["kind"], "profile_not_found");
        assert_eq!(wire["uid"], "p5");
    }
}
