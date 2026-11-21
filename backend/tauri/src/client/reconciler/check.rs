//! Read-only runtime candidate gate shared by typed Clash, Application and
//! Profiles checks while the source actors are migrated to ref RuntimeGate.
//!
//! This module never applies a runtime, publishes a source document, or takes
//! CoreLifecycle's mutating command lease. Source owners must still commit
//! their prepared documents before asking the lifecycle to reconcile.

use anyhow::Result;
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use chimera_contracts::{
    mutation::ConfigDomain,
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_core::state::{StateSnapshot, Version};

use crate::{
    client::{
        application_workflow::{
            mutation::{CheckRecord, RuntimeCheckFailure},
            ports::{RuntimeCheckOutcome, RuntimeCheckUnavailable as GateUnavailable},
        },
        core_lifecycle::{CoreLifecycleClient, ports::CoreLifecyclePort},
        reconciler::{inputs::RuntimeInputs, preparation::RuntimePreparation},
    },
    state::mutation::{DomainChange, ProfileContentReader, RuntimeCandidate},
};

/// Freeze the full candidate input *before* invoking the independent Core
/// validator. An unavailable backend and an invalid candidate are both
/// failures, never implicit approval.
pub(crate) async fn check_candidate(
    core: &dyn CoreLifecyclePort,
    inputs: RuntimeInputs,
    service_connected: bool,
) -> Result<CheckRecord> {
    let (app, clash, profiles, content) = inputs.into_parts().map_err(|error| {
        anyhow::Error::new(RuntimeCheckFailure::candidate_unavailable(
            error.to_string(),
        ))
    })?;
    let run_type = crate::core::RunType::classify(
        app.enable_service_mode,
        if service_connected {
            crate::core::service::ipc::IpcState::Connected
        } else {
            crate::core::service::ipc::IpcState::Disconnected
        },
    );
    core.validate_profile_runtime(clash, app.core, run_type, profiles, app, content)
        .await
}

/// Source-facing candidate verdict with its original error chain. An
/// explicitly rejected Core candidate and an unavailable/unknown result
/// cannot be collapsed into the same textual `anyhow` error.
#[derive(Debug, thiserror::Error)]
#[error("independent {domain} runtime candidate check did not pass: {record:?}")]
pub(crate) struct RuntimeGateVerdict {
    pub(crate) domain: String,
    pub(crate) record: CheckRecord,
    #[source]
    pub(crate) source: Option<anyhow::Error>,
}

impl RuntimeGateVerdict {
    /// Preserve a typed CoreCheck failure if the backend supplied one;
    /// otherwise lack of a check verdict remains unserviceable, never a
    /// deterministic rejection inferred from arbitrary error text.
    fn from_error(domain: &str, error: anyhow::Error) -> Self {
        let record = error
            .downcast_ref::<RuntimeCheckFailure>()
            .map(RuntimeCheckFailure::record)
            .unwrap_or_else(|| CheckRecord::Unserviceable(error.to_string()));
        Self {
            domain: domain.into(),
            record,
            source: Some(error),
        }
    }

    /// Convert one source verdict into the ref's typed RuntimeGate failure.
    fn runtime_error(&self) -> RuntimeError {
        let detailed = self
            .source
            .as_ref()
            .and_then(|error| error.downcast_ref::<RuntimeCheckFailure>());
        let rejected = matches!(&self.record, CheckRecord::Rejected(_))
            || detailed.is_some_and(|failure| {
                matches!(failure.outcome, RuntimeCheckOutcome::Rejected { .. })
            });
        if rejected {
            let (core_kind, message) = match detailed.map(|failure| &failure.outcome) {
                Some(RuntimeCheckOutcome::Rejected { kind, message }) => (*kind, message.clone()),
                _ => (
                    None,
                    match &self.record {
                        CheckRecord::Rejected(message) => message.clone(),
                        _ => self.to_string(),
                    },
                ),
            };
            RuntimeError::CoreRejectedConfig { core_kind, message }
        } else {
            let reason = match detailed.map(|failure| &failure.outcome) {
                Some(RuntimeCheckOutcome::Unavailable(GateUnavailable::Backend {
                    kind,
                    message,
                    retryable,
                })) => RuntimeCheckUnavailable::Backend {
                    kind: *kind,
                    message: message.clone(),
                    retryable: *retryable,
                },
                Some(RuntimeCheckOutcome::Unavailable(GateUnavailable::CandidateUnavailable {
                    reason,
                })) => RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: reason.clone(),
                },
                Some(RuntimeCheckOutcome::Unavailable(GateUnavailable::NoEndpoint { reason })) => {
                    RuntimeCheckUnavailable::NoEndpoint {
                        reason: reason.clone(),
                    }
                }
                Some(RuntimeCheckOutcome::Unavailable(GateUnavailable::HostUnsupported {
                    host,
                    reason,
                })) => RuntimeCheckUnavailable::HostUnsupported {
                    host: format!("{host:?}"),
                    reason: reason.clone(),
                },
                _ => match &self.record {
                    CheckRecord::Skipped(reason) | CheckRecord::Unserviceable(reason) => {
                        RuntimeCheckUnavailable::NotChecked {
                            reason: reason.clone(),
                        }
                    }
                    CheckRecord::NotOwed => RuntimeCheckUnavailable::NotChecked {
                        reason: "candidate check not owed".into(),
                    },
                    _ => RuntimeCheckUnavailable::Other {
                        reason: self.to_string(),
                    },
                },
            };
            RuntimeError::CheckUnavailable { reason }
        }
    }
}

