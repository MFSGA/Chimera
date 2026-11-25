//! Construct the ref CandidateCheck using the live Core-owned session.
use super::{
    adapters::CoreCheckValidator,
    check::{self, CandidateCheck, CheckedCandidatePort},
    ports::{RuntimeValidatorPort, ScriptRunnerFactory},
};
use crate::{
    client::{
        ports::RuntimePreparationSession,
        runtime::RuntimePaths,
        runtime_build::{CoreSpecResolver, RuntimeBuildService},
    },
    state::mutation::ProfileContentReader,
};
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig,
    runtime::executor::ProfileContentSource,
};
use chimera_state_engine::StateSnapshot;
use std::sync::Arc;

pub struct CandidateGateSources {
    pub application: StateSnapshot<ChimeraAppConfig>,
    pub clash: StateSnapshot<ClashConfig>,
    pub profile_content: ProfileContentReader,
    pub content: Arc<dyn ProfileContentSource + Send + Sync>,
    pub session: RuntimePreparationSession,
    pub paths: RuntimePaths,
}

pub fn production_candidate_gate(
    sources: CandidateGateSources,
    core: &crate::actor_v2::CoreClient,
    core_specs: Arc<CoreSpecResolver>,
    scripts: Arc<ScriptRunnerFactory>,
    workflow: Arc<dyn CheckedCandidatePort>,
) -> CandidateCheck {
    candidate_gate_with_ports(
        sources,
        core_specs,
        scripts,
        Arc::new(CoreCheckValidator::new(core)),
        workflow,
    )
}

