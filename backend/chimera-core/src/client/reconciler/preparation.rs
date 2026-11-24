use std::sync::Arc;

use anyhow::{Context, Result};
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use chimera_core_manager::{CoreSpec, LocalIpcPolicy, LocalIpcSettings};

use super::{
    super::{ports::SessionPortResolver, runtime},
    ports::RuntimeBuildPort,
};
use crate::actor_v2::intent::RuntimeIntentBuilder;
use crate::client::core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort};
use crate::client::core_spec::CoreSpecError;
use chimera_contracts::publish::PublishRuntimeError;

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: build capture must not write the source
/// domains it reads.
pub struct RuntimePreparation {
    profile_content: crate::state::mutation::ProfileContentReader,
    builder: Arc<dyn RuntimeBuildPort>,
    ports: Arc<SessionPortResolver>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    /// Require one live Core-owned session at construction: there is no
    /// fall-back per-builder revision allocator or disconnected port cache.
    pub fn new(
        profile_content: crate::state::mutation::ProfileContentReader,
        builder: Arc<dyn RuntimeBuildPort>,
        session: crate::client::ports::RuntimePreparationSession,
    ) -> Self {
        Self {
            profile_content,
            builder,
            ports: session.ports,
            revisions: session.revisions,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn prepare(
        &self,
        profiles: Arc<Profiles>,
        clash: ClashConfig,
        app: ChimeraAppConfig,
    ) -> Result<PreparedRuntime> {
        let inputs = self.capture_inputs(app, clash, profiles).await;
        self.prepare_inputs(inputs).await
    }

    pub async fn capture_inputs(
        &self,
        app: ChimeraAppConfig,
        clash: ClashConfig,
        profiles: Arc<Profiles>,
    ) -> super::inputs::RuntimeInputs {
        let content = self
            .profile_content
            .candidate(self.builder.capture_content(&profiles))
            .await;
        super::inputs::RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        }
    }

    pub async fn capture_committed_inputs(
        &self,
        app: ChimeraAppConfig,
        clash: ClashConfig,
    ) -> (crate::state::Version, super::inputs::RuntimeInputs) {
        self.profile_content
            .committed(|profiles| async move {
                let content = self.builder.capture_content(&profiles.state).await;
                (
                    profiles.version,
                    super::inputs::RuntimeInputs {
                        app,
                        clash,
                        profiles: Arc::new(profiles.state.clone()),
                        content,
                    },
                )
            })
            .await
    }

    pub async fn prepare_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime> {
        self.prepare_inputs_with_policy(inputs, false).await
    }

    pub async fn prepare_candidate_inputs(
        &self,
        inputs: super::inputs::RuntimeInputs,
    ) -> Result<PreparedRuntime> {
        self.prepare_inputs_with_policy(inputs, true).await
    }

    async fn prepare_inputs_with_policy(
        &self,
        inputs: super::inputs::RuntimeInputs,
        strict_transforms: bool,
    ) -> Result<PreparedRuntime> {
        let revision = self
            .revisions
            .allocate()
            .context("allocate runtime revision")?;
        let local_ipc = runtime_local_ipc(&inputs.clash);
        // A candidate resolution, not an active one: nothing here touches the
        // confirmed binding, so a build that is never applied leaves the
        // running instance's ports alone (v2 §6.2).
        let ports = self
            .ports
            .resolve_candidate(&inputs.clash)
            .context("resolve candidate listener ports")?;
        let target = inputs.target_key();
        let snapshot = self
            .builder
            .build(
                revision,
                inputs,
                ports.bindings().clone(),
                strict_transforms,
            )
            .await
            .context("build runtime candidate")?;
        prepared_runtime_from_snapshot(snapshot, ports, target, local_ipc)
    }
}

/// Core command options derive from the same typed Clash input on every path.
pub fn runtime_local_ipc(clash: &ClashConfig) -> LocalIpcSettings {
    LocalIpcSettings {
        policy: match clash.clash_control_channel {
            chimera_config::clash::config::ClashControlChannel::PreferIpc => LocalIpcPolicy::Prefer,
            chimera_config::clash::config::ClashControlChannel::HttpOnly => LocalIpcPolicy::Disable,
        },
        keep_http_controller: !clash.clash_ipc_disable_http_controller,
    }
}

/// One exact-byte intent constructor for all prepared runtime admissions.
pub fn prepared_runtime_from_snapshot(
    snapshot: Arc<runtime::RuntimeSnapshot>,
    ports: crate::client::ports::CandidatePortBindings,
    target: Option<String>,
    local_ipc: LocalIpcSettings,
) -> Result<PreparedRuntime> {
    let text = std::str::from_utf8(snapshot.product_bytes())
        .context("the built runtime document is not UTF-8")?;
    let intent = RuntimeIntentBuilder::build(
        (&snapshot.target_core).into(),
        Arc::<str>::from(text),
        local_ipc,
    );
    Ok(PreparedRuntime {
        snapshot,
        intent: Arc::new(intent),
        ports,
        target,
    })
}

#[async_trait::async_trait]
impl RuntimePreparationPort for RuntimePreparation {
    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), PublishRuntimeError> {
        self.builder.publish(snapshot).await
    }

