//! The pinned ref source mutation's typed pre-commit failures.
//!
//! A failed RuntimeGate cannot be treated as an accepted source write.
//! A deterministic Core rejection and a missing verdict have different
//! recovery implications and must keep their original causal errors.

use std::sync::Arc;

use serde::Serialize;
use snafu::Snafu;

use crate::{BoxedCause, runtime_error::RuntimeError};

/// Source domain for a committed application, Clash or Profiles document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConfigDomain {
    Application,
    Clash,
    Profiles,
}

#[derive(Debug, Snafu)]
#[snafu(display("the runtime reconciler is not ready"))]
#[snafu(visibility(pub))]
pub struct NotReady;

/// Port of the ref's real source-commit error shape; a runtime candidate
/// check has not written any source file and must never be replayed as one.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[snafu(visibility(pub))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CommitAborted {
    #[snafu(display("failed to write the config"))]
    WriteConfig {
        #[serde(skip)]
        source: BoxedCause,
    },
    #[snafu(display("failed to write the config and failed to recover afterwards"))]
    RecoverAfterWriteFailure {
        #[serde(skip)]
        source: BoxedCause,
    },
    #[snafu(display("the core rejected the candidate configuration: {}", joined(errors)))]
    RuntimeRefused {
        errors: Vec<Arc<RuntimeError>>,
        #[serde(skip)]
        source: BoxedCause,
    },
    #[snafu(display("the change could not be checked against the core: {}", joined(errors)))]
    RuntimeFailed {
        errors: Vec<Arc<RuntimeError>>,
        #[serde(skip)]
        source: BoxedCause,
    },
}

fn joined(errors: &[Arc<RuntimeError>]) -> String {
    errors
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error::RuntimeCheckUnavailable;

    #[test]
    fn runtime_refusal_and_missing_verdict_keep_distinct_ref_kinds() {
        let refused = CommitAborted::RuntimeRefused {
            errors: vec![Arc::new(RuntimeError::CoreRejectedConfig {
                core_kind: None,
                message: "candidate was rejected".into(),
            })],
            source: Box::new(std::io::Error::other("real gate refused")),
        };
        let refusal_wire = serde_json::to_value(&refused).unwrap();
        assert_eq!(refusal_wire["kind"], "runtime_refused");
        assert_eq!(refusal_wire["errors"][0]["kind"], "core_rejected_config");
        assert!(refusal_wire.get("source").is_none());

        let failed = CommitAborted::RuntimeFailed {
            errors: vec![Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "profile file cannot be read".into(),
                },
            })],
            source: Box::new(std::io::Error::other("real read failure")),
        };
        let failed_wire = serde_json::to_value(&failed).unwrap();
        assert_eq!(failed_wire["kind"], "runtime_failed");
        assert_eq!(failed_wire["errors"][0]["kind"], "check_unavailable");
        assert!(failed_wire.get("source").is_none());
    }
}