/// Test-injectable verification seam. Only the checker is substitutable;
/// the real RuntimeBuildService and canonical Core runtime session remain.
pub fn candidate_gate_with_ports(
    sources: CandidateGateSources,
    core_specs: Arc<CoreSpecResolver>,
    scripts: Arc<ScriptRunnerFactory>,
    validator: Arc<dyn RuntimeValidatorPort>,
    workflow: Arc<dyn CheckedCandidatePort>,
) -> CandidateCheck {
    let CandidateGateSources {
        application,
        clash,
        profile_content,
        content,
        session,
        paths,
    } = sources;
    let builder = Arc::new(RuntimeBuildService::new(
        content, core_specs, paths, scripts,
    ));
    CandidateCheck::new(check::Args {
        application,
        clash,
        profile_content,
        builder,
        session,
        validator,
        workflow,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{PersistentStateManagerSetup as ProfilesManagerSetup, Version};
    use crate::{
        client::{
            ports::SessionPortResolver,
            reconciler::ports::{RuntimeCheckOutcome, RuntimeCheckRequest},
            runtime::RuntimeLifecycle,
        },
        state::mutation::{DomainChange, ProfileContentLock, RuntimeCandidate, RuntimeGate},
    };
    use camino::Utf8PathBuf;
    use chimera_config::{
        application::ClashCore,
        profile::{ManagedProfilePath, Profiles, ScriptRuntime},
        runtime::{
            executor::{PortError, ScriptRunner, StepLogEntry},
            value::ConfigValue,
        },
    };
    use chimera_contracts::{mutation::ConfigDomain, runtime_error::RuntimeError};
    use chimera_state_engine::PersistentStateManagerSetup;
    use std::{collections::BTreeMap, sync::Mutex};

    struct MissingManagedFiles;
    impl ProfileContentSource for MissingManagedFiles {
        fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
            Err("not needed for empty profile".into())
        }
    }
    struct EchoScript;
    impl ScriptRunner for EchoScript {
        fn run(
            &self,
            _: ScriptRuntime,
            _: &str,
            config: &ConfigValue,
            _: &mut Vec<StepLogEntry>,
        ) -> Result<ConfigValue, PortError> {
            Ok(config.clone())
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, config: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(config.clone())
        }
    }
    #[derive(Default)]
    struct RecordingInbox {
        checked: Mutex<Vec<(u64, String, Vec<u8>, String)>>,
        source_notices: Mutex<Vec<(ConfigDomain, Version)>>,
    }
    #[async_trait::async_trait]
    impl CheckedCandidatePort for RecordingInbox {
        async fn checked(
            &self,
            domain: ConfigDomain,
            candidate: crate::client::core_lifecycle::ports::PreparedRuntime,
            core_spec: chimera_core_manager::CoreSpec,
        ) -> Result<(), Arc<RuntimeError>> {
            assert_eq!(domain, ConfigDomain::Application);
            assert_eq!(core_spec.kind, chimera_core_manager::CoreKind::Mihomo);

            self.checked.lock().unwrap().push((
                candidate.snapshot.revision.get(),
                candidate.intent.digest.clone(),
                candidate.snapshot.product_bytes().to_vec(),
                candidate.target.unwrap(),
            ));
            Ok(())
        }
        fn source_committed(&self, domain: ConfigDomain, version: Version) {
            self.source_notices.lock().unwrap().push((domain, version));
        }
        fn source_requires_reconcile(&self, domain: ConfigDomain, version: Version) {
            self.source_notices.lock().unwrap().push((domain, version));
        }
        fn abort_uncommitted(&self, _domain: ConfigDomain) {}
        async fn checked_for_request(
            &self,
            domain: ConfigDomain,
            candidate: crate::client::core_lifecycle::ports::PreparedRuntime,
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
    #[derive(Default)]
    struct RecordingValidator(Mutex<Vec<(String, Vec<u8>)>>);
    #[async_trait::async_trait]
    impl RuntimeValidatorPort for RecordingValidator {
        async fn check(&self, request: RuntimeCheckRequest<'_>) -> RuntimeCheckOutcome {
            self.0.lock().unwrap().push((
                request.intent.digest.clone(),
                request.intent.config_text.as_bytes().to_vec(),
            ));
            RuntimeCheckOutcome::Passed
        }
    }

    #[tokio::test]
    async fn actual_builder_and_shared_owner_session_preserve_checked_bytes_without_publishing() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| Utf8PathBuf::from_path_buf(dir.path().join(name)).unwrap();
        let app = ChimeraAppConfig {
            core: ClashCore::Mihomo,
            enable_builtin_enhanced: false,
            ..Default::default()
        };
        let app_manager = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(path("app.yaml"))
            .assemble()
            .from_state(app.clone());
        let clash_manager = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path("clash.yaml"))
            .assemble()
            .from_state(ClashConfig::default());
        let profiles_manager = ProfilesManagerSetup::<Profiles>::builder()
            .config_path(path("profiles.yaml"))
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let product_paths = RuntimePaths::from_config_root(dir.path().to_path_buf());
        let lifecycle = Arc::new(RuntimeLifecycle::default());
        let next_revision = lifecycle.revision_allocator();
        let sources = CandidateGateSources {
            application: app_manager.snapshot_handle(),
            clash: clash_manager.snapshot_handle(),
            profile_content: ProfileContentReader::new(
                profiles_manager.snapshot_handle(),
                ProfileContentLock::default(),
            ),
            content: Arc::new(MissingManagedFiles),
            session: RuntimePreparationSession {
                revisions: next_revision.clone(),
                ports: Arc::new(SessionPortResolver::new(lifecycle.clone())),
            },
            paths: product_paths.clone(),
        };
        let inbox = Arc::new(RecordingInbox::default());
        let validator = Arc::new(RecordingValidator::default());
        let gate = candidate_gate_with_ports(
            sources,
            Arc::new(|core| {
                crate::client::core_spec::resolve_with(core, |_, _| Ok("/bin/test-mihomo".into()))
            }),
            Arc::new(|| Ok(Box::new(EchoScript))),
            validator.clone(),
            inbox.clone(),
        );
        gate.check(RuntimeCandidate {
            change: DomainChange::Application {
                candidate: Arc::new(app),
            },
            staged: BTreeMap::new(),
        })
        .await
        .unwrap();
        let sent = inbox.checked.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].0 > 0);
        assert_eq!(sent[0].1, chimera_core_manager::payload_digest(&sent[0].2));
        assert_eq!(
            validator.0.lock().unwrap().as_slice(),
            &[(sent[0].1.clone(), sent[0].2.clone())]
        );
        let next = next_revision.allocate().unwrap();
        assert_eq!(
            next.get(),
            sent[0].0 + 1,
            "the actual checked candidate consumed the owner's shared revision counter"
        );
        assert!(
            lifecycle.accepted_confirmed_receipt().is_none(),
            "a passed precommit check cannot confirm the running session"
        );
        assert!(
            !product_paths.product().exists(),
            "the precommit real builder must not write the runtime product"
        );
    }
}
