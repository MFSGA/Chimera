//! Typed response privacy from the pinned ref. Project before serialization.
use crate::audit::{CompletionSummary, summarize_mutation, summarize_public_mutation};
use chimera_config::{
    clash::config::{
        TransparentProxyMode, TransparentProxyValidationError, clash_strategy::PickPortError,
    },
    runtime::executor::RuntimePipelineError,
};
use chimera_contracts::{
    core_install::InstallCoreBinaryError,
    ports::PortResolveError,
    profiles::command_failure::{CommitFailureReason, ProfileFailure},
    runtime::{Degradation, DegradationReason, MutationOutcome},
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_profile_transform::RuntimeBuildError;
use chimera_rpc_protocol::{ResponseVisibility, RpcError, types::*};

pub trait RpcResponseProjection: Sized {
    fn summarize_completion(&self) -> CompletionSummary;
    fn encode_for_visibility(
        self,
        visibility: ResponseVisibility,
    ) -> Result<serde_json::Value, RpcError>;
}

impl<T: serde::Serialize> RpcResponseProjection for RpcMutationOutcome<T> {
    fn summarize_completion(&self) -> CompletionSummary {
        match self {
            Self::Full(outcome) => summarize_mutation(outcome),
            Self::Public(outcome) => summarize_public_mutation(outcome),
        }
    }
    fn encode_for_visibility(
        self,
        visibility: ResponseVisibility,
    ) -> Result<serde_json::Value, RpcError> {
        let encoded = match (visibility, self) {
            (ResponseVisibility::Full, Self::Full(outcome)) => serde_json::to_value(outcome),
            (_, Self::Public(outcome)) => serde_json::to_value(outcome),
            (ResponseVisibility::Public, Self::Full(outcome)) => {
                serde_json::to_value(project_mutation_outcome(outcome))
            }
        };
        encoded.map_err(|error| crate::error::application_error_for(visibility, error))
    }
}

macro_rules! project_wrapper {
    ($($ty:ty),+ $(,)?) => {$(
        impl RpcResponseProjection for $ty {
            fn summarize_completion(&self) -> CompletionSummary { self.0.summarize_completion() }
            fn encode_for_visibility(self, visibility: ResponseVisibility) -> Result<serde_json::Value, RpcError> {
                self.0.encode_for_visibility(visibility)
            }
        }
    )+};
}
project_wrapper!(
    RpcUnitMutationOutcome,
    RpcProfileMutationOutcome,
    RpcFolderMutationOutcome
);

pub(crate) fn project_mutation_outcome<T>(outcome: MutationOutcome<T>) -> PublicMutationOutcome<T> {
    match outcome {
        MutationOutcome::Committed {
            value,
            commits,
            notifications_pending,
        } => PublicMutationOutcome::Committed {
            value,
            commits,
            notifications_pending,
        },
        MutationOutcome::CommittedDegraded {
            value,
            commits,
            notifications_pending,
            degradations,
        } => PublicMutationOutcome::CommittedDegraded {
            value,
            commits,
            notifications_pending,
            degradations: degradations.iter().map(project_degradation).collect(),
        },
    }
}

fn project_degradation(degradation: &Degradation) -> PublicDegradation {
    let Degradation {
        phase,
        reason,
        message: _,
        retryable,
    } = degradation;
    PublicDegradation {
        phase: *phase,
        reason: project_degradation_reason(reason),
        retryable: *retryable,
    }
}

fn project_degradation_reason(reason: &DegradationReason) -> PublicDegradationReason {
    match reason {
        DegradationReason::RuntimeDeferred { cause } => PublicDegradationReason::RuntimeDeferred {
            cause: project_runtime_error(cause),
        },
        DegradationReason::RuntimeProductPublicationPending { .. } => {
            PublicDegradationReason::RuntimeProductPublicationPending
        }
        DegradationReason::ModeInterruptionFailed { cause } => {
            PublicDegradationReason::ModeInterruptionFailed { cause: *cause }
        }
        DegradationReason::ProfileInterruptionFailed { cause } => {
            PublicDegradationReason::ProfileInterruptionFailed { cause: *cause }
        }
        DegradationReason::ProxyInterruptionFailed { cause } => {
            PublicDegradationReason::ProxyInterruptionFailed { cause: *cause }
        }
        DegradationReason::ProxyCacheRefreshFailed => {
            PublicDegradationReason::ProxyCacheRefreshFailed
        }
        DegradationReason::JournalInvalid => PublicDegradationReason::JournalInvalid,
        DegradationReason::JobsJournalUnavailable => {
            PublicDegradationReason::JobsJournalUnavailable
        }
        DegradationReason::MaterializationDeferred => {
            PublicDegradationReason::MaterializationDeferred
        }
        DegradationReason::CleanupDeferred => PublicDegradationReason::CleanupDeferred,
        DegradationReason::ProfileAutoActivationFailed { profile: _, cause } => {
            PublicDegradationReason::ProfileAutoActivationFailed {
                cause: project_profile_failure(cause),
            }
        }
        DegradationReason::HttpServerUnavailable => PublicDegradationReason::HttpServerUnavailable,
        DegradationReason::LocalSocketUnavailable => {
            PublicDegradationReason::LocalSocketUnavailable
        }
    }
}

pub(crate) fn project_profile_failure(error: &ProfileFailure) -> PublicProfilesError {
    use ProfileFailure as F;
    use PublicProfilesError as P;
    match error {
        F::ProfileNotFound { .. } => P::ProfileNotFound,
        F::ProfileInUse { referrers, .. } => P::ProfileInUse {
            referrer_count: referrers.len(),
        },
        F::ReorderListSizeMismatch { .. } => P::ReorderListSizeMismatch,
        F::ReorderListDuplicate { .. } => P::ReorderListDuplicate,
        F::ValidationFailed { .. } => P::ValidationFailed,
        F::VersionConflict { .. } => P::VersionConflict,
        F::RevisionOverflow => P::RevisionOverflow,
        F::Commit { reason } => P::Commit {
            source: PublicCommitAborted {
                kind: match reason {
                    CommitFailureReason::WriteConfig => PublicCommitAbortedKind::WriteConfig,
                    CommitFailureReason::RecoverAfterWriteFailure => {
                        PublicCommitAbortedKind::RecoverAfterWriteFailure
                    }
                    CommitFailureReason::LocalWrite => PublicCommitAbortedKind::LocalWrite,
                    CommitFailureReason::PrepareAck => PublicCommitAbortedKind::PrepareAck,
                    CommitFailureReason::StateValidation => {
                        PublicCommitAbortedKind::StateValidation
                    }
                    CommitFailureReason::StateRecovery => PublicCommitAbortedKind::StateRecovery,
                    CommitFailureReason::RuntimeRefused => PublicCommitAbortedKind::RuntimeRefused,
                    CommitFailureReason::RuntimeFailed => PublicCommitAbortedKind::RuntimeFailed,
                },
                runtime_causes: Vec::new(),
            },
        },
        F::ProfileHasNoFile => P::ProfileHasNoFile,
        F::NotARemoteProfile => P::NotARemoteProfile,
        F::ProfileFileNotWritable { .. } => P::ProfileFileNotWritable,
        F::ProfileIdCollision { .. } => P::ProfileIdCollision,
        F::RefreshInProgress { .. } => P::RefreshInProgress,
        F::ProfileDeletedDuringRefresh { .. } => P::ProfileDeletedDuringRefresh,
        F::ProfileChangedDuringRefresh { .. } => P::ProfileChangedDuringRefresh,
        F::FingerprintDefinition { .. } => P::FingerprintDefinition,
        F::ShuttingDown => P::ShuttingDown,
        F::RefreshFailed => P::RefreshFailed,
        F::ImportFailed => P::ImportFailed,
        F::Persist => P::Persist,
        F::RuntimeCheck => P::RuntimeCheck,
        F::SourceTransaction => P::SourceTransaction,
        F::MaterializationFailed => P::MaterializationFailed,
        F::FetchSubscriptionFailed => P::FetchSubscriptionFailed,
        F::ProfileContentRejected => P::ContentRejectedWithoutDetails,
        F::Rpc => P::Rpc,
        F::ProfileSyncJobs => P::ProfileSyncJobs,
    }
}

fn project_transparent_proxy_mode(mode: TransparentProxyMode) -> PublicTransparentProxyMode {
    match mode {
        TransparentProxyMode::Disabled => PublicTransparentProxyMode::Disabled,
        TransparentProxyMode::Redir => PublicTransparentProxyMode::Redir,
        TransparentProxyMode::Tproxy => PublicTransparentProxyMode::Tproxy,
    }
}

fn project_transparent_proxy_validation(
    error: &TransparentProxyValidationError,
) -> PublicTransparentProxyValidationKind {
    match error {
        TransparentProxyValidationError::UnsupportedTarget => {
            PublicTransparentProxyValidationKind::UnsupportedTarget
        }
        TransparentProxyValidationError::TunConflict => {
            PublicTransparentProxyValidationKind::TunConflict
        }
        TransparentProxyValidationError::MissingListener { mode } => {
            PublicTransparentProxyValidationKind::MissingListener {
                mode: project_transparent_proxy_mode(*mode),
            }
        }
        TransparentProxyValidationError::ZeroPort => PublicTransparentProxyValidationKind::ZeroPort,
        TransparentProxyValidationError::EmptyTarget => {
            PublicTransparentProxyValidationKind::EmptyTarget
        }
        TransparentProxyValidationError::InvalidInterface { .. } => {
            PublicTransparentProxyValidationKind::InvalidInterface
        }
        TransparentProxyValidationError::DuplicateInterface => {
            PublicTransparentProxyValidationKind::DuplicateInterface
        }
        TransparentProxyValidationError::TooManyInterfaces => {
            PublicTransparentProxyValidationKind::TooManyInterfaces
        }
    }
}

fn project_runtime_pipeline_error(error: &RuntimePipelineError) -> PublicRuntimePipelineKind {
    match error {
        RuntimePipelineError::ExecuteScript { .. } => PublicRuntimePipelineKind::ExecuteScript,
        RuntimePipelineError::SelectedProfileNotFound { .. } => {
            PublicRuntimePipelineKind::SelectedProfileNotFound
        }
        RuntimePipelineError::SelectedProfileNotConfig { .. } => {
            PublicRuntimePipelineKind::SelectedProfileNotConfig
        }
        RuntimePipelineError::CompositionMemberInvalid { .. } => {
            PublicRuntimePipelineKind::CompositionMemberInvalid
        }
        RuntimePipelineError::ContentSource { .. } => PublicRuntimePipelineKind::ContentSource,
        RuntimePipelineError::ParseProfile { .. } => PublicRuntimePipelineKind::ParseProfile,
        RuntimePipelineError::Snapshot { .. } => PublicRuntimePipelineKind::Snapshot,
        RuntimePipelineError::Internal { .. } => PublicRuntimePipelineKind::Internal,
    }
}

fn project_runtime_build_error(error: &RuntimeBuildError) -> PublicRuntimeBuildKind {
    match error {
        RuntimeBuildError::StartScriptRunner { .. } => PublicRuntimeBuildKind::StartScriptRunner,
        RuntimeBuildError::ValidateProfiles { .. } => PublicRuntimeBuildKind::ValidateProfiles,
        RuntimeBuildError::ValidateTransparentProxy { source } => {
            PublicRuntimeBuildKind::ValidateTransparentProxy {
                reason: project_transparent_proxy_validation(source),
            }
        }
        RuntimeBuildError::TransparentProxyRequiresServiceMode => {
            PublicRuntimeBuildKind::TransparentProxyRequiresServiceMode
        }
        RuntimeBuildError::RunPipeline { source } => PublicRuntimeBuildKind::RunPipeline {
            stage: project_runtime_pipeline_error(source),
        },
        RuntimeBuildError::TransformsFailed { .. } => PublicRuntimeBuildKind::TransformsFailed,
        RuntimeBuildError::SerializeFinalConfig { .. } => {
            PublicRuntimeBuildKind::SerializeFinalConfig
        }
        RuntimeBuildError::BuildWorker { .. } => PublicRuntimeBuildKind::BuildWorker,
        RuntimeBuildError::ProjectArtifact { .. } => PublicRuntimeBuildKind::ProjectArtifact,
        RuntimeBuildError::UnsupportedDnsCore { .. } => PublicRuntimeBuildKind::UnsupportedDnsCore,
        RuntimeBuildError::ConfigNotMapping => PublicRuntimeBuildKind::ConfigNotMapping,
        RuntimeBuildError::UnsupportedConfigValues { .. } => {
            PublicRuntimeBuildKind::UnsupportedConfigValues
        }
    }
}

fn project_port_resolve_error(error: &PortResolveError) -> PublicPortResolveKind {
    match error {
        PortResolveError::ResolvePort { field, source } => match source {
            PickPortError::PortNotAvailable { port } => PublicPortResolveKind::PortNotAvailable {
                field: *field,
                port: *port,
            },
            PickPortError::NoOpenPort => PublicPortResolveKind::NoOpenPort { field: *field },
        },
        PortResolveError::PortConflict {
            first,
            second,
            port,
        } => PublicPortResolveKind::PortConflict {
            first: *first,
            second: *second,
            port: *port,
        },
    }
}

fn project_install_core_binary_error(
    error: &InstallCoreBinaryError,
) -> PublicInstallCoreBinaryKind {
    match error {
        InstallCoreBinaryError::AuthorizeCoreCopy { core, .. } => {
            PublicInstallCoreBinaryKind::AuthorizeCoreCopy { core: *core }
        }
        InstallCoreBinaryError::StartElevatedCopy { core, .. } => {
            PublicInstallCoreBinaryKind::StartElevatedCopy { core: *core }
        }
        InstallCoreBinaryError::ElevatedCopyFailed {
            core, exit_code, ..
        } => PublicInstallCoreBinaryKind::ElevatedCopyFailed {
            core: *core,
            exit_code: *exit_code,
        },
        InstallCoreBinaryError::PathNotUtf8 { .. } => PublicInstallCoreBinaryKind::PathNotUtf8,
    }
}

fn project_check_unavailable(error: &RuntimeCheckUnavailable) -> PublicRuntimeCheckUnavailableKind {
    match error {
        RuntimeCheckUnavailable::Backend { kind, .. } => {
            PublicRuntimeCheckUnavailableKind::Backend { core_kind: *kind }
        }
        RuntimeCheckUnavailable::CandidateUnavailable { .. } => {
            PublicRuntimeCheckUnavailableKind::CandidateUnavailable
        }
        RuntimeCheckUnavailable::NoEndpoint { .. } => PublicRuntimeCheckUnavailableKind::NoEndpoint,
        RuntimeCheckUnavailable::HostUnsupported { .. } => {
            PublicRuntimeCheckUnavailableKind::HostUnsupported
        }
        RuntimeCheckUnavailable::NotChecked { .. } => PublicRuntimeCheckUnavailableKind::NotChecked,
        RuntimeCheckUnavailable::Other { .. } => PublicRuntimeCheckUnavailableKind::Other,
    }
}

pub(crate) fn project_runtime_error(error: &RuntimeError) -> PublicRuntimeError {
    match error {
        RuntimeError::ShuttingDown => PublicRuntimeError::ShuttingDown,
        RuntimeError::OwnerUnresponsive { .. } => PublicRuntimeError::OwnerUnresponsive,
        RuntimeError::ApplyRuntime { failure } => PublicRuntimeError::ApplyRuntime {
            core_kind: failure.kind,
        },
        RuntimeError::StopCore { failure } => PublicRuntimeError::StopCore {
            core_kind: failure.kind,
        },
        RuntimeError::RecoverRuntime { failure } => PublicRuntimeError::RecoverRuntime {
            core_kind: failure.kind,
        },
        RuntimeError::RecoverServiceEndpoint { failure } => {
            PublicRuntimeError::RecoverServiceEndpoint {
                core_kind: failure.kind,
            }
        }
        RuntimeError::RefreshStatus { failure } => PublicRuntimeError::RefreshStatus {
            core_kind: failure.kind,
        },
        RuntimeError::InstallService { failure } => PublicRuntimeError::InstallService {
            core_kind: failure.kind,
        },
        RuntimeError::StartService { failure } => PublicRuntimeError::StartService {
            core_kind: failure.kind,
        },
        RuntimeError::StopService { failure } => PublicRuntimeError::StopService {
            core_kind: failure.kind,
        },
        RuntimeError::RestartService { failure } => PublicRuntimeError::RestartService {
            core_kind: failure.kind,
        },
        RuntimeError::UninstallService { failure } => PublicRuntimeError::UninstallService {
            core_kind: failure.kind,
        },
        RuntimeError::MoveHost { host, failure } => PublicRuntimeError::MoveHost {
            host: *host,
            core_kind: failure.kind,
        },
        RuntimeError::ServiceHostsCore => PublicRuntimeError::ServiceHostsCore,
        RuntimeError::ConfigurationNotInEffect { .. } => {
            PublicRuntimeError::ConfigurationNotInEffect
        }
        RuntimeError::BuildRuntime { source } => PublicRuntimeError::BuildRuntime {
            source: project_runtime_build_error(source),
        },
        RuntimeError::AllocateRuntimeRevision { .. } => PublicRuntimeError::AllocateRuntimeRevision,
        RuntimeError::PublishRuntime { .. } => PublicRuntimeError::PublishRuntime,
        RuntimeError::ResolvePort { source } => PublicRuntimeError::ResolvePort {
            source: project_port_resolve_error(source),
        },
        RuntimeError::ResolveCoreBinary { source } => {
            let core = match source {
                chimera_contracts::core_spec::CoreSpecError::FindCoreBinary { core, .. } => *core,
                chimera_contracts::core_spec::CoreSpecError::InvalidPath { .. } => {
                    return PublicRuntimeError::InvalidCorePath;
                }
            };
            PublicRuntimeError::ResolveCoreBinary { core }
        }
        RuntimeError::InstallCoreBinary { source } => PublicRuntimeError::InstallCoreBinary {
            source: project_install_core_binary_error(source),
        },
        RuntimeError::PrepareServiceInstallPrompt { .. } => {
            PublicRuntimeError::PrepareServiceInstallPrompt
        }
        RuntimeError::ReadCoreVersion { source } => {
            let core = match source {
                chimera_contracts::core_version::CoreVersionError::RunCoreVersion {
                    core, ..
                }
                | chimera_contracts::core_version::CoreVersionError::CoreVersionExit {
                    core, ..
                }
                | chimera_contracts::core_version::CoreVersionError::CoreVersionNotReported {
                    core,
                } => *core,
            };
            PublicRuntimeError::ReadCoreVersion { core }
        }
        RuntimeError::CoreRejectedConfig { core_kind, .. } => {
            PublicRuntimeError::CoreRejectedConfig {
                core_kind: *core_kind,
            }
        }
        RuntimeError::CheckUnavailable { reason } => PublicRuntimeError::CheckUnavailable {
            reason: project_check_unavailable(reason),
        },
        RuntimeError::CoreRolledBack { .. } => PublicRuntimeError::CoreRolledBack,
        RuntimeError::SubmissionUnobserved { failure } => {
            PublicRuntimeError::SubmissionUnobserved {
                core_kind: failure.kind,
            }
        }
        RuntimeError::NoRuntimeConfig => PublicRuntimeError::NoRuntimeConfig,
        RuntimeError::RuntimeSnapshotChanged => PublicRuntimeError::RuntimeSnapshotChanged,
        RuntimeError::RuntimeNodeNotFound { .. } => PublicRuntimeError::RuntimeNodeNotFound,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_contracts::runtime::{CommitReceipt, DegradationPhase, RuntimeCommitStatus};
    use chimera_contracts::runtime_error::CoreFailure;
    use std::sync::Arc;

    #[test]
    fn public_mutation_preserves_receipts_and_projects_actual_degradations() {
        let outcome = MutationOutcome::from_parts(
            (),
            vec![
                Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    reason: DegradationReason::RuntimeDeferred {
                        cause: Arc::new(RuntimeError::ApplyRuntime {
                            failure: CoreFailure {
                                kind: None,
                                message: "private-token".into(),
                                retryable: true,
                                operation_id: Some("op-1".into()),
                            },
                        }),
                    },
                    message: "private-path".into(),
                    retryable: true,
                },
                Degradation {
                    phase: DegradationPhase::RuntimePublish,
                    reason: DegradationReason::RuntimeProductPublicationPending {
                        target: "private-target".into(),
                    },
                    message: "private-target".into(),
                    retryable: true,
                },
                Degradation {
                    phase: DegradationPhase::RuntimeApply,
                    reason: DegradationReason::ProfileAutoActivationFailed {
                        profile: chimera_config::profile::ProfileId("private-profile".into()),
                        cause: Box::new(ProfileFailure::VersionConflict {
                            expected: 1,
                            actual: 2,
                            cleanup: Some("private-cleanup".into()),
                        }),
                    },
                    message: "private-diagnostic".into(),
                    retryable: false,
                },
            ],
        )
        .with_commit(CommitReceipt {
            operation_id: Some("op-1".into()),
            domain: "profiles".into(),
            source_version: 7,
            runtime: RuntimeCommitStatus::Deferred,
        });
        let rpc = RpcUnitMutationOutcome::from(outcome);
        let summary = rpc.summarize_completion();
        let full = rpc
            .clone()
            .encode_for_visibility(ResponseVisibility::Full)
            .unwrap();
        let public = rpc
            .encode_for_visibility(ResponseVisibility::Public)
            .unwrap();
        assert_eq!(public["status"], "committed_degraded");
        assert_eq!(public["commits"], full["commits"]);
        assert_eq!(
            public["notifications_pending"],
            full["notifications_pending"]
        );
        assert_eq!(
            public["degradations"][0]["reason"]["cause"]["kind"],
            "apply_runtime"
        );
        assert_eq!(
            public["degradations"][1]["reason"]["code"],
            "runtime_product_publication_pending"
        );
        assert_eq!(
            public["degradations"][2]["reason"]["cause"]["kind"],
            "version_conflict"
        );
        assert!(!public.to_string().contains("private-"));
        assert!(full.to_string().contains("private-token"));
        assert!(matches!(
            summary,
            CompletionSummary::Mutation {
                degradation_count: 3,
                ..
            }
        ));
    }

    #[test]
    fn public_error_uses_typed_cause_through_context_without_leaking_text() {
        let error = anyhow::Error::new(RuntimeError::CheckUnavailable {
            reason: RuntimeCheckUnavailable::NoEndpoint {
                reason: "private-endpoint".into(),
            },
        })
        .context("private-context");
        let envelope = crate::error::application_error_for(ResponseVisibility::Public, error);
        let wire = serde_json::to_value(envelope).unwrap();
        assert_eq!(wire["domain_error"]["domain"], "runtime");
        assert_eq!(wire["domain_error"]["error"]["kind"], "check_unavailable");
        assert_eq!(
            wire["domain_error"]["error"]["reason"]["kind"],
            "no_endpoint"
        );
        assert!(!wire.to_string().contains("private-"));
    }
}