pub(crate) fn typed_gate_error(domain: &str, error: anyhow::Error) -> std::sync::Arc<RuntimeError> {
    std::sync::Arc::new(RuntimeGateVerdict::from_error(domain, error).runtime_error())
}

fn typed_gate_record(domain: &str, record: CheckRecord) -> std::sync::Arc<RuntimeError> {
    std::sync::Arc::new(
        RuntimeGateVerdict {
            domain: domain.into(),
            record,
            source: None,
        }
        .runtime_error(),
    )
}

/// A source may commit a runtime-affecting candidate only after a real pass.
/// In particular, NotOwed/Skipped are not interchangeable with Passed here.
pub(crate) fn require_passed(record: CheckRecord, domain: &str) -> Result<()> {
    match record {
        CheckRecord::Passed => Ok(()),
        other => Err(anyhow::Error::new(RuntimeGateVerdict {
            domain: domain.into(),
            record: other,
            source: None,
        })),
    }
}

/// The one ref-style read-only admission path for all three candidate
/// documents. Source owners supply their proposed domain document; the other
/// two domains use their committed versions. For Application/Clash, the
/// Profiles document and file bytes are captured under the same content fence.
/// This function never persists a source or applies a running Core.
fn domain_name(change: &DomainChange) -> &'static str {
    match change {
        DomainChange::Application { .. } => "application",
        DomainChange::Clash { .. } => "clash",
        DomainChange::Profiles { .. } => "profiles",
    }
}

pub(crate) async fn check_domain_candidate(
    core: &dyn CoreLifecyclePort,
    committed_app: ChimeraAppConfig,
    committed_clash: ClashConfig,
    profiles: &StateSnapshot<Profiles>,
    profile_content: Option<&ProfileContentReader>,
    candidate: RuntimeCandidate,
    service_connected: bool,
) -> Result<(), std::sync::Arc<RuntimeError>> {
    let domain = domain_name(&candidate.change);
    let app = match &candidate.change {
        DomainChange::Application { candidate } => candidate.as_ref().clone(),
        _ => committed_app,
    };
    let clash = match &candidate.change {
        DomainChange::Clash { candidate } => candidate.as_ref().clone(),
        _ => committed_clash,
    };
    let profiles_dir =
        crate::utils::dirs::app_profiles_dir().map_err(|error| typed_gate_error(domain, error))?;
    let source = crate::enhance::FsProfileContentSource::new(profiles_dir);
    let preparation = RuntimePreparation::new(profile_content, Some(profiles), &source);
    let inputs = match &candidate.change {
        DomainChange::Profiles {
            candidate: proposed,
        } => {
            preparation
                .capture_inputs(app, clash, proposed.clone(), candidate.staged)
                .await
        }
        _ => preparation
            .capture_committed_inputs(app, clash)
            .await
            .map(|mut inputs| {
                for (path, content) in candidate.staged {
                    inputs.content.0.insert(path, Ok(content));
                }
                inputs
            }),
    }
    .map_err(|error| typed_gate_error(domain, error))?;
    let checked = check_candidate(core, inputs, service_connected)
        .await
        .map_err(|error| typed_gate_error(domain, error))?;
    match checked {
        CheckRecord::Passed => Ok(()),
        other => Err(typed_gate_record(domain, other)),
    }
}

