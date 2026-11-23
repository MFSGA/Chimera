//! Independent candidate validation; only successful checks and commits reach the workflow.
use std::sync::Arc;

use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use chimera_contracts::{
    mutation::ConfigDomain,
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_core::state::Version;
use chimera_state_engine::StateSnapshot;

use super::{
    ports::{RuntimeBuildPort, RuntimeCheckOutcome, RuntimeCheckRequest, RuntimeValidatorPort},
    preparation::RuntimePreparation,
};
use crate::{
    client::{
        core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort},
        ports::SessionPortResolver,
    },
    state::mutation::{DomainChange, RuntimeCandidate, RuntimeGate},
};

/// The checked-candidate inbox that the single runtime owner must implement.
/// This contract intentionally does NOT provide a no-op implementation. The
/// source commit may proceed only if that owner has retained the exact checked
/// product and can later pair it with a committed source version.
#[async_trait::async_trait]
pub(in crate::client) trait CheckedCandidatePort: Send + Sync + 'static {
    /// Admission is acknowledged by the sole runtime owner before source commit.
    /// An enqueued-but-unhandled message is not proof of ownership.
    async fn checked(
        &self,
        domain: ConfigDomain,
        candidate: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<RuntimeError>>;
    fn source_committed(&self, domain: ConfigDomain, version: Version);
    /// Failed source CAS after an ACK must not leave a permanent proposal.
    /// Until the ref operation token travels through every source actor,
    /// conservatively abort the entire matching domain only.
    fn abort_uncommitted(&self, domain: ConfigDomain);
    /// A source-owned request can identify its exact prepared product without
    /// evicting concurrent successful checks for the same domain.
    async fn checked_for_request(
        &self,
        domain: ConfigDomain,
        candidate: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        request: String,
        /// Source-owned explicit mode transition, proven by the same Clash
        /// patch plan that authorizes the durable CAS. The Core executor
        /// must never re-infer this one-shot effect from current config.
        interrupt_mode: bool,
    ) -> Result<(), Arc<RuntimeError>>;
    fn source_committed_for_request(&self, domain: ConfigDomain, version: Version, request: String);
    fn abort_uncommitted_for_request(&self, domain: ConfigDomain, request: String);
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
    /// Both handles must originate from the actual Core owner. Splitting
    /// the allocator and resolver would silently fork the runtime session.
    pub session: crate::client::ports::RuntimePreparationSession,
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
            session,
            validator,
            workflow,
        } = args;
        let preparation = RuntimePreparation::new(profile_content, builder, session);
        Self {
            application,
            clash,
            preparation,
            validator,
            workflow,
        }
    }
}

