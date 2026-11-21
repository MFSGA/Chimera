//! Independent candidate validation; only successful checks and commits reach the workflow.
use std::sync::Arc;

use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use chimera_contracts::{
    mutation::ConfigDomain,
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_core::state::{StateSnapshot, Version};

use super::{
    ports::{RuntimeBuildPort, RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeValidatorPort},
    preparation::RuntimePreparation,
};
use crate::{
    client::{
        core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort},
        ports::SessionPortResolver,
        runtime::RuntimeRevisionAllocator,
    },
    state::mutation::{DomainChange, RuntimeCandidate, RuntimeGate},
};

/// The checked-candidate inbox that the single runtime owner must implement.
/// This contract intentionally does NOT provide a no-op implementation. The
/// source commit may proceed only if that owner has retained the exact checked
/// product and can later pair it with a committed source version.
pub(in crate::client) trait CheckedCandidatePort: Send + Sync + 'static {
    fn checked(&self, candidate: PreparedRuntime) -> Result<(), Arc<RuntimeError>>;
    fn source_committed(&self, domain: ConfigDomain, version: Version);
}

pub(in crate::client) struct CandidateCheck {
    application: StateSnapshot<ChimeraAppConfig>,
    clash: StateSnapshot<ClashConfig>,
    preparation: RuntimePreparation,
    validator: Arc<dyn RuntimeValidatorPort>,
    workflow: Arc<dyn CheckedCandidatePort>,
}

pub(in crate::client) struct Args {
    pub application: StateSnapshot<ChimeraAppConfig>,
    pub clash: StateSnapshot<ClashConfig>,
    pub profile_content: crate::state::mutation::ProfileContentReader,
    pub builder: Arc<dyn RuntimeBuildPort>,
    pub ports: Arc<SessionPortResolver>,
    pub revisions: RuntimeRevisionAllocator,
    pub validator: Arc<dyn RuntimeValidatorPort>,
    pub workflow: Arc<dyn CheckedCandidatePort>,
}

impl CandidateCheck {
    pub(in crate::client) fn new(args: Args) -> Self {
        let Args {
            application,
            clash,
            profile_content,
            builder,
            ports,
            revisions,
            validator,
            workflow,
        } = args;
        let preparation =
            RuntimePreparation::new(profile_content, builder, ports).with_revisions(revisions);
        Self {
            application,
            clash,
            preparation,
            validator,
            workflow,
        }
    }
}