/// Ref CandidateCheck boundary adapted to Chimera's typed source clients.
pub(crate) struct CandidateCheck {
    core: std::sync::Arc<dyn CoreLifecyclePort>,
    application: super::super::application::ApplicationClient,
    clash: super::super::clash_config::ClashConfigClient,
    profile_content: crate::state::mutation::ProfileContentReader,
    profiles: StateSnapshot<Profiles>,
    lifecycle: CoreLifecycleClient,
}

impl CandidateCheck {
    pub(crate) fn new(
        core: std::sync::Arc<dyn CoreLifecyclePort>,
        application: super::super::application::ApplicationClient,
        clash: super::super::clash_config::ClashConfigClient,
        profile_content: crate::state::mutation::ProfileContentReader,
        profiles: StateSnapshot<Profiles>,
        lifecycle: CoreLifecycleClient,
    ) -> Self {
        Self {
            core,
            application,
            clash,
            profile_content,
            profiles,
            lifecycle,
        }
    }
}

#[async_trait::async_trait]
impl crate::state::mutation::RuntimeGate for CandidateCheck {
    async fn check(
        &self,
        candidate: crate::state::mutation::RuntimeCandidate,
    ) -> Result<(), std::sync::Arc<RuntimeError>> {
        let domain = domain_name(&candidate.change);
        let clash = match &candidate.change {
            DomainChange::Clash { candidate } => candidate.as_ref().clone(),
            _ => self
                .clash
                .get()
                .map_err(|error| typed_gate_error(domain, error))?,
        };
        check_domain_candidate(
            self.core.as_ref(),
            self.application.get_typed(),
            clash,
            &self.profiles,
            Some(&self.profile_content),
            candidate,
            crate::core::service::ipc::get_ipc_state().is_connected(),
        )
        .await
    }

    fn committed(&self, domain: ConfigDomain, version: Version) {
        use crate::client::reconciler::committed::SourceDomain;
        let source = match domain {
            ConfigDomain::Application => SourceDomain::Application,
            ConfigDomain::Clash => SourceDomain::Clash,
            ConfigDomain::Profiles => SourceDomain::Profiles,
        };
        // Only a durable, published source commit reaches this path. The
        // existing lifecycle mailbox still owns runtime application/retry.
        self.lifecycle
            .source_committed(source, *version.as_ref(), true);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use chimera_config::{
        application::{ChimeraAppConfig, ClashCore},
        clash::config::ClashConfig,
        profile::Profiles,
    };

    use super::*;
    use crate::{client::reconciler::inputs::FrozenProfileContent, core::RunType};

    #[derive(Default)]
    struct RecordingChecker {
        runs: Mutex<Vec<RunType>>,
        calls: AtomicUsize,
        reject: bool,
    }

    #[async_trait::async_trait]
    impl CoreLifecyclePort for RecordingChecker {
        async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> Result<()> {
            panic!("candidate checking must not reconcile");
        }

        async fn stop(&self) -> Result<()> {
            panic!("candidate checking must not stop the core");
        }

        async fn change_core(&self, _: ClashCore) -> Result<()> {
            panic!("candidate checking must not switch binaries");
        }

        async fn status(&self) -> Result<crate::client::core_lifecycle::ports::CoreStatusSnapshot> {
            panic!("candidate checking must not probe the mutable lifecycle");
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            None
        }

        async fn on_profile_change(&self, _: bool) {
            panic!("candidate checking must not interrupt connections");
        }

        async fn validate_profile_runtime(
            &self,
            _: ClashConfig,
            _: ClashCore,
            run_type: RunType,
            _: Arc<Profiles>,
            app: ChimeraAppConfig,
            _: BTreeMap<String, String>,
        ) -> Result<CheckRecord> {
            assert!(app.enable_service_mode);
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.runs.lock().unwrap().push(run_type);
            if self.reject {
                Ok(CheckRecord::Rejected("invalid candidate".into()))
            } else {
                Ok(CheckRecord::Passed)
            }
        }
    }

    fn inputs() -> RuntimeInputs {
        RuntimeInputs {
            app: ChimeraAppConfig {
                enable_service_mode: true,
                ..Default::default()
            },
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: FrozenProfileContent::default(),
        }
    }

    #[tokio::test]
    async fn independent_gate_uses_requested_host_without_mutating_core() {
        let checker = RecordingChecker::default();
        let checked = check_candidate(&checker, inputs(), true).await.unwrap();
        require_passed(checked, "application").unwrap();
        assert_eq!(checker.calls.load(Ordering::SeqCst), 1);
        assert_eq!(*checker.runs.lock().unwrap(), [RunType::Service]);

        let checked = check_candidate(&checker, inputs(), false).await.unwrap();
        require_passed(checked, "clash").unwrap();
        assert_eq!(
            *checker.runs.lock().unwrap(),
            [RunType::Service, RunType::Normal]
        );
    }

    #[tokio::test]
    async fn rejected_core_and_unreadable_frozen_content_fail_closed() {
        let checker = RecordingChecker {
            reject: true,
            ..Default::default()
        };
        let result = check_candidate(&checker, inputs(), true).await.unwrap();
        assert!(
            require_passed(result, "clash")
                .unwrap_err()
                .to_string()
                .contains("invalid candidate")
        );

        let mut unreadable = inputs();
        unreadable
            .content
            .0
            .insert("active.yaml".into(), Err("content read failed".into()));
        let error = check_candidate(&checker, unreadable, true)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("content read failed"));
        assert_eq!(
            checker.calls.load(Ordering::SeqCst),
            1,
            "the lower checker must never receive incomplete content"
        );
    }