    fn core_spec(
        &self,
        core: &chimera_config::application::ClashCore,
    ) -> Result<CoreSpec, CoreSpecError> {
        self.builder.core_spec(core)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::state::PersistentStateManagerSetup;
    use crate::{
        client::{
            reconciler::inputs::{FrozenProfileContent, RuntimeInputs},
            runtime::{RuntimeLifecycle, RuntimeSnapshot},
        },
        state::mutation::ProfileContentLock,
    };
    use chimera_config::{application::ClashCore, clash::config::ClashControlChannel};

    #[derive(Default)]
    struct RecordingBuildPort {
        strict: Mutex<Vec<bool>>,
    }

    #[async_trait::async_trait]
    impl RuntimeBuildPort for RecordingBuildPort {
        async fn capture_content(&self, _: &Profiles) -> FrozenProfileContent {
            FrozenProfileContent::default()
        }

        fn core_spec(&self, core: &ClashCore) -> std::result::Result<CoreSpec, CoreSpecError> {
            crate::client::core_spec::resolve_with(core, |_, _| {
                Ok(std::path::PathBuf::from("/tmp/chimera-fixture-core"))
            })
        }

        async fn build(
            &self,
            revision: runtime::RuntimeRevision,
            inputs: RuntimeInputs,
            _: chimera_config::runtime::executor::ResolvedPortBindings,
            strict_transforms: bool,
        ) -> std::result::Result<
            Arc<RuntimeSnapshot>,
            crate::client::runtime_build::RuntimeBuildError,
        > {
            self.strict.lock().unwrap().push(strict_transforms);
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
        ) -> std::result::Result<(), PublishRuntimeError> {
            Ok(())
        }
    }

    async fn preparation() -> (RuntimePreparation, Arc<RecordingBuildPort>) {
        let path = camino::Utf8PathBuf::from_path_buf(
            tempfile::tempdir().unwrap().path().join("profiles.yaml"),
        )
        .unwrap();
        let manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(path)
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let reader = crate::state::mutation::ProfileContentReader::new(
            manager.snapshot_handle(),
            ProfileContentLock::default(),
        );
        let builder = Arc::new(RecordingBuildPort::default());
        let lifecycle = Arc::new(RuntimeLifecycle::default());
        let session = crate::client::ports::RuntimePreparationSession {
            revisions: lifecycle.revision_allocator(),
            ports: Arc::new(SessionPortResolver::new(lifecycle)),
        };
        (
            RuntimePreparation::new(reader, builder.clone(), session),
            builder,
        )
    }

    #[tokio::test]
    async fn reference_preparation_freezes_committed_inputs_and_reuses_exact_bytes() {
        let (preparation, builder) = preparation().await;
        let app = ChimeraAppConfig {
            core: ClashCore::Mihomo,
            ..Default::default()
        };
        let clash = ClashConfig::default();
        let (version, inputs) = preparation.capture_committed_inputs(app, clash).await;
        assert_eq!(version, crate::state::Version::new(0));
        let expected_target = inputs.target_key();
        let prepared = preparation.prepare_inputs(inputs).await.unwrap();
        assert_eq!(prepared.target, expected_target);
        assert_eq!(prepared.snapshot.product_bytes(), b"mode: rule\n");
        assert_eq!(prepared.intent.config_text.as_ref(), "mode: rule\n");
        assert_eq!(
            prepared.intent.digest,
            chimera_core_manager::payload_digest(prepared.snapshot.product_bytes())
        );
        assert_eq!(builder.strict.lock().unwrap().as_slice(), &[false]);
    }

    #[tokio::test]
    async fn candidate_preparation_is_strict_and_does_not_confirm_ports() {
        let (preparation, builder) = preparation().await;
        let app = ChimeraAppConfig {
            core: ClashCore::ChimeraClient,
            ..Default::default()
        };
        let mut clash = ClashConfig::default();
        clash.clash_control_channel = ClashControlChannel::HttpOnly;
        let inputs = preparation
            .capture_inputs(app, clash, Arc::new(Profiles::default()))
            .await;
        let prepared = preparation.prepare_candidate_inputs(inputs).await.unwrap();
        assert_eq!(prepared.snapshot.target_core, ClashCore::ChimeraClient);
        assert_eq!(prepared.intent.local_ipc.policy, LocalIpcPolicy::Disable);
        assert_eq!(builder.strict.lock().unwrap().as_slice(), &[true]);
        assert!(prepared.target.is_some());
    }
}
