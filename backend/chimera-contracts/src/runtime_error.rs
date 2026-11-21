//! Typed Core and runtime workflow errors, migrated from the pinned upstream
//! `nyanpasu-runtime-values/src/runtime_error.rs`.
//!
//! The full upstream enum also owns profile-transform, package-install,
//! publish, and port-resolve error types. Those variants must migrate with
//! their actual domain errors; placeholders would destroy the cause chain.

use chimera_core_metadata::CoreErrorKind;
use serde::{Deserialize, Serialize};
use snafu::Snafu;

/// Wire representation of a Core Manager error. Retain the original kind,
/// operation id, message, and producer-defined retryability, as upstream does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CoreFailure {
    pub kind: Option<CoreErrorKind>,
    pub message: String,
    pub retryable: bool,
    pub operation_id: Option<String>,
}

impl std::fmt::Display for CoreFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            Some(kind) => write!(f, "{kind}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for CoreFailure {}

/// A candidate validation had no authoritative verdict. `Backend` is the
/// exact pinned-ref wire case; the other branches retain Chimera's distinct
/// no-endpoint, unavailable input, and skipped/not-owed boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum RuntimeCheckUnavailable {
    Backend {
        kind: Option<CoreErrorKind>,
        message: String,
        retryable: bool,
    },
    CandidateUnavailable {
        reason: String,
    },
    NoEndpoint {
        reason: String,
    },
    HostUnsupported {
        host: String,
        reason: String,
    },
    NotChecked {
        reason: String,
    },
    Other {
        reason: String,
    },
}

/// Ref runtime errors whose real causes are already available in the shared
/// Core metadata layer. Other source-carrying variants are added only when
/// their domain owner has migrated and can supply the exact error type.
#[derive(Debug, Snafu, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[snafu(visibility(pub))]
pub enum RuntimeError {
    #[snafu(display("the application is shutting down"))]
    ShuttingDown,

