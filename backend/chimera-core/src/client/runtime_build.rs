//! Candidate orchestration over the shared profile-transform crate.
use anyhow::Context;
use chimera_config::{
    profile::Profiles,
    runtime::executor::{BuiltinTransform, ProfileContentSource, ScriptRunner},
};
use std::{collections::BTreeMap, sync::Arc};

use super::{
    artifact_snapshot::{ArtifactSnapshotData, artifact_to_snapshot_data},
    runtime_build_port::FrozenProfileContent,
};
pub use chimera_profile_transform::{
    RuntimeBuildError, RuntimeBuildInput, RuntimeBuildLog, RuntimeBuilder, builtin_transforms_for,
    derive_tun_flavor,
};

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
        let artifact = RuntimeBuilder::build_with_builtins(
            &input,
            &builtin_transforms,
            &content,
            scripts.as_ref(),
        )?;
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
        application::{ChimeraAppConfig, ClashCore},
        clash::config::ClashConfig,
        runtime::executor::{ResolvedPortBindings, TransformFailure},
    };
    use chimera_config::{
        profile::{ManagedProfilePath, ScriptRuntime},
        runtime::{
            executor::{PortError, StepLogEntry},
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

    struct RejectingScripts;
    impl ScriptRunner for RejectingScripts {
        fn run(
            &self,
            _: ScriptRuntime,
            _: &str,
            _: &ConfigValue,
            _: &mut Vec<StepLogEntry>,
        ) -> Result<ConfigValue, PortError> {
            Err("forced transform failure".into())
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    struct EmitInvalidStack;
    impl ScriptRunner for EmitInvalidStack {
        fn run(
            &self,
            _: ScriptRuntime,
            _: &str,
            _: &ConfigValue,
            _: &mut Vec<StepLogEntry>,
        ) -> Result<ConfigValue, PortError> {
            Ok(ConfigValue::try_from(serde_json::json!({
                "proxies": [],
                "tun": {"enable": false, "stack": "mixed"}
            }))
            .unwrap())
        }
        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }
        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    #[test]
    fn mips_controller_is_emitted_only_when_selected_for_mihomo() {
        use chimera_config::clash::config::tun_stack::{TunCongestionController, TunStack};
        let mut candidate = input();
        candidate.app.core = ClashCore::Mihomo;
        candidate.clash.enable_tun_mode = true;
        candidate.clash.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        for (stack, expected) in [(TunStack::Mips, Some("bbr3")), (TunStack::Gvisor, None)] {
            candidate.clash.tun_stack = stack;
            let artifact = RuntimeBuilder::build_with_builtins(
                &candidate,
                &[],
                &EmptyContent,
                &PassthroughScripts,
            )
            .unwrap();
            let config = artifact.final_config.to_json();
            assert_eq!(config["tun"]["stack"], serde_json::json!(stack.as_ref()));
            assert_eq!(
                config["tun"]
                    .get("congestion-controller")
                    .and_then(|v| v.as_str()),
                expected
            );
        }
    }

    #[test]
    fn unsupported_premium_mips_downgrades_with_warning_but_keeps_saved_selection() {
        use chimera_config::clash::config::tun_stack::{TunCongestionController, TunStack};
        let mut candidate = input();
        candidate.app.core = ClashCore::ClashPremium;
        candidate.clash.enable_tun_mode = true;
        candidate.clash.tun_stack = TunStack::Mips;
        candidate.clash.tun_mips.congestion_controller = Some(TunCongestionController::Bbr);
        let artifact = RuntimeBuilder::build_with_builtins(
            &candidate,
            &[],
            &EmptyContent,
            &PassthroughScripts,
        )
        .unwrap();
        let config = artifact.final_config.to_json();
        assert_eq!(config["tun"]["stack"], "gvisor");
        assert!(config["tun"].get("congestion-controller").is_none());
        assert_eq!(candidate.clash.tun_stack, TunStack::Mips);
        let warnings: Vec<_> = artifact
            .step_logs
            .iter()
            .flat_map(|log| &log.entries)
            .filter(|entry| entry.level == chimera_config::runtime::executor::StepLogLevel::Warn)
            .collect();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].message.contains("mips"));
        assert!(warnings[0].message.contains("gvisor"));

        candidate.clash.enable_tun_mode = false;
        let disabled = RuntimeBuilder::build_with_builtins(
            &candidate,
            &[],
            &EmptyContent,
            &PassthroughScripts,
        )
        .unwrap();
        assert!(disabled.step_logs.iter().all(|log| {
            log.entries
                .iter()
                .all(|entry| entry.level != chimera_config::runtime::executor::StepLogLevel::Warn)
        }));
    }

    #[test]
    fn final_config_validation_rejects_a_known_unsupported_stack_even_if_tun_is_off() {
        let mut candidate = input();
        candidate.app.core = ClashCore::ClashPremium;
        candidate.clash.enable_tun_mode = false;
        let builtins = [BuiltinTransform {
            name: "invalid-tun-script".into(),
            runtime: ScriptRuntime::JavaScript,
            source: "replace".into(),
        }];
        let error = RuntimeBuilder::build_with_builtins(
            &candidate,
            &builtins,
            &EmptyContent,
            &EmitInvalidStack,
        )
        .unwrap_err();
        let RuntimeBuildError::UnsupportedConfigValues { values } = error else {
            panic!("unsupported stack must reject the candidate, not publish it");
        };
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].path, "tun.stack");
        assert_eq!(values[0].value, "mixed");
        assert_eq!(values[0].supported, ["system", "gvisor"]);
    }

    #[test]
    fn client_core_does_not_inherit_mihomo_stack_or_mips_options() {
        use chimera_config::clash::config::tun_stack::{TunCongestionController, TunStack};
        let mut candidate = input();
        candidate.app.core = ClashCore::ChimeraClient;
        candidate.clash.enable_tun_mode = true;
        candidate.clash.tun_stack = TunStack::Mips;
        candidate.clash.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        let config = RuntimeBuilder::build_with_builtins(
            &candidate,
            &[],
            &EmptyContent,
            &PassthroughScripts,
        )
        .unwrap()
        .final_config
        .to_json();
        assert!(config["tun"].get("stack").is_none());
        assert!(config["tun"].get("congestion-controller").is_none());
        assert_eq!(config["tun"]["route-all"], true);
        assert_eq!(config["tun"]["so-mark"], 7777);
        assert!(config["dns"]["default-nameserver"].is_array());
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
