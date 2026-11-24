//! Application orchestration for content capture, blocking builds, and runtime publication.

use std::{collections::BTreeMap, sync::Arc};

use crate::client::core_spec::CoreSpecError;
use crate::client::reconciler::ports::ScriptRunnerFactory;
use async_trait::async_trait;
use chimera_config::runtime::executor::{
    BuiltinTransform, PortError, ProfileContentSource, RuntimeArtifact, ScriptRunner,
};
use chimera_contracts::publish::PublishRuntimeError;
pub use chimera_profile_transform::{
    RuntimeBuildError, RuntimeBuildInput, RuntimeBuildLog, RuntimeBuilder, builtin_transforms_for,
    derive_tun_flavor,
};
#[cfg(test)]
mod build_tests;

use crate::client::{
    artifact_snapshot::{
        ArtifactSnapshotData, artifact_to_snapshot_data, runtime_snapshot_data_from_artifact,
    },
    reconciler::{inputs, ports::RuntimeBuildPort},
    runtime,
};

/// Resolves the process specification for the selected core using shell-provided paths.
pub type CoreSpecResolver = crate::client::core_spec::CoreSpecResolver;

/// Builds application runtime snapshots using injected content and script execution ports.
pub struct RuntimeBuildService {
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    core_specs: Arc<CoreSpecResolver>,
    paths: runtime::RuntimePaths,
    scripts: Arc<ScriptRunnerFactory>,
}

fn selected_builtins(input: &RuntimeBuildInput) -> Vec<BuiltinTransform> {
    if input.app.enable_builtin_enhanced {
        builtin_transforms_for(input.app.core)
    } else {
        Vec::new()
    }
}

fn build_artifact(
    input: &RuntimeBuildInput,
    content: &dyn ProfileContentSource,
    scripts: &dyn ScriptRunner,
    builtins: &[BuiltinTransform],
    strict_transforms: bool,
) -> Result<RuntimeArtifact, RuntimeBuildError> {
    let artifact = RuntimeBuilder::build_with_builtins(input, builtins, content, scripts)?;
    if strict_transforms {
        RuntimeBuilder::validate_transforms(&artifact)?;
    }
    Ok(artifact)
}

impl RuntimeBuildService {
    /// Injects content capture, publication paths, core resolution and a blocking runner factory.
    pub fn new(
        content: Arc<dyn ProfileContentSource + Send + Sync>,
        core_specs: Arc<CoreSpecResolver>,
        paths: runtime::RuntimePaths,
        scripts: Arc<ScriptRunnerFactory>,
    ) -> Self {
        Self {
            content,
            core_specs,
            paths,
            scripts,
        }
    }

    /// Build from a typed source before a runtime revision is assigned. The
    /// source is frozen on the blocking worker before scripts are constructed;
    /// this path has no product publication or runtime ownership.
    pub async fn build_from_source(
        input: RuntimeBuildInput,
        source: Arc<dyn ProfileContentSource + Send + Sync>,
        staged_content: BTreeMap<String, String>,
        scripts: Arc<ScriptRunnerFactory>,
        strict_transforms: bool,
    ) -> Result<ArtifactSnapshotData, RuntimeBuildError> {
        let builtins = selected_builtins(&input);
        let built = tokio::task::spawn_blocking(move || {
            let content = inputs::FrozenProfileContent::capture_content(
                &input.profiles,
                source.as_ref(),
                staged_content,
            );
            Self::build_candidate_data(input, &content, || scripts(), &builtins, strict_transforms)
        });
        chimera_owner_support::blocking::join(built.await)
    }

    fn build_candidate_data(
        input: RuntimeBuildInput,
        content: &dyn ProfileContentSource,
        make_scripts: impl FnOnce() -> Result<Box<dyn ScriptRunner>, PortError>,
        builtins: &[BuiltinTransform],
        strict_transforms: bool,
    ) -> Result<ArtifactSnapshotData, RuntimeBuildError> {
        let scripts = make_scripts().map_err(RuntimeBuildError::StartScriptRunner)?;
        let artifact = build_artifact(
            &input,
            content,
            scripts.as_ref(),
            builtins,
            strict_transforms,
        )?;
        let names = builtins
            .iter()
            .map(|builtin| builtin.name.clone())
            .collect::<Vec<_>>();
        artifact_to_snapshot_data(artifact, &input.profiles, &names)
            .map_err(RuntimeBuildError::ProjectArtifact)
    }
}

#[async_trait]
impl RuntimeBuildPort for RuntimeBuildService {
    async fn capture_content(
        &self,
        profiles: &chimera_config::profile::Profiles,
    ) -> inputs::FrozenProfileContent {
        inputs::FrozenProfileContent::capture_content(
            profiles,
            self.content.as_ref(),
            BTreeMap::new(),
        )
    }