impl CandidateCheck {
    /// The same strict ref preparation and Core validation shared by every
    /// source. Only a production RuntimeGate check may admit the result into
    /// the owner's Checked mailbox; legacy postcommit paths cannot do so.
    async fn validate(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<
        (
            ConfigDomain,
            PreparedRuntime,
            chimera_core_manager::CoreSpec,
        ),
        Arc<RuntimeError>,
    > {
        let domain = match &candidate.change {
            DomainChange::Application { .. } => ConfigDomain::Application,
            DomainChange::Clash { .. } => ConfigDomain::Clash,
            DomainChange::Profiles { .. } => ConfigDomain::Profiles,
        };
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
                core_spec: core_spec.clone(),
                intent: &prepared.intent,
            })
            .await
        {
            RuntimeCheckOutcome::Passed => Ok((domain, prepared, core_spec)),
            RuntimeCheckOutcome::Rejected { kind, message } => {
                Err(Arc::new(RuntimeError::CoreRejectedConfig {
                    core_kind: kind,
                    message,
                }))
            }
            RuntimeCheckOutcome::Unavailable(reason) => {
                Err(Arc::new(RuntimeError::CheckUnavailable { reason }))
            }
        }
    }

    /// Migration bridge for legacy postcommit Application/Clash workflows:
    /// use the upstream candidate builder and validator, but do not enqueue
    /// checked bytes that the old apply path cannot consume safely yet.
    pub(in crate::client) async fn validate_only(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<(), Arc<RuntimeError>> {
        self.validate(candidate).await.map(|_| ())
    }

    /// Return a source-owned receipt only after the sole Core actor has ACKed
    /// the specific checked bytes. The source must return this receipt on CAS
    /// success or failure; no other same-domain proposal may be consumed.
    pub(in crate::client) async fn check_with_receipt(
        &self,
        candidate: RuntimeCandidate,
    ) -> Result<String, Arc<RuntimeError>> {
        self.check_with_receipt_effect(candidate, false).await
    }

    /// Explicit one-shot mode interruption is attached to the exact checked
    /// request *before* source CAS, and is discarded with that same request
    /// on conflict. It is not a second postcommit Core writer.
    pub(in crate::client) async fn check_with_receipt_effect(
        &self,
        candidate: RuntimeCandidate,
        interrupt_mode: bool,
    ) -> Result<String, Arc<RuntimeError>> {
        let (domain, prepared, core_spec) = self.validate(candidate).await?;
        if interrupt_mode && domain != ConfigDomain::Clash {
            return Err(Arc::new(RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::CandidateUnavailable {
                    reason: "mode interruption is only valid for a committed Clash source patch"
                        .into(),
                },
            }));
        }
        let request = chimera_core_manager::OperationId::generate().to_string();
        self.workflow
            .checked_for_request(domain, prepared, core_spec, request.clone(), interrupt_mode)
            .await?;
        Ok(request)
    }

    pub(in crate::client) fn committed_with_receipt(
        &self,
        domain: ConfigDomain,
        version: Version,
        request: String,
    ) {
        self.workflow
            .source_committed_for_request(domain, version, request);
    }

    pub(in crate::client) fn abort_with_receipt(&self, domain: ConfigDomain, request: String) {
        self.workflow.abort_uncommitted_for_request(domain, request);
    }

    pub(in crate::client) fn abort_uncommitted(&self, domain: ConfigDomain) {
        self.workflow.abort_uncommitted(domain);
    }
}

#[async_trait::async_trait]
impl RuntimeGate for CandidateCheck {
    async fn check(&self, candidate: RuntimeCandidate) -> Result<(), Arc<RuntimeError>> {
        let (domain, prepared, core_spec) = self.validate(candidate).await?;
        self.workflow.checked(domain, prepared, core_spec).await
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
    use chimera_core::client::{core_spec::CoreSpecError, runtime_build::RuntimeBuildError};
    use chimera_core::state::PersistentStateManagerSetup as ProfilesManagerSetup;
    use chimera_core_manager::{CoreErrorKind, CoreKind, CoreSpec};
    use chimera_state_engine::PersistentStateManagerSetup;

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
        domains: Mutex<Vec<ConfigDomain>>,
        committed: Mutex<Vec<(ConfigDomain, Version)>>,
        reject: bool,
    }

    #[async_trait::async_trait]
    impl CheckedCandidatePort for RecordingInbox {
        async fn checked(
            &self,
            domain: ConfigDomain,
            candidate: PreparedRuntime,
            core_spec: chimera_core_manager::CoreSpec,
        ) -> Result<(), Arc<RuntimeError>> {
            assert_eq!(core_spec.kind, chimera_core_manager::CoreKind::Mihomo);
            if self.reject {
                return Err(Arc::new(RuntimeError::ShuttingDown));
            }
            self.checked.lock().unwrap().push(candidate);
            self.domains.lock().unwrap().push(domain);
            Ok(())
        }

