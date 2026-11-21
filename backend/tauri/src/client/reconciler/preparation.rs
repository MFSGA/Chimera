use std::sync::Arc;

use anyhow::{Context, Result};
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
use chimera_core_manager::{CoreSpec, LocalIpcPolicy, LocalIpcSettings};

use super::{
    super::{SessionPortResolver, runtime},
    ports::RuntimeBuildPort,
};
use crate::client::core_lifecycle::ports::{PreparedRuntime, RuntimePreparationPort};
use chimera_contracts::publish::PublishRuntimeError;
use chimera_core::actor_v2::intent::RuntimeIntentBuilder;
use chimera_core::client::core_spec::CoreSpecError;

/// Builds runtime candidates from committed source config. It holds read-only
/// state handles, never a domain client: build capture must not write the source
/// domains it reads.
pub(in crate::client) struct RuntimePreparation {
    profile_content: crate::state::mutation::ProfileContentReader,
    builder: Arc<dyn RuntimeBuildPort>,
    ports: Arc<SessionPortResolver>,
    revisions: runtime::RuntimeRevisionAllocator,
}

impl RuntimePreparation {
    pub fn new(
        profile_content: crate::state::mutation::ProfileContentReader,
        builder: Arc<dyn RuntimeBuildPort>,
        ports: Arc<SessionPortResolver>,
    ) -> Self {
        Self {
            profile_content,
            builder,
            ports,
            revisions: runtime::RuntimeRevisionAllocator::new(),
        }
    }

    pub(super) fn with_revisions(mut self, revisions: runtime::RuntimeRevisionAllocator) -> Self {
        self.revisions = revisions;
        self
    }

    #[cfg(test)]
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
    ) -> (chimera_core::state::Version, super::inputs::RuntimeInputs) {
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
        let local_ipc = LocalIpcSettings {
            policy: match inputs.clash.clash_control_channel {
                chimera_config::clash::config::ClashControlChannel::PreferIpc => {
                    LocalIpcPolicy::Prefer
                }
                chimera_config::clash::config::ClashControlChannel::HttpOnly => {
                    LocalIpcPolicy::Disable
                }
            },
            keep_http_controller: !inputs.clash.clash_ipc_disable_http_controller,
        };
        // A candidate resolution, not an active one: nothing here touches the
        // confirmed binding, so a build that is never applied leaves the
        // running instance's ports alone (v2 §6.2).
        let ports = self
            .ports
            .resolve_candidate(&inputs.clash)
            .context("resolve candidate listener ports")?;
        let core_type: chimera_utils::core::CoreType = (&inputs.app.core).into();
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
        // Serialized once, here: the check and the reconcile both consume this
        // value, so "same bytes" holds by construction rather than by
        // convention.
        // Chimera's RuntimeSnapshot currently stores the exact product bytes,
        // rather than ref's separate `config_text` member. Reuse those bytes
        // without serializing again, so checks and later submission agree.
        let text = std::str::from_utf8(snapshot.product_bytes())
            .context("the built runtime document is not UTF-8")?;
        let intent = RuntimeIntentBuilder::build(core_type, Arc::<str>::from(text), local_ipc);
        Ok(PreparedRuntime {
            snapshot,
            intent: Arc::new(intent),
            ports,
            target,
        })
    }
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

    use crate::{
        client::{
            reconciler::inputs::{FrozenProfileContent, RuntimeInputs},
            runtime::{RuntimeLifecycle, RuntimeSnapshot},
        },
        state::mutation::ProfileContentLock,
    };
    use chimera_config::{application::ClashCore, clash::config::ClashControlChannel};
    use chimera_core::state::PersistentStateManagerSetup;

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
            chimera_core::client::core_spec::resolve_with(core, |_, _| {
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
            chimera_core::client::runtime_build::RuntimeBuildError,
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
        let ports = Arc::new(SessionPortResolver::new(Arc::new(
            RuntimeLifecycle::default(),
        )));
        (
            RuntimePreparation::new(reader, builder.clone(), ports),
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
        assert_eq!(version, chimera_core::state::Version::new(0));
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