    fn core_spec(
        &self,
        core: &chimera_config::application::ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, CoreSpecError> {
        (self.core_specs)(core)
    }

    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: inputs::RuntimeInputs,
        resolved_ports: chimera_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildError> {
        let script_runner = self.scripts.clone();
        let built = tokio::task::spawn_blocking(move || {
            let inputs::RuntimeInputs {
                app,
                clash,
                profiles,
                content,
            } = inputs;
            let core = app.core;
            let builtin_enabled = app.enable_builtin_enhanced;
            let scripts = script_runner().map_err(RuntimeBuildError::StartScriptRunner)?;
            let input = RuntimeBuildInput {
                profiles: profiles.clone(),
                clash,
                app,
                resolved_ports,
            };
            let builtins = selected_builtins(&input);
            let artifact = build_artifact(
                &input,
                &content,
                scripts.as_ref(),
                &builtins,
                strict_transforms,
            )?;
            let (bytes, data) =
                runtime_snapshot_data_from_artifact(artifact, &profiles, core, builtin_enabled)?;
            Ok(Arc::new(runtime::RuntimeSnapshot::from_data(
                revision, core, bytes, data,
            )))
        });
        chimera_owner_support::blocking::join(built.await)
    }

    async fn publish(
        &self,
        snapshot: &runtime::RuntimeSnapshot,
    ) -> Result<(), PublishRuntimeError> {
        runtime::write_product(
            self.paths.product().as_std_path(),
            snapshot.product_bytes().to_vec(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::state::PersistentStateManagerSetup;
    use crate::{
        client::{
            core_lifecycle::ports::RuntimePreparationPort, ports::SessionPortResolver,
            reconciler::preparation::RuntimePreparation, runtime::RuntimeLifecycle,
        },
        state::mutation::{ProfileContentLock, ProfileContentReader},
    };
    use chimera_config::{
        application::{ChimeraAppConfig, ClashCore},
        clash::config::ClashConfig,
        profile::{ManagedProfilePath, Profiles, ScriptRuntime},
        runtime::{
            executor::{PortError, ScriptRunner, StepLogEntry},
            value::ConfigValue,
        },
    };

    struct EmptyContent;
    impl ProfileContentSource for EmptyContent {
        fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
            Err("no managed files expected in the bare profile fixture".into())
        }
    }

    struct EchoRunner;
    impl ScriptRunner for EchoRunner {
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
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    #[tokio::test]
    async fn real_builder_and_ref_preparation_share_bytes_without_precommit_publication() {
        let temp = tempfile::tempdir().unwrap();
        let paths = runtime::RuntimePaths::from_config_root(temp.path().to_path_buf());
        let output = paths.product().to_owned();
        let builder = Arc::new(RuntimeBuildService::new(
            Arc::new(EmptyContent),
            Arc::new(|core| {
                crate::client::core_spec::resolve_with(core, |_, _| {
                    Ok(std::path::PathBuf::from("/tmp/chimera-test-core"))
                })
            }),
            paths,
            Arc::new(|| Ok(Box::new(EchoRunner))),
        ));
        let manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(
                camino::Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap(),
            )
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let reader =
            ProfileContentReader::new(manager.snapshot_handle(), ProfileContentLock::default());
        let lifecycle = Arc::new(RuntimeLifecycle::default());
        let session = crate::client::ports::RuntimePreparationSession {
            revisions: lifecycle.revision_allocator(),
            ports: Arc::new(SessionPortResolver::new(lifecycle)),
        };
        let preparation = RuntimePreparation::new(reader, builder, session);
        let app = ChimeraAppConfig {
            core: ClashCore::Mihomo,
            enable_builtin_enhanced: false,
            ..Default::default()
        };
        let (_, inputs) = preparation
            .capture_committed_inputs(app, ClashConfig::default())
            .await;
        let prepared = preparation.prepare_candidate_inputs(inputs).await.unwrap();
        assert_eq!(
            prepared.intent.config_text.as_bytes(),
            prepared.snapshot.product_bytes(),
            "the check must use the exact same bytes the builder would publish",
        );
        assert_eq!(
            prepared.intent.digest,
            chimera_core_manager::payload_digest(prepared.snapshot.product_bytes()),
        );
        assert!(!output.exists(), "preflight must not write runtime product");
        preparation.publish(&prepared.snapshot).await.unwrap();
        assert_eq!(
            tokio::fs::read(&output).await.unwrap(),
            prepared.snapshot.product_bytes()
        );
    }

    #[tokio::test]
    async fn builder_capture_and_core_spec_use_injected_ports() {
        let temp = tempfile::tempdir().unwrap();
        let builder = RuntimeBuildService::new(
            Arc::new(EmptyContent),
            Arc::new(|core| {
                crate::client::core_spec::resolve_with(core, |_, _| {
                    Ok(std::path::PathBuf::from("/tmp/chimera-test-core"))
                })
            }),
            runtime::RuntimePaths::from_config_root(temp.path().to_path_buf()),
            Arc::new(|| Ok(Box::new(EchoRunner))),
        );
        assert!(
            builder
                .capture_content(&Profiles::default())
                .await
                .0
                .is_empty()
        );
        let spec = builder.core_spec(&ClashCore::ChimeraClientAlpha).unwrap();
        assert_eq!(spec.kind, chimera_core_manager::CoreKind::ChimeraClient);
    }
}