        fn source_committed(&self, domain: ConfigDomain, version: Version) {
            self.committed.lock().unwrap().push((domain, version));
        }
        fn abort_uncommitted(&self, _domain: ConfigDomain) {}
        async fn checked_for_request(
            &self,
            domain: ConfigDomain,
            candidate: PreparedRuntime,
            core_spec: chimera_core_manager::CoreSpec,
            _request: String,
            _interrupt_mode: bool,
        ) -> Result<(), Arc<RuntimeError>> {
            self.checked(domain, candidate, core_spec).await
        }
        fn source_committed_for_request(
            &self,
            domain: ConfigDomain,
            version: Version,
            _request: String,
        ) {
            self.source_committed(domain, version);
        }
        fn abort_uncommitted_for_request(&self, _domain: ConfigDomain, _request: String) {}
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
            .from_state(ChimeraAppConfig::default());
        let clash = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path("clash.yaml"))
            .assemble()
            .from_state(ClashConfig::default());
        let profiles = ProfilesManagerSetup::<Profiles>::builder()
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
            session: {
                let lifecycle = Arc::new(RuntimeLifecycle::default());
                crate::client::ports::RuntimePreparationSession {
                    revisions: lifecycle.revision_allocator(),
                    ports: Arc::new(SessionPortResolver::new(lifecycle)),
                }
            },
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
        assert_eq!(
            inbox.domains.lock().unwrap().as_slice(),
            &[ConfigDomain::Application]
        );
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
    async fn read_only_clash_validation_does_not_admit_checked_bytes() {
        let (gate, builder, validator, inbox) = gate(RuntimeCheckOutcome::Passed, false).await;
        let clash = RuntimeCandidate {
            change: DomainChange::Clash {
                candidate: Arc::new(ClashConfig::default()),
            },
            staged: BTreeMap::new(),
        };
        gate.validate_only(clash).await.unwrap();
        assert_eq!(builder.prepared.lock().unwrap().len(), 1);
        assert_eq!(validator.checked.lock().unwrap().len(), 1);
        assert!(inbox.checked.lock().unwrap().is_empty());
        assert!(inbox.domains.lock().unwrap().is_empty());
        assert!(inbox.committed.lock().unwrap().is_empty());

        // The real RuntimeGate path still admits a newly validated candidate;
        // validation-only calls never populate or consume its Checked inbox.
        gate.check(RuntimeCandidate {
            change: DomainChange::Clash {
                candidate: Arc::new(ClashConfig::default()),
            },
            staged: BTreeMap::new(),
        })
        .await
        .unwrap();
        assert_eq!(inbox.checked.lock().unwrap().len(), 1);
        assert_eq!(
            inbox.domains.lock().unwrap().as_slice(),
            &[ConfigDomain::Clash]
        );
    }

    #[tokio::test]
    async fn application_validation_only_checks_core_without_queuing_a_runtime_apply() {
        let (accepted_gate, builder, validator, inbox) =
            gate(RuntimeCheckOutcome::Passed, false).await;
        accepted_gate.validate_only(candidate()).await.unwrap();
        assert_eq!(builder.prepared.lock().unwrap().len(), 1);
        assert_eq!(validator.checked.lock().unwrap().len(), 1);
        assert!(inbox.checked.lock().unwrap().is_empty());
        assert!(inbox.committed.lock().unwrap().is_empty());

        // A real Core rejection still prevents publication even though
        // legacy Application postcommit execution remains separate.
        let (rejected_gate, _builder, validator, inbox) = gate(
            RuntimeCheckOutcome::Rejected {
                kind: Some(CoreErrorKind::ConfigCheckFailed),
                message: "invalid application candidate".into(),
            },
            false,
        )
        .await;
        assert!(matches!(
            rejected_gate.validate_only(candidate()).await,
            Err(error) if matches!(error.as_ref(), RuntimeError::CoreRejectedConfig { .. })
        ));
        assert_eq!(validator.checked.lock().unwrap().len(), 1);
        assert!(inbox.checked.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn checked_gate_preserves_the_actual_source_domain_for_each_candidate() {
        let (gate, _builder, _validator, inbox) = gate(RuntimeCheckOutcome::Passed, false).await;
        gate.check(RuntimeCandidate {
            change: DomainChange::Profiles {
                candidate: Arc::new(Profiles::default()),
            },
            staged: BTreeMap::new(),
        })
        .await
        .unwrap();
        gate.check(RuntimeCandidate {
            change: DomainChange::Clash {
                candidate: Arc::new(ClashConfig::default()),
            },
            staged: BTreeMap::new(),
        })
        .await
        .unwrap();
        assert_eq!(
            inbox.domains.lock().unwrap().as_slice(),
            &[ConfigDomain::Profiles, ConfigDomain::Clash]
        );
        assert_eq!(inbox.checked.lock().unwrap().len(), 2);
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
