//! Compose the pinned ref CandidateCheck from the existing source owners and
//! Core-owned runtime session. Construction itself is read-only.
//!
//! Profiles production installs this gate through MutationCoordinator and
//! sends checked commits to the single CoreLifecycle mailbox. Application and
//! Clash mutations still use their separate legacy runtime paths.

use std::sync::Arc;

use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig,
    runtime::executor::ProfileContentSource,
};
use chimera_state_engine::StateSnapshot;

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

/// Bridges ref's Checked/SourceCommitted notifications into the *existing*
/// CoreLifecycle Actor. Checked admissions are acknowledged and committed
/// notices use a dedicated mailbox message; they do not call the legacy
/// SourceCommitted rebuild handler.
pub(in crate::client) struct CoreLifecycleCheckedInbox {
    owner: crate::client::core_lifecycle::CoreLifecycleClient,
}

impl CoreLifecycleCheckedInbox {
    pub(in crate::client) fn new(
        owner: crate::client::core_lifecycle::CoreLifecycleClient,
    ) -> Self {
        Self { owner }
    }

    fn source_domain(
        domain: chimera_contracts::mutation::ConfigDomain,
    ) -> crate::client::reconciler::committed::SourceDomain {
        use crate::client::reconciler::committed::SourceDomain;
        match domain {
            chimera_contracts::mutation::ConfigDomain::Application => SourceDomain::Application,
            chimera_contracts::mutation::ConfigDomain::Clash => SourceDomain::Clash,
            chimera_contracts::mutation::ConfigDomain::Profiles => SourceDomain::Profiles,
        }
    }
}

#[async_trait::async_trait]
impl CheckedCandidatePort for CoreLifecycleCheckedInbox {
    async fn checked(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        prepared: crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        let domain = Self::source_domain(domain);
        self.owner
            .checked_candidate_for_domain(domain, prepared, core_spec)
            .await
    }

    fn source_committed(
        &self,
        domain: chimera_contracts::mutation::ConfigDomain,
        version: chimera_core::state::Version,
    ) {
        self.owner
            .checked_source_committed(Self::source_domain(domain), *version);
    }

    fn abort_uncommitted(&self, domain: chimera_contracts::mutation::ConfigDomain) {
        self.owner
            .abort_uncommitted_checked(Self::source_domain(domain));
    }
}

pub(in crate::client) struct CandidateGateSources {
    pub application: StateSnapshot<ChimeraAppConfig>,
    pub clash: StateSnapshot<ClashConfig>,
    pub profile_content: ProfileContentReader,
    pub content: Arc<dyn ProfileContentSource + Send + Sync>,
    pub session: RuntimePreparationSession,
    pub paths: RuntimePaths,
}

/// Assemble the Profiles production gate from the live source owners and the
/// canonical Core host. Only checked-source notifications consume its products;
/// no legacy SourceCommitted rebuild is sent for the same Profiles commit.
/// No fallback session or speculative source snapshot is ever constructed.
pub(in crate::client) fn prepare_production_gate(
    application: &crate::client::application::ApplicationClient,
    clash: &crate::client::clash_config::ClashConfigClient,
    profiles: ProfileContentReader,
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    paths: RuntimePaths,
    core: &chimera_core::actor_v2::CoreClient,
    owner: &crate::client::core_lifecycle::CoreLifecycleClient,
) -> anyhow::Result<CandidateCheck> {
    let session = owner.runtime_preparation_session().ok_or_else(|| {
        anyhow::anyhow!("ref CandidateCheck requires the active Core-owned runtime session")
    })?;
    let sources = CandidateGateSources {
        application: application.committed_snapshot_handle()?,
        clash: clash.committed_snapshot_handle()?,
        profile_content: profiles,
        content,
        session,
        paths,
    };
    Ok(production_candidate_gate(
        sources,
        core,
        Arc::new(CoreLifecycleCheckedInbox::new(owner.clone())),
    ))
}

/// The actual production builder/validator, with a checked inbox supplied by
/// the single Runtime owner. No independent port resolver, revision allocator,
/// filesystem publication, or Core mutation actor is created here.
pub(in crate::client) fn production_candidate_gate(
    sources: CandidateGateSources,
    core: &chimera_core::actor_v2::CoreClient,
    workflow: Arc<dyn CheckedCandidatePort>,
) -> CandidateCheck {
    let core_specs: Arc<CoreSpecResolver> = Arc::new(|core| {
        chimera_core::client::core_spec::resolve_with(core, |choice, core_type| {
            if *choice == chimera_config::application::ClashCore::ChimeraClientAlpha {
                let name = format!("chimera-client-alpha{}", std::env::consts::EXE_SUFFIX);
                crate::core::clash::core::find_binary_path_by_name(&name)
            } else {
                crate::core::clash::core::find_binary_path(core_type)
            }
        })
    });
    let scripts: Arc<ScriptRunnerFactory> =
        Arc::new(|| Ok(Box::new(crate::enhance::EnhanceScriptRunner::new()?)));
    let validator = Arc::new(CoreCheckValidator::new(core));
    candidate_gate_with_ports(sources, core_specs, scripts, validator, workflow)
}

/// Test-injectable verification seam. Only the checker is substitutable;
/// the real RuntimeBuildService and canonical Core runtime session remain.
fn candidate_gate_with_ports(
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
            executor::{PortError, ScriptRunOutcome, ScriptRunner},
            value::ConfigValue,
        },
    };
    use chimera_contracts::{mutation::ConfigDomain, runtime_error::RuntimeError};
    use chimera_core::state::{PersistentStateManagerSetup as ProfilesManagerSetup, Version};
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
        fn run(&self, _: ScriptRuntime, _: &str, config: &ConfigValue) -> ScriptRunOutcome {
            ScriptRunOutcome {
                result: Ok(config.clone()),
                logs: vec![],
            }
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
        fn abort_uncommitted(&self, _domain: ConfigDomain) {}
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
                chimera_core::client::core_spec::resolve_with(core, |_, _| {
                    Ok("/bin/test-mihomo".into())
                })
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
