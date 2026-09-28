//! Profile workflow ports and the typed result of one advisory runtime check.

use crate::core::actor_v2::control_endpoint::ExecutionHost;

/// Why no config check ran. These are reasons, never passing verdicts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeCheckUnavailable {
    NoEndpoint {
        reason: String,
    },
    HostUnsupported {
        host: ExecutionHost,
        reason: String,
    },
    CandidateUnavailable {
        reason: String,
    },
    Backend {
        kind: Option<chimera_core_manager::CoreErrorKind>,
        message: String,
        retryable: bool,
    },
}

/// What the runtime host said about a candidate document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeCheckOutcome {
    Passed,
    Rejected {
        kind: Option<chimera_core_manager::CoreErrorKind>,
        message: String,
    },
    Unavailable(RuntimeCheckUnavailable),
}

impl RuntimeCheckOutcome {
    pub(crate) fn check_record(&self) -> super::mutation::CheckRecord {
        use super::mutation::CheckRecord;

        match self {
            Self::Passed => CheckRecord::Passed,
            Self::Rejected { message, .. } => CheckRecord::Rejected(message.clone()),
            Self::Unavailable(
                RuntimeCheckUnavailable::NoEndpoint { reason }
                | RuntimeCheckUnavailable::HostUnsupported { reason, .. },
            ) => CheckRecord::Skipped(reason.clone()),
            Self::Unavailable(RuntimeCheckUnavailable::CandidateUnavailable { reason }) => {
                CheckRecord::Unserviceable(reason.clone())
            }
            Self::Unavailable(RuntimeCheckUnavailable::Backend { message, .. }) => {
                CheckRecord::Unserviceable(message.clone())
            }
        }
    }
}