    #[snafu(display(
        "the runtime owner did not answer; the outcome of operation {operation_id} is unknown"
    ))]
    OwnerUnresponsive { operation_id: String },

    #[snafu(display("could not apply the runtime configuration: {failure}"))]
    ApplyRuntime {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not stop the core: {failure}"))]
    StopCore {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not recover the core: {failure}"))]
    RecoverRuntime {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not recover the service endpoint: {failure}"))]
    RecoverServiceEndpoint {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not read the core status: {failure}"))]
    RefreshStatus {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not move the runtime to the {host:?} execution host: {failure}"))]
    MoveHost {
        host: crate::execution_host::ExecutionHost,
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not install the service: {failure}"))]
    InstallService {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not start the service: {failure}"))]
    StartService {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not stop the service: {failure}"))]
    StopService {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not restart the service: {failure}"))]
    RestartService {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not uninstall the service: {failure}"))]
    UninstallService {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("could not find the core to run: {source}"))]
    ResolveCoreBinary {
        source: crate::core_spec::CoreSpecError,
    },

    #[snafu(display("could not install the core binary: {source}"))]
    InstallCoreBinary {
        source: crate::core_install::InstallCoreBinaryError,
    },

    #[snafu(display("could not prepare the service install command: {source}"))]
    PrepareServiceInstallPrompt {
        source: crate::service::ServiceCommandError,
    },

    #[snafu(display("could not read the version of the core: {source}"))]
    ReadCoreVersion {
        source: crate::core_version::CoreVersionError,
    },

    #[snafu(display("could not publish the runtime configuration: {source}"))]
    PublishRuntime {
        source: crate::publish::PublishRuntimeError,
    },

    #[snafu(display("could not resolve the ports of the runtime: {source}"))]
    ResolvePort {
        source: crate::ports::PortResolveError,
    },

    /// The core restored its previous configuration; this is not a successful apply.
    #[snafu(display("the core would not start this configuration and kept the previous one"))]
    CoreRolledBack { reason: Option<String> },

    #[snafu(display("the committed configuration is not in effect: {reason}"))]
    ConfigurationNotInEffect { reason: String },

    #[snafu(display("the core rejected the configuration: {message}"))]
    CoreRejectedConfig {
        core_kind: Option<CoreErrorKind>,
        message: String,
    },

    #[snafu(display("the configuration could not be checked: {reason:?}"))]
    CheckUnavailable { reason: RuntimeCheckUnavailable },

    #[snafu(display("the runtime submission is unobserved: {failure}"))]
    SubmissionUnobserved {
        #[snafu(source)]
        failure: CoreFailure,
    },

    #[snafu(display("the service still hosts the core; hand it to local before uninstalling"))]
    ServiceHostsCore,

    #[snafu(display("there is no runtime configuration yet"))]
    NoRuntimeConfig,

    #[snafu(display("the runtime snapshot changed; refresh the inspection"))]
    RuntimeSnapshotChanged,

    #[snafu(display("the runtime snapshot has no node {node_id}"))]
    RuntimeNodeNotFound { node_id: u32 },
}

impl RuntimeError {
    /// Compatibility constructor for existing planner judgments which have
    /// no Core Manager error. It preserves the precise reason; real Core
    /// errors must instead enter through their typed `CoreFailure`.
    pub fn new(reason: impl Into<String>) -> Self {
        Self::ConfigurationNotInEffect {
            reason: reason.into(),
        }
    }

    pub fn core_failure(&self) -> Option<&CoreFailure> {
        match self {
            Self::ApplyRuntime { failure }
            | Self::StopCore { failure }
            | Self::RecoverRuntime { failure }
            | Self::RecoverServiceEndpoint { failure }
            | Self::RefreshStatus { failure }
            | Self::InstallService { failure }
            | Self::StartService { failure }
            | Self::StopService { failure }
            | Self::RestartService { failure }
            | Self::UninstallService { failure }
            | Self::MoveHost { failure, .. }
            | Self::SubmissionUnobserved { failure } => Some(failure),
            Self::ShuttingDown
            | Self::OwnerUnresponsive { .. }
            | Self::ConfigurationNotInEffect { .. }
            | Self::PublishRuntime { .. }
            | Self::ResolveCoreBinary { .. }
            | Self::InstallCoreBinary { .. }
            | Self::PrepareServiceInstallPrompt { .. }
            | Self::ReadCoreVersion { .. }
            | Self::ResolvePort { .. }
            | Self::CoreRolledBack { .. }
            | Self::CoreRejectedConfig { .. }
            | Self::CheckUnavailable { .. }
            | Self::ServiceHostsCore
            | Self::NoRuntimeConfig
            | Self::RuntimeSnapshotChanged
            | Self::RuntimeNodeNotFound { .. } => None,
        }
    }

    pub fn core_kind(&self) -> Option<CoreErrorKind> {
        if matches!(self, Self::ResolveCoreBinary { .. }) {
            Some(CoreErrorKind::BinaryNotFound)
        } else {
            self.core_failure().and_then(|failure| failure.kind)
        }
    }

    pub fn retryable(&self) -> bool {
        self.core_failure().is_some_and(|failure| failure.retryable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_failure_retains_kind_operation_id_and_retryability() {
        let cause = CoreFailure {
            kind: Some(CoreErrorKind::ApplyFailed),
            message: "candidate rejected".into(),
            retryable: false,
            operation_id: Some("operation-1".into()),
        };
        let error = RuntimeError::ApplyRuntime {
            failure: cause.clone(),
        };
        assert_eq!(error.core_kind(), cause.kind);
        assert!(!error.retryable());
        let wire = serde_json::to_value(&error).unwrap();
        assert_eq!(wire["kind"], "apply_runtime");
        assert_eq!(wire["failure"]["kind"], "apply_failed");
        assert_eq!(wire["failure"]["operation_id"], "operation-1");
        assert_eq!(wire["failure"]["retryable"], false);
    }

    #[test]
    fn candidate_check_refusal_and_backend_failure_have_distinct_ref_wire_shapes() {
        let rejected = RuntimeError::CoreRejectedConfig {
            core_kind: Some(CoreErrorKind::ConfigCheckFailed),
            message: "bad candidate".into(),
        };
        let rejected_wire = serde_json::to_value(&rejected).unwrap();
        assert_eq!(rejected_wire["kind"], "core_rejected_config");
        assert_eq!(rejected_wire["core_kind"], "config_check_failed");
        let unavailable = RuntimeError::CheckUnavailable {
            reason: RuntimeCheckUnavailable::Backend {
                kind: Some(CoreErrorKind::BackendUnavailable),
                message: "endpoint offline".into(),
                retryable: true,
            },
        };
        let unavailable_wire = serde_json::to_value(&unavailable).unwrap();
        assert_eq!(unavailable_wire["kind"], "check_unavailable");
        assert_eq!(unavailable_wire["reason"]["cause"], "backend");
        assert_eq!(unavailable_wire["reason"]["retryable"], true);
    }

    #[test]
    fn upstream_port_resolution_failure_stays_typed_in_runtime_error() {
        let error = RuntimeError::ResolvePort {
            source: crate::ports::PortResolveError::PortConflict {
                first: crate::ports::PortField::Mixed,
                second: crate::ports::PortField::Redir,
                port: 7890,
            },
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "kind": "resolve_port",
                "source": {
                    "kind": "port_conflict",
                    "first": "mixed",
                    "second": "redir",
                    "port": 7890,
                }
            })
        );
        assert_eq!(error.core_failure(), None);
    }

    #[test]
    fn ref_host_change_keeps_real_failure_kind_and_host_wire() {
        let failure = CoreFailure {
            kind: Some(CoreErrorKind::BackendUnavailable),
            message: "service endpoint lost".into(),
            retryable: true,
            operation_id: Some("service-handoff-7".into()),
        };
        let err = RuntimeError::MoveHost {
            host: crate::execution_host::ExecutionHost::Service,
            failure,
        };
        assert_eq!(err.core_kind(), Some(CoreErrorKind::BackendUnavailable));
        assert!(err.retryable());
        let wire = serde_json::to_value(err).unwrap();
        assert_eq!(wire["kind"], "move_host");
        assert_eq!(wire["host"], "service");
        assert_eq!(wire["failure"]["kind"], "backend_unavailable");
        assert_eq!(wire["failure"]["operation_id"], "service-handoff-7");
    }

    #[test]
    fn missing_core_binary_is_typed_and_not_plausibly_retryable() {
        let error = RuntimeError::ResolveCoreBinary {
            source: crate::core_spec::CoreSpecError::FindCoreBinary {
                core: chimera_config::application::ClashCore::ChimeraClientAlpha,
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing core"),
            },
        };
        assert_eq!(error.core_kind(), Some(CoreErrorKind::BinaryNotFound));
        assert!(!error.retryable());
        let wire = serde_json::to_value(error).unwrap();
        assert_eq!(wire["kind"], "resolve_core_binary");
        assert_eq!(wire["source"]["kind"], "find_core_binary");
        assert_eq!(wire["source"]["core"], "chimera-client-alpha");
        assert!(wire["source"].get("source").is_none());
    }

    #[test]
    fn conservative_configuration_cause_is_not_a_fabricated_core_failure() {
        let error = RuntimeError::new("waiting for owner");
        assert_eq!(error.core_failure(), None);
        assert!(!error.retryable());
        assert_eq!(
            serde_json::to_value(error).unwrap(),
            serde_json::json!({"kind":"configuration_not_in_effect","reason":"waiting for owner"})
        );
    }
}
