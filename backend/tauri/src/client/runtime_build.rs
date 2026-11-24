//! Application orchestration for content capture, blocking builds, and runtime publication.

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chimera_config::runtime::executor::BuiltinTransform;
use chimera_config::runtime::executor::ProfileContentSource;
use chimera_contracts::publish::PublishRuntimeError;
use chimera_core::client::core_spec::CoreSpecError;
use chimera_core::client::runtime_build::{RuntimeBuildError, RuntimeBuildInput, RuntimeBuilder};

use crate::client::{
    reconciler::{
        inputs,
        ports::{RuntimeBuildPort, ScriptRunnerFactory},
    },
    runtime,
};

/// Resolves the process specification for the selected core using shell-provided paths.
pub type CoreSpecResolver = chimera_core::client::core_spec::CoreSpecResolver;

/// Builds application runtime snapshots using injected content and script execution ports.
pub struct RuntimeBuildService {
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    core_specs: Arc<CoreSpecResolver>,
    paths: runtime::RuntimePaths,
    scripts: Arc<ScriptRunnerFactory>,
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
            // The ref's script runner is built and dropped on this blocking
            // worker. Chimera adds its retained core-specific builtin table.
            let builtins: Vec<BuiltinTransform> = if builtin_enabled {
                crate::enhance::builtin_transforms_for(core)
            } else {
                Vec::new()
            };
            let artifact = RuntimeBuilder::build(&input, &content, scripts.as_ref())?;
            if strict_transforms {
                RuntimeBuilder::validate_transforms(&artifact)?;
            }
            let builtin_names = builtins
                .iter()
                .map(|item| item.name.clone())
                .collect::<Vec<_>>();
            let data = chimera_core::client::artifact_snapshot::artifact_to_snapshot_data(
                artifact,
                &profiles,
                &builtin_names,
            )
            .map_err(RuntimeBuildError::ProjectArtifact)?;
            // Chimera's snapshot still stores the product bytes and Mapping;
            // ref's snapshot stores config_text. Serialize only at this source
            // build boundary, and reuse those exact bytes for the intent.
            let bytes = serde_yaml::to_string(&data.config)
                .map_err(RuntimeBuildError::SerializeFinalConfig)?
                .into_bytes();
            Ok(Arc::new(
                runtime::RuntimeSnapshot::new_with_transform_output_and_inspection_and_exists_keys(
                    revision,
                    core,
                    bytes,
                    data.config,
                    data.postprocessing_output,
                    data.inspection,
                    data.exists_keys,
                ),
            ))
        });
        built.await.map_err(RuntimeBuildError::BuildWorker)?
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
    use chimera_core::state::PersistentStateManagerSetup;

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
                chimera_core::client::core_spec::resolve_with(core, |_, _| {
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
                chimera_core::client::core_spec::resolve_with(core, |_, _| {
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