#[async_trait::async_trait]
impl RuntimeGate for CandidateCheck {
    async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
        let app = match &candidate.change {
            DomainChange::Application { candidate } => candidate.as_ref().clone(),
            _ => self.application.load().state.clone(),
        };
        let clash = match &candidate.change {
            DomainChange::Clash { candidate } => candidate.as_ref().clone(),
            _ => self.clash.load().state.clone(),
        };
        let mut inputs = match &candidate.change {
            DomainChange::Profiles { candidate } => {
                self.preparation
                    .capture_inputs(app, clash, candidate.clone())
                    .await
            }
            _ => {
                self.preparation
                    .capture_committed_inputs(app, clash)
                    .await
                    .1
            }
        };
        for (path, content) in candidate.staged {
            inputs.content.0.insert(path, Ok(content));
        }
        let prepared = self
            .preparation
            .prepare_candidate_inputs(inputs)
            .await
            .map_err(|error| {
                Arc::new(RuntimeError::CheckUnavailable {
                    reason: RuntimeCheckUnavailable::CandidateUnavailable {
                        reason: error.to_string(),
                    },
                })
            })?;
        let core_spec = self
            .preparation
            .core_spec(&prepared.snapshot.target_core)
            .map_err(|source| Arc::new(RuntimeError::ResolveCoreBinary { source }))?;
        match self
            .validator
            .check(RuntimeCheckRequest {
                core_spec,
                intent: &prepared.intent,
            })
            .await
        {
            RuntimeCheckOutcome::Passed => self.workflow.checked(prepared)?,
            RuntimeCheckOutcome::Rejected { kind, message } => {
                return Err(Arc::new(RuntimeError::CoreRejectedConfig {
                    core_kind: kind,
                    message,
                }));
            }
            RuntimeCheckOutcome::Unavailable(reason) => {
                return Err(Arc::new(RuntimeError::CheckUnavailable { reason }));
            }
        }
        Ok(())
    }

    fn committed(&self, domain: ConfigDomain, version: Version) {
        self.workflow.source_committed(domain, version);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use chimera_config::{
        application::ClashCore, profile::Profiles, runtime::executor::ResolvedPortBindings,
    };
    use chimera_contracts::runtime_error::RuntimeCheckUnavailable;
    use chimera_core::{
        client::{core_spec::CoreSpecError, runtime_build::RuntimeBuildError},
        state::PersistentStateManagerSetup,
    };
    use chimera_core_manager::{CoreErrorKind, CoreKind, CoreSpec};

    use crate::{
        client::{
            reconciler::inputs::{FrozenProfileContent, RuntimeInputs},
            runtime::{RuntimeLifecycle, RuntimeSnapshot},
        },
        state::mutation::{ProfileContentLock, ProfileContentReader},
    };

    #[derive(Default)]
    struct StubBuilder {
        prepared: Mutex<Vec<(bool, String)>>,
        reject_core_spec: AtomicBool,
    }

    #[async_trait::async_trait]
    impl RuntimeBuildPort for StubBuilder {
        async fn capture_content(&self, _: &Profiles) -> FrozenProfileContent {
            FrozenProfileContent::default()
        }

        fn core_spec(&self, core: &ClashCore) -> Result<CoreSpec, CoreSpecError> {
            if self.reject_core_spec.load(Ordering::Relaxed) {
                return Err(CoreSpecError::FindCoreBinary {
                    core: *core,
                    source: std::io::Error::new(std::io::ErrorKind::NotFound, "core unavailable"),
                });
            }
            Ok(CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("/tmp/ref-test-mihomo"),
                version: None,
                features: Vec::new(),
            })
        }

        async fn build(
            &self,
            revision: crate::client::runtime::RuntimeRevision,
            inputs: RuntimeInputs,
            _: ResolvedPortBindings,
            strict_transforms: bool,
        ) -> Result<Arc<RuntimeSnapshot>, RuntimeBuildError> {
            self.prepared.lock().unwrap().push((
                strict_transforms,
                inputs.target_key().expect("fixture has a target"),
            ));
            Ok(Arc::new(RuntimeSnapshot::new(
                revision,
                inputs.app.core,
                b"mode: rule\n".to_vec(),
                serde_yaml::Mapping::new(),
            )))
        }

        async fn publish(
            &self,
            _: &RuntimeSnapshot,
        ) -> Result<(), chimera_contracts::publish::PublishRuntimeError> {
            panic!("the source precommit gate must never publish a product");
        }
    }

    struct StubValidator {
        outcome: RuntimeCheckOutcome,
        checked: Mutex<Vec<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl RuntimeValidatorPort for StubValidator {
        async fn check(&self, request: RuntimeCheckRequest<'_>) -> RuntimeCheckOutcome {
            self.checked.lock().unwrap().push((
                request.intent.digest.clone(),
                request.intent.config_text.to_string(),
            ));
            self.outcome.clone()
        }
    }

    #[derive(Default)]
    struct RecordingInbox {
        checked: Mutex<Vec<PreparedRuntime>>,
        committed: Mutex<Vec<(ConfigDomain, Version)>>,
        reject: bool,
    }

    impl CheckedCandidatePort for RecordingInbox {
        fn checked(&self, candidate: PreparedRuntime) -> Result<(), Arc<RuntimeError>> {
            if self.reject {
                return Err(Arc::new(RuntimeError::ShuttingDown));
            }
            self.checked.lock().unwrap().push(candidate);
            Ok(())
        }

        fn source_committed(&self, domain: ConfigDomain, version: Version) {
            self.committed.lock().unwrap().push((domain, version));
        }
    }

    async fn gate(
        outcome: RuntimeCheckOutcome,
        reject_owner: bool,
    ) -> (
        CandidateCheck,
        Arc<StubBuilder>,
        Arc<StubValidator>,
        Arc<RecordingInbox>,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let path = |name: &str| camino::Utf8PathBuf::from_path_buf(temp.path().join(name)).unwrap();
        let app = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(path("app.yaml"))
            .assemble()
            .from_state(ChimeraAppConfig::default())
            .await
            .unwrap();
        let clash = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path("clash.yaml"))
            .assemble()
            .from_state(ClashConfig::default())
            .await
            .unwrap();
        let profiles = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(path("profiles.yaml"))
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let builder = Arc::new(StubBuilder::default());
        let validator = Arc::new(StubValidator {
            outcome,
            checked: Mutex::new(Vec::new()),
        });
        let inbox = Arc::new(RecordingInbox {
            reject: reject_owner,
            ..Default::default()
        });
        let checker = CandidateCheck::new(Args {
            application: app.snapshot_handle(),
            clash: clash.snapshot_handle(),
            profile_content: ProfileContentReader::new(
                profiles.snapshot_handle(),
                ProfileContentLock::default(),
            ),
            builder: builder.clone(),
            ports: Arc::new(SessionPortResolver::new(Arc::new(
                RuntimeLifecycle::default(),
            ))),
            revisions: RuntimeRevisionAllocator::default(),
            validator: validator.clone(),
            workflow: inbox.clone(),
        });
        (checker, builder, validator, inbox)
    }

    fn candidate() -> RuntimeCandidate {
        RuntimeCandidate {
            change: DomainChange::Application {
                candidate: Arc::new(ChimeraAppConfig::default()),
            },
            staged: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn passed_validation_hands_exact_prepared_bytes_to_owner_before_commit() {
        let (gate, builder, validator, inbox) = gate(RuntimeCheckOutcome::Passed, false).await;
        gate.check(candidate()).await.unwrap();
        let prepared = inbox.checked.lock().unwrap();
        assert_eq!(prepared.len(), 1);
        let build = &prepared[0];
        let validation = validator.checked.lock().unwrap();
        assert_eq!(
            validation.as_slice(),
            &[(
                build.intent.digest.clone(),
                String::from_utf8(build.snapshot.product_bytes().to_vec()).unwrap(),
            )]
        );
        assert_eq!(
            build.intent.config_text.as_bytes(),
            build.snapshot.product_bytes()
        );
        assert_eq!(builder.prepared.lock().unwrap().len(), 1);
        assert!(
            builder.prepared.lock().unwrap()[0].0,
            "candidates must use strict transform validation"
        );
        assert!(inbox.committed.lock().unwrap().is_empty());
        drop(validation);
        drop(prepared);
        gate.committed(ConfigDomain::Application, Version::new(7));
        assert_eq!(
            inbox.committed.lock().unwrap().as_slice(),
            &[(ConfigDomain::Application, Version::new(7))]
        );
    }

    #[tokio::test]
    async fn core_rejection_and_unavailable_verdict_never_queue_checked_candidate() {
        for outcome in [
            RuntimeCheckOutcome::Rejected {
                kind: Some(CoreErrorKind::ConfigCheckFailed),
                message: "invalid candidate".into(),
            },
            RuntimeCheckOutcome::Unavailable(RuntimeCheckUnavailable::Backend {
                kind: Some(CoreErrorKind::BackendUnavailable),
                message: "no endpoint".into(),
                retryable: true,
            }),
        ] {
            let (gate, _, _, inbox) = gate(outcome.clone(), false).await;
            let error = gate.check(candidate()).await.unwrap_err();
            assert!(inbox.checked.lock().unwrap().is_empty());
            assert!(inbox.committed.lock().unwrap().is_empty());
            match outcome {
                RuntimeCheckOutcome::Rejected { .. } => assert!(matches!(
                    error.as_ref(),
                    RuntimeError::CoreRejectedConfig {
                        core_kind: Some(CoreErrorKind::ConfigCheckFailed),
                        ..
                    }
                )),
                RuntimeCheckOutcome::Unavailable(_) => assert!(matches!(
                    error.as_ref(),
                    RuntimeError::CheckUnavailable {
                        reason: RuntimeCheckUnavailable::Backend {
                            retryable: true,
                            ..
                        }
                    }
                )),
                RuntimeCheckOutcome::Passed => unreachable!(),
            }
        }
    }

    #[tokio::test]
    async fn missing_core_binary_preserves_typed_source_and_never_validates_or_commits() {
        let (gate, builder, validator, inbox) = gate(RuntimeCheckOutcome::Passed, false).await;
        builder.reject_core_spec.store(true, Ordering::Relaxed);
        let error = gate.check(candidate()).await.unwrap_err();
        assert!(matches!(
            error.as_ref(),
            RuntimeError::ResolveCoreBinary { .. }
        ));
        assert_eq!(error.core_kind(), Some(CoreErrorKind::BinaryNotFound));
        assert_eq!(validator.checked.lock().unwrap().len(), 0);
        assert!(inbox.checked.lock().unwrap().is_empty());
        assert!(inbox.committed.lock().unwrap().is_empty());
        let wire = serde_json::to_value(error).unwrap();
        assert_eq!(wire["kind"], "resolve_core_binary");
        assert_eq!(wire["source"]["kind"], "find_core_binary");
    }

    #[tokio::test]
    async fn a_closed_owner_refuses_precommit_even_after_core_check_passes() {
        let (gate, _, validator, inbox) = gate(RuntimeCheckOutcome::Passed, true).await;
        let error = gate.check(candidate()).await.unwrap_err();
        assert!(matches!(error.as_ref(), RuntimeError::ShuttingDown));
        assert_eq!(validator.checked.lock().unwrap().len(), 1);
        assert!(inbox.checked.lock().unwrap().is_empty());
        assert!(inbox.committed.lock().unwrap().is_empty());
    }
}
