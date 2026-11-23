//! Typed, source-derived profile failure details for shared runtime outcomes.
//!
//! This is a deliberately narrow projection of the pinned upstream
//! `nyanpasu-runtime-values/src/profiles.rs::ProfilesError`. The document
//! owner retains its actual error and source chain; only variants with the
//! same concrete data in Chimera can be published here. Never synthesize an
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

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileFailure {
    ProfileNotFound {
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
    ProfileHasNoFile,
    NotARemoteProfile,
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
    fn missing_profile_preserves_original_uid() {
        let error = ProfileFailure::ProfileNotFound {
            uid: ProfileId("p5".into()),
        };
        let wire = serde_json::to_value(error).unwrap();
        assert_eq!(wire["kind"], "profile_not_found");
        assert_eq!(wire["uid"], "p5");
    }
}