    #[test]
    fn typed_core_rejection_keeps_a_rejection_and_unavailable_checks_do_not() {
        use chimera_core_manager::{CoreError, CoreErrorKind};

        let rejected = RuntimeGateVerdict::from_error(
            "profiles",
            anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                CoreErrorKind::ConfigCheckFailed,
                "invalid profile target",
                false,
            ))),
        );
        assert_eq!(
            rejected.record,
            CheckRecord::Rejected("invalid profile target".into())
        );
        let unavailable = RuntimeGateVerdict::from_error(
            "profiles",
            anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "validator unreachable",
                true,
            ))),
        );
        assert_eq!(
            unavailable.record,
            CheckRecord::Unserviceable("validator unreachable".into())
        );
        assert!(unavailable.source.is_some());
        let opaque = RuntimeGateVerdict::from_error(
            "profiles",
            anyhow::anyhow!("unknown validation RPC failure"),
        );
        assert!(matches!(opaque.record, CheckRecord::Unserviceable(_)));
        assert!(opaque.source.is_some());
    }

    #[test]
    fn shared_domain_gate_preserves_rejection_vs_unchecked_evidence() {
        use chimera_core_manager::{CoreError, CoreErrorKind};

        let rejected = typed_gate_error(
            "application",
            anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                CoreErrorKind::ConfigCheckFailed,
                "invalid proposed application",
                false,
            ))),
        );
        assert!(matches!(
            rejected.as_ref(),
            RuntimeError::CoreRejectedConfig {
                core_kind: Some(CoreErrorKind::ConfigCheckFailed),
                ..
            }
        ));
        let unavailable = typed_gate_error(
            "clash",
            anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                CoreErrorKind::BackendUnavailable,
                "validator disconnected",
                true,
            ))),
        );
        assert!(matches!(
            unavailable.as_ref(),
            RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::Backend {
                    retryable: true,
                    kind: Some(CoreErrorKind::BackendUnavailable),
                    ..
                }
            }
        ));
        for skipped in [
            CheckRecord::NotOwed,
            CheckRecord::Skipped("not supported".into()),
            CheckRecord::Unserviceable("no verdict".into()),
        ] {
            assert!(matches!(
                typed_gate_record("profiles", skipped).as_ref(),
                RuntimeError::CheckUnavailable { .. }
            ));
        }
    }

    #[test]
    fn a_source_requires_a_positive_runtime_check() {
        assert!(require_passed(CheckRecord::Passed, "clash").is_ok());
        for outcome in [
            CheckRecord::NotOwed,
            CheckRecord::Skipped("not checked".into()),
            CheckRecord::Unserviceable("backend unavailable".into()),
            CheckRecord::Rejected("core rejected candidate".into()),
        ] {
            let error = require_passed(outcome, "application").unwrap_err();
            assert!(error.to_string().contains("did not pass"));
        }
    }
}
