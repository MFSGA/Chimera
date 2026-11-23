//! Ref-style pure runtime builder: no filesystem, script runtime or core owner.
//! The Tauri shell supplies builtin scripts and executor ports; this module
//! validates a typed candidate and executes its graph deterministically.

use anyhow::Context;
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{
        ClashConfig, TransparentProxyMode, TransparentProxyValidationError, tun_stack::TunStack,
    },
    profile::Profiles,
    runtime::executor::{
        BuiltinTransform, ExecutionTarget, GuardInputs, ProfileContentSource, ResolvedPortBindings,
        RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs, ScriptRunner,
        TransformFailure, TunFlavor, TunParams, execute,
    },
    runtime::snapshot::OperatorTag,
};
use std::collections::BTreeMap;
use std::sync::Arc;

use super::{
    artifact_snapshot::{ArtifactSnapshotData, artifact_to_snapshot_data},
    runtime_build_port::FrozenProfileContent,
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeBuildError {
    // Pinned ref runtime_builder typed failures, mapped to Chimera's crate ownership.
    #[error("could not start the script runner: {0}")]
    StartScriptRunner(#[source] chimera_config::runtime::executor::PortError),
    #[error("runtime build task did not complete: {0}")]
    BuildWorker(#[source] tokio::task::JoinError),
    #[error("could not project the runtime artifact: {0}")]
    ProjectArtifact(#[source] anyhow::Error),
    #[error("could not serialize the final runtime document: {0}")]
    SerializeFinalConfig(#[source] serde_yaml::Error),
    #[error("profiles snapshot failed validation: {0:?}")]
    Validation(Vec<chimera_config::profile::ProfileValidationError>),
    #[error("transparent proxy settings are invalid: {0}")]
    ValidateTransparentProxy(#[from] TransparentProxyValidationError),
    #[error("transparent capture requires service mode")]
    TransparentProxyRequiresServiceMode,
    #[error(transparent)]
    Pipeline(#[from] RuntimePipelineError),
    #[error("runtime candidate contains failed transforms: {failures:?}")]
    TransformsFailed {
        failures: Vec<TransformFailure>,
        logs: Vec<RuntimeBuildLog>,
    },
}

#[derive(Debug, Clone)]
pub struct RuntimeBuildLog {
    pub tag: OperatorTag,
    pub entries: Vec<chimera_config::runtime::executor::StepLogEntry>,
}

pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,
    pub clash: ClashConfig,
    pub app: ChimeraAppConfig,
    pub resolved_ports: ResolvedPortBindings,
}

/// Legacy TUN derivation, including Premium+Mixed -> Gvisor.
pub fn derive_tun_flavor(
    core: chimera_config::application::ClashCore,
    stack: TunStack,
) -> TunFlavor {
    if core == chimera_config::application::ClashCore::ClashRs {
        return TunFlavor::ClashRs;
    }
    if matches!(
        core,
        chimera_config::application::ClashCore::ChimeraClient
            | chimera_config::application::ClashCore::ChimeraClientAlpha
    ) {
        return TunFlavor::ChimeraClient;
    }
    let stack = if core == chimera_config::application::ClashCore::ClashPremium
        && stack == TunStack::Mixed
    {
        TunStack::Gvisor
    } else {
        stack
    };
    TunFlavor::Standard { stack }
}

pub struct RuntimeBuilder;

impl RuntimeBuilder {
    /// Retain the rejected candidate's diagnostics without publishing its output.
    pub fn validate_transforms(artifact: &RuntimeArtifact) -> Result<(), RuntimeBuildError> {
        if artifact.transform_failures.is_empty() {
            return Ok(());
        }
        let logs = artifact
            .step_logs
            .iter()
            .map(|log| RuntimeBuildLog {
                tag: artifact
                    .graph
                    .nodes
                    .iter()
                    .find(|node| node.key == log.key)
                    .expect("executor logs must belong to a snapshot node")
                    .tag
                    .clone(),
                entries: log.entries.clone(),
            })
            .collect();
        Err(RuntimeBuildError::TransformsFailed {
            failures: artifact.transform_failures.clone(),
            logs,
        })
    }

    /// Executes with the shell-provided builtin table. The table is chosen
    /// before entering the pure executor, as in ref's injected script port.
    pub fn build(
        input: &RuntimeBuildInput,
        builtin_transforms: &[BuiltinTransform],
        content: &dyn ProfileContentSource,
        scripts: &dyn ScriptRunner,
    ) -> Result<RuntimeArtifact, RuntimeBuildError> {
        input
            .profiles
            .validate()
            .map_err(RuntimeBuildError::Validation)?;
        if input.clash.transparent_proxy.mode != TransparentProxyMode::Disabled
            && !input.app.enable_service_mode
        {
            return Err(RuntimeBuildError::TransparentProxyRequiresServiceMode);
        }
        input
            .clash
            .validate_transparent_proxy(input.app.core, cfg!(target_os = "linux"))?;

        let target = match &input.profiles.current {
            Some(uid) => ExecutionTarget::Selected(uid.clone()),
            None => ExecutionTarget::Bare,
        };
        let inputs = RuntimePipelineInputs {
            profiles: &input.profiles,
            target,
            guard: GuardInputs {
                overrides: &input.clash.overrides,
                ports: input.resolved_ports.clone(),
                routing_mark: (cfg!(target_os = "linux")
                    && input.clash.transparent_proxy.mode != TransparentProxyMode::Disabled)
                    .then_some(0x20000),
            },
            whitelist_enabled: input.clash.enable_clash_fields,
            core_supports_mmdb: matches!(
                input.app.core,
                chimera_config::application::ClashCore::ChimeraClient
                    | chimera_config::application::ClashCore::ChimeraClientAlpha
            ),
            tun: TunParams {
                enable: input.clash.enable_tun_mode,
                overrides: input.clash.tun,
                flavor: derive_tun_flavor(input.app.core, input.clash.tun_stack),
                windows_fake_ip_filter: cfg!(windows),
            },
            expand_include_all: input.clash.expand_include_all,
            builtin_transforms,
        };
        execute(&inputs, content, scripts).map_err(RuntimeBuildError::Pipeline)
    }
}

/// One ref-style candidate build with its execution ports supplied by the
/// shell. A script runner is *constructed and dropped* on the blocking thread:
/// Lua/JavaScript runtimes must never migrate between Tokio workers.
///
/// The caller captures the profile snapshot, port choices and staged bytes
/// before invoking this method. It retains responsibility for publication;
/// this method never mutates the runtime store or a running Core.
/// Factory that creates and drops the script runner on the *same* blocking
/// worker that executes its candidate. Matches the ref runtime build port.
pub type ScriptRunnerFactory = dyn Fn() -> anyhow::Result<Box<dyn ScriptRunner>> + Send + Sync;

/// Injected ref-style candidate builder. The application shell supplies the
/// content source and script runner factory; no global Profiles, filesystem
/// path, running Core, or runtime product is owned by this service.
///
/// Publication remains the confirmed CoreLifecycle transaction until the new
/// Reconciler is the unique runtime writer. Do not publish during candidate
/// build/check: those operations must be entirely advisory.
pub struct RuntimeBuildService {
    content: Arc<dyn ProfileContentSource + Send + Sync>,
    scripts: Arc<ScriptRunnerFactory>,
}

impl RuntimeBuildService {
    pub fn new(
        content: Arc<dyn ProfileContentSource + Send + Sync>,
        scripts: Arc<ScriptRunnerFactory>,
    ) -> Self {
        Self { content, scripts }
    }

    /// Capture every active dependency once, retaining source read failures
    /// rather than treating a failed read as absent content. This is a separate
    /// operation for the new independent RuntimeGate check workflow.
    pub async fn capture_content(
        &self,
        profiles: Arc<Profiles>,
        staged_content: BTreeMap<String, String>,
    ) -> anyhow::Result<FrozenProfileContent> {
        let content = Arc::clone(&self.content);
        tokio::task::spawn_blocking(move || {
            FrozenProfileContent::capture_content(&profiles, content.as_ref(), staged_content)
        })
        .await
        .context("profile content capture task failed")
    }

    /// Production build path. Captures managed content before constructing the
    /// script runner, then builds with that immutable input on the same worker.
    /// Unsuccessful candidates never write a runtime product or mutate Core.
    pub async fn build(
        &self,
        input: RuntimeBuildInput,
        staged_content: BTreeMap<String, String>,
        builtin_transforms: Vec<BuiltinTransform>,
        strict_transforms: bool,
    ) -> anyhow::Result<ArtifactSnapshotData> {
        let content = Arc::clone(&self.content);
        let scripts = Arc::clone(&self.scripts);
        tokio::task::spawn_blocking(move || {
            let frozen = FrozenProfileContent::capture_content(
                &input.profiles,
                content.as_ref(),
                staged_content,
            );
            Self::execute_candidate(
                input,
                frozen,
                builtin_transforms,
                move || scripts(),
                strict_transforms,
            )
        })
        .await
        .context("runtime builder task failed")?
    }

    pub async fn build_candidate<C, F>(
        input: RuntimeBuildInput,
        content: C,
        builtin_transforms: Vec<BuiltinTransform>,
        make_scripts: F,
        strict_transforms: bool,
    ) -> anyhow::Result<ArtifactSnapshotData>
    where
        C: ProfileContentSource + Send + 'static,
        F: FnOnce() -> anyhow::Result<Box<dyn ScriptRunner>> + Send + 'static,
    {
        tokio::task::spawn_blocking(move || {
            Self::execute_candidate(
                input,
                content,
                builtin_transforms,
                make_scripts,
                strict_transforms,
            )
        })
        .await
        .context("runtime builder task failed")?
    }

    /// Capture the active Profile dependency closure exactly once from the
    /// supplied source, then build from that frozen image on the same blocking
    /// thread. Only captured paths receive staged overrides. Missing/failed
    /// reads stay explicit errors rather than falling back to changed files.
    pub async fn build_with_capture<C, F>(
        input: RuntimeBuildInput,
        source: C,
        staged_content: BTreeMap<String, String>,
        builtin_transforms: Vec<BuiltinTransform>,
        make_scripts: F,
        strict_transforms: bool,
    ) -> anyhow::Result<ArtifactSnapshotData>
    where
        C: ProfileContentSource + Send + 'static,
        F: FnOnce() -> anyhow::Result<Box<dyn ScriptRunner>> + Send + 'static,
    {
        tokio::task::spawn_blocking(move || {
            let content =
                FrozenProfileContent::capture_content(&input.profiles, &source, staged_content);
            Self::execute_candidate(
                input,
                content,
                builtin_transforms,
                make_scripts,
                strict_transforms,
            )
        })
        .await
        .context("runtime builder task failed")?
    }

    fn execute_candidate<C, F>(
        input: RuntimeBuildInput,
        content: C,
        builtin_transforms: Vec<BuiltinTransform>,
        make_scripts: F,
        strict_transforms: bool,
    ) -> anyhow::Result<ArtifactSnapshotData>
    where
        C: ProfileContentSource,
        F: FnOnce() -> anyhow::Result<Box<dyn ScriptRunner>>,
    {
        let scripts = make_scripts()?;
        let artifact =
            RuntimeBuilder::build(&input, &builtin_transforms, &content, scripts.as_ref())?;
        if strict_transforms {
            RuntimeBuilder::validate_transforms(&artifact)?;
        }
        let names: Vec<_> = builtin_transforms
            .iter()
            .map(|builtin| builtin.name.clone())
            .collect();
        artifact_to_snapshot_data(artifact, &input.profiles, &names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        profile::{ManagedProfilePath, ScriptRuntime},
        runtime::{
            executor::{PortError, ScriptRunOutcome},
            value::ConfigValue,
        },
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct EmptyContent;
    impl ProfileContentSource for EmptyContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            Err(format!("no test profile for {path}").into())
        }
    }

    struct PassthroughScripts;
    impl ScriptRunner for PassthroughScripts {
        fn run(&self, _: ScriptRuntime, _: &str, config: &ConfigValue) -> ScriptRunOutcome {
            ScriptRunOutcome {
                result: Ok(config.clone()),
                logs: vec![],
            }
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    struct RejectingScripts;
    impl ScriptRunner for RejectingScripts {
        fn run(&self, _: ScriptRuntime, _: &str, _: &ConfigValue) -> ScriptRunOutcome {
            ScriptRunOutcome {
                result: Err("forced transform failure".into()),
                logs: vec![],
            }
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    fn input() -> RuntimeBuildInput {
        RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig::default(),
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn candidate_build_runs_runner_factory_on_blocking_thread_and_retains_receipt_keys() {
        let caller = std::thread::current().id();
        let factories = Arc::new(AtomicUsize::new(0));
        let called = Arc::clone(&factories);
        let data = RuntimeBuildService::build_candidate(
            input(),
            EmptyContent,
            Vec::new(),
            move || {
                assert_ne!(std::thread::current().id(), caller);
                called.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(PassthroughScripts))
            },
            true,
        )
        .await
        .unwrap();
        assert_eq!(factories.load(Ordering::SeqCst), 1);
        assert!(data.inspection.final_node < data.inspection.graph.nodes.len() as u32);
        assert!(data.postprocessing_output.scopes.is_empty());
    }

    #[tokio::test]
    async fn injected_builder_uses_one_script_factory_on_a_blocking_worker() {
        let calling_thread = std::thread::current().id();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let builder = RuntimeBuildService::new(
            Arc::new(EmptyContent),
            Arc::new(move || {
                assert_ne!(std::thread::current().id(), calling_thread);
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(Box::new(PassthroughScripts))
            }),
        );
        let data = builder
            .build(input(), BTreeMap::new(), Vec::new(), true)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(data.inspection.final_node < data.inspection.graph.nodes.len() as u32);
    }

    #[tokio::test]
    async fn strict_candidate_rejects_failed_builtin_but_lenient_candidate_retains_diagnostics() {
        let builtin = BuiltinTransform {
            name: "deliberately-failing".into(),
            runtime: ScriptRuntime::JavaScript,
            source: "throw new Error('fail')".into(),
        };
        let error = RuntimeBuildService::build_candidate(
            input(),
            EmptyContent,
            vec![builtin.clone()],
            || Ok(Box::new(RejectingScripts)),
            true,
        )
        .await
        .unwrap_err();
        let Some(RuntimeBuildError::TransformsFailed { failures, logs }) =
            error.downcast_ref::<RuntimeBuildError>()
        else {
            panic!("strict candidate must retain typed transform failure: {error:#}");
        };
        assert!(matches!(
            failures.as_slice(),
            [TransformFailure::Builtin { .. }]
        ));
        assert!(!logs.is_empty());

        let candidate = RuntimeBuildService::build_candidate(
            input(),
            EmptyContent,
            vec![builtin],
            || Ok(Box::new(RejectingScripts)),
            false,
        )
        .await
        .unwrap();
        assert!(candidate.inspection.step_logs.len() > 0);
        assert!(!candidate.postprocessing_output.global.is_empty());
    }

    #[tokio::test]
    async fn candidate_build_preserves_typed_transparent_proxy_rejection() {
        use chimera_config::clash::config::TransparentProxyMode;
        let mut input = input();
        input.clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        input.app.enable_service_mode = false;
        let error = RuntimeBuildService::build_candidate(
            input,
            EmptyContent,
            Vec::new(),
            || Ok(Box::new(PassthroughScripts)),
            true,
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<RuntimeBuildError>(),
            Some(RuntimeBuildError::TransparentProxyRequiresServiceMode)
        ));
    }
}
