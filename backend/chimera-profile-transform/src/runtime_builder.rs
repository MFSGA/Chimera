//! RuntimeBuilder: pure assembly from domain snapshots to the runtime pipeline
//! executor (PR-3 T06, design §8 + §19). No globals, no IO — port resolution
//! happens at the caller and file reads/script runs arrive via executor ports.
//! Must be called from a blocking context (the script adapter blocks on its
//! own runtime); the facade wraps the whole build in spawn_blocking (T07).

use std::sync::Arc;

use chimera_config::{
    application::{ChimeraAppConfig, ClashCore},
    clash::{
        config::{
            ClashConfig, TransparentProxyMode, TransparentProxyValidationError,
            dns::{RenderedTable, render_hosts},
            tun_stack::SelectedTunStack,
        },
        features::{DnsFeatures, FieldSupport, TunFeatures, UnsupportedConfigValue},
    },
    profile::{Profiles, ScriptRuntime},
    runtime::executor::{
        BuiltinTransform, DnsParams, ExecutionTarget, GuardInputs, ProfileContentSource,
        ResolvedPortBindings, RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs,
        ScriptRunner, TransformFailure, TunFlavor, TunParams, TunStackWrite, execute,
    },
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
    #[error("the selected core does not support {values:?}")]
    UnsupportedConfigValues { values: Vec<UnsupportedConfigValue> },
    #[error("DNS/Hosts takeover is not yet supported for the selected core: {0:?}")]
    UnsupportedDnsCore(chimera_config::application::ClashCore),
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
    pub tag: chimera_config::runtime::snapshot::OperatorTag,
    pub entries: Vec<chimera_config::runtime::executor::StepLogEntry>,
}

pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,
    pub clash: ClashConfig,
    pub app: ChimeraAppConfig,
    pub resolved_ports: ResolvedPortBindings,
}

const MIHOMO_FAMILY: &[ClashCore] = &[
    ClashCore::Mihomo,
    ClashCore::MihomoAlpha,
    ClashCore::ChimeraClient,
    ClashCore::ChimeraClientAlpha,
];
const ALL_CORES: &[ClashCore] = &[
    ClashCore::ClashPremium,
    ClashCore::ClashRs,
    ClashCore::Mihomo,
    ClashCore::MihomoAlpha,
    ClashCore::ClashRsAlpha,
    ClashCore::ChimeraClient,
    ClashCore::ChimeraClientAlpha,
];
const CLASH_RS_ONLY: &[ClashCore] = &[ClashCore::ClashRs];

/// Builtin transform table kept in ref order.
pub fn builtin_transforms_for(core: ClashCore) -> Vec<BuiltinTransform> {
    let table: [(&[ClashCore], &str, ScriptRuntime, &str); 4] = [
        (
            MIHOMO_FAMILY,
            "verge_hy_alpn",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_hy_alpn.js"),
        ),
        (
            MIHOMO_FAMILY,
            "verge_meta_guard",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_guard.js"),
        ),
        (
            ALL_CORES,
            "config_fixer",
            ScriptRuntime::JavaScript,
            include_str!("./builtin/config_fixer.js"),
        ),
        (
            CLASH_RS_ONLY,
            "clash_rs_comp",
            ScriptRuntime::Lua,
            include_str!("./builtin/clash_rs_comp.lua"),
        ),
    ];
    table
        .into_iter()
        .filter(|(gate, ..)| gate.contains(&core))
        .map(|(_, name, runtime, source)| BuiltinTransform {
            name: name.to_string(),
            runtime,
            source: source.to_string(),
        })
        .collect()
}

/// Only `ClashRs` (not Alpha) takes the ClashRs branch, a legacy quirk
/// (enhance/tun.rs:47). The stack and its options follow the core's config
/// features.
pub fn derive_tun_flavor(core: ClashCore, selected: SelectedTunStack<'_>) -> TunFlavor {
    if core == ClashCore::ClashRs {
        return TunFlavor::ClashRs;
    }
    if matches!(
        core,
        ClashCore::ChimeraClient | ClashCore::ChimeraClientAlpha
    ) {
        return TunFlavor::ChimeraClient;
    }
    let features = core.config_features().tun;
    let stack = match features.stack {
        FieldSupport::Ignored => TunStackWrite::Omitted,
        support if support.supports(&selected.stack()) => TunStackWrite::Selected(selected.stack()),
        FieldSupport::Values(_) => TunStackWrite::Downgraded {
            selected: selected.stack(),
            written: TunFeatures::FALLBACK_STACK,
        },
    };
    // A downgraded stack drops the options of the stack that was selected.
    let congestion_controller = match (stack, selected) {
        (TunStackWrite::Selected(_), SelectedTunStack::Mips(mips)) => mips
            .congestion_controller
            .filter(|controller| features.congestion_controller.supports(controller)),
        _ => None,
    };
    TunFlavor::Standard {
        stack,
        congestion_controller,
    }
}

/// The `dns` and `hosts` tables Chimera writes, rendered for the core's
/// `features`. A core without a `dns` table gets neither.
fn derive_dns_params(clash: &ClashConfig, features: &DnsFeatures) -> DnsParams {
    let dns = clash.managed_dns().and_then(|dns| dns.render(features));
    let hosts = clash
        .managed_hosts()
        .and_then(|hosts| render_hosts(hosts, features));
    let mut warnings = Vec::new();
    let mut take = |rendered: Option<RenderedTable>| {
        rendered.map(|rendered| {
            warnings.extend(rendered.warnings);
            rendered.table
        })
    };
    DnsParams {
        dns: take(dns),
        hosts: take(hosts),
        warnings,
    }
}

pub struct RuntimeBuilder;

impl RuntimeBuilder {
    /// Retain diagnostics from the rejected candidate, which is never published.
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
                    .find(|node| node.tag.node_key() == log.key)
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

    pub fn build(
        input: &RuntimeBuildInput,
        content: &dyn ProfileContentSource,
        scripts: &dyn ScriptRunner,
    ) -> Result<RuntimeArtifact, RuntimeBuildError> {
        let builtin_transforms = if input.app.enable_builtin_enhanced {
            builtin_transforms_for(input.app.core)
        } else {
            Vec::new()
        };
        Self::build_with_builtins(input, &builtin_transforms, content, scripts)
    }

    /// Explicit table injection for compatibility and deterministic fixtures.
    pub fn build_with_builtins(
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

        let dns_features = input.app.core.dns_features();
        if dns_features.keys.is_ignored()
            && (input.clash.managed_dns().is_some() || input.clash.managed_hosts().is_some())
        {
            return Err(RuntimeBuildError::UnsupportedDnsCore(input.app.core));
        }

        let target = match &input.profiles.current {
            Some(uid) => ExecutionTarget::Selected(uid.clone()),
            None => ExecutionTarget::Bare,
        };
        let features = input.app.core.config_features();
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
                ClashCore::ChimeraClient | ClashCore::ChimeraClientAlpha
            ),
            tun: TunParams {
                enable: input.clash.enable_tun_mode,
                overrides: input.clash.tun,
                flavor: derive_tun_flavor(input.app.core, input.clash.selected_tun_stack()),
                windows_fake_ip_filter: cfg!(windows),
            },
            dns: derive_dns_params(&input.clash, &dns_features),
            expand_include_all: input.clash.expand_include_all,
            builtin_transforms,
        };
        let artifact = execute(&inputs, content, scripts).map_err(RuntimeBuildError::Pipeline)?;

        // The core parses these keys whether TUN is on or not, and rejects the
        // whole config on a value it does not know.
        let values = features.unsupported_values(&artifact.final_config);
        if !values.is_empty() {
            return Err(RuntimeBuildError::UnsupportedConfigValues { values });
        }
        Ok(artifact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        clash::config::{
            overrides::ManageableField,
            tun_stack::{MipsConfig, TunCongestionController, TunStack},
        },
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
            ScriptRuntime,
        },
        runtime::{
            executor::{PortError, StepLogEntry, StepLogLevel},
            value::ConfigValue,
        },
    };

    /// 极小 fakes(executor 的 support.rs 是 crate 内部,tauri 不可 import)
    struct EmptyContent;
    impl ProfileContentSource for EmptyContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            Err(format!("no content for {path}").into())
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

    fn base_input() -> RuntimeBuildInput {
        RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig {
                core: ClashCore::Mihomo,
                ..Default::default()
            },
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        }
    }

    #[test]
    fn builtin_gating_matches_legacy_table() {
        let names = |core: ClashCore| -> Vec<String> {
            builtin_transforms_for(core)
                .into_iter()
                .map(|b| b.name)
                .collect()
        };
        assert_eq!(
            names(ClashCore::Mihomo),
            vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"]
        );
        assert_eq!(
            names(ClashCore::ClashRs),
            vec!["config_fixer", "clash_rs_comp"]
        );
        // legacy 怪癖忠实移植:Alpha 不吃 clash_rs_comp(chain.rs:174)
        assert_eq!(names(ClashCore::ClashRsAlpha), vec!["config_fixer"]);
        // Chimera Client retains the Mihomo-compatible transforms.
        assert_eq!(names(ClashCore::ChimeraClient), names(ClashCore::Mihomo));
    }

    /// Serves one profile file whatever path is asked for.
    struct OneFile(&'static str);
    impl ProfileContentSource for OneFile {
        fn read(&self, _: &ManagedProfilePath) -> Result<String, PortError> {
            Ok(self.0.to_owned())
        }
    }

    /// `base_input` with one selected config profile and no builtin
    /// transforms.
    fn profile_input(core: ClashCore) -> RuntimeBuildInput {
        let uid = ProfileId("p".into());
        let item = ProfileItem {
            uid: uid.clone(),
            metadata: ProfileMetadata {
                name: "p".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("p.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        };
        let mut profiles = Profiles::default();
        profiles.items.insert(uid.clone(), item);
        profiles.current = Some(uid);

        let mut input = base_input();
        input.profiles = Arc::new(profiles);
        input.app.core = core;
        input.app.enable_builtin_enhanced = false;
        input
    }

    #[test]
    fn tun_flavor_follows_the_core_features() {
        let standard = |stack, congestion_controller| TunFlavor::Standard {
            stack,
            congestion_controller,
        };
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRs, SelectedTunStack::Mixed),
            TunFlavor::ClashRs
        );
        // Alpha keeps the Standard branch (tun.rs:47 legacy quirk), without a
        // stack it does not read.
        for core in [ClashCore::ClashRsAlpha] {
            assert_eq!(
                derive_tun_flavor(core, SelectedTunStack::Mixed),
                standard(TunStackWrite::Omitted, None),
                "{core}"
            );
        }
        assert_eq!(
            derive_tun_flavor(ClashCore::Mihomo, SelectedTunStack::System),
            standard(TunStackWrite::Selected(TunStack::System), None)
        );

        let mips = MipsConfig {
            congestion_controller: Some(TunCongestionController::Bbr),
        };
        assert_eq!(
            derive_tun_flavor(ClashCore::MihomoAlpha, SelectedTunStack::Mips(&mips)),
            standard(
                TunStackWrite::Selected(TunStack::Mips),
                Some(TunCongestionController::Bbr)
            )
        );
    }

    #[test]
    fn chimera_client_retains_its_distinct_tun_flavor() {
        for core in [ClashCore::ChimeraClient, ClashCore::ChimeraClientAlpha] {
            assert_eq!(
                derive_tun_flavor(core, SelectedTunStack::Mixed),
                TunFlavor::ChimeraClient
            );
        }
    }

    #[test]
    fn mips_options_reach_the_runtime_only_while_mips_is_selected() {
        let mut input = profile_input(ClashCore::Mihomo);
        input.clash.enable_tun_mode = true;
        input.clash.tun_mips.congestion_controller = Some(TunCongestionController::Bbr3);
        for (stack, expected) in [(TunStack::Mips, Some("bbr3")), (TunStack::Gvisor, None)] {
            input.clash.tun_stack = stack;
            let config = RuntimeBuilder::build(&input, &OneFile("proxies: []\n"), &EchoRunner)
                .unwrap()
                .final_config
                .to_json();
            assert_eq!(
                config["tun"].get("congestion-controller"),
                expected.map(serde_json::Value::from).as_ref(),
                "{stack:?}"
            );
        }
    }

    /// Kept settings have no effect while the profiles control DNS; once
    /// Chimera does, the table is rendered for the selected core.
    #[test]
    fn dns_settings_reach_the_runtime_only_while_controlled() {
        use chimera_config::clash::config::dns::{ControlSource, OneOrMany};

        let profile =
            OneFile("dns: { enable: true, nameserver: [1.1.1.1] }\nhosts: { a.lan: 10.0.0.1 }\n");
        let mut input = profile_input(ClashCore::ClashRs);
        input.clash.dns.prefer_h3 = true;
        input.clash.hosts =
            indexmap::IndexMap::from([("b.lan".to_owned(), OneOrMany::One("10.0.0.2".into()))]);

        let config = RuntimeBuilder::build(&input, &profile, &EchoRunner)
            .unwrap()
            .final_config
            .to_json();
        assert_eq!(
            config["dns"],
            serde_json::json!({ "enable": true, "nameserver": ["1.1.1.1"] })
        );

        input.clash.dns_control = ControlSource::Chimera;
        input.clash.hosts_control = ControlSource::Chimera;
        let artifact = RuntimeBuilder::build(&input, &profile, &EchoRunner).unwrap();
        let config = artifact.final_config.to_json();
        assert_eq!(
            config["dns"]["nameserver"],
            serde_json::json!(input.clash.dns.nameserver)
        );
        assert!(
            config["dns"].get("prefer-h3").is_none(),
            "clash-rs does not read it"
        );
        assert_eq!(config["hosts"], serde_json::json!({ "b.lan": "10.0.0.2" }));
        let warnings: Vec<_> = artifact
            .step_logs
            .iter()
            .flat_map(|log| &log.entries)
            .filter(|entry| entry.level == StepLogLevel::Warn)
            .map(|entry| entry.message.as_str())
            .collect();
        assert_eq!(
            warnings,
            vec!["the selected core does not read `dns.prefer-h3`; it is not written"]
        );
    }

    #[test]
    fn the_selected_stack_overrides_the_profile_stack_while_tun_is_on() {
        let mut input = profile_input(ClashCore::Mihomo);
        input.clash.enable_tun_mode = true;
        input.clash.tun_stack = TunStack::System;
        let artifact = RuntimeBuilder::build(
            &input,
            &OneFile(
                "tun: { stack: mixed }
",
            ),
            &EchoRunner,
        )
        .unwrap();
        assert_eq!(
            artifact.final_config.to_json()["tun"]["stack"],
            serde_json::json!("system")
        );
        let warnings: Vec<_> = artifact
            .step_logs
            .iter()
            .flat_map(|log| &log.entries)
            .filter(|entry| entry.level == StepLogLevel::Warn)
            .collect();
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn bare_build_produces_artifact_with_guarded_ports() {
        let mut input = base_input(); // current = None → Bare
        input.app.enable_builtin_enhanced = false; // EchoRunner 下 builtin 无意义
        let artifact =
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).expect("bare build");
        let yaml = serde_yaml::to_value(artifact.final_config.to_json()).expect("artifact to yaml");
        assert_eq!(yaml["mixed-port"], serde_yaml::Value::from(7890));
    }

    #[test]
    fn tun_auto_route_policy_reaches_the_runtime_without_releasing_tun_enable() {
        for auto_route in [
            ManageableField::Managed(true),
            ManageableField::Managed(false),
            ManageableField::Unmanaged,
        ] {
            for enabled in [false, true] {
                let mut input = base_input();
                input.app.enable_builtin_enhanced = false;
                input.app.core = ClashCore::Mihomo;
                input.clash.enable_tun_mode = enabled;
                input.clash.tun.auto_route = auto_route;
                let config = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner)
                    .unwrap()
                    .final_config
                    .to_json();
                if enabled {
                    assert_eq!(config["tun"]["enable"], serde_json::json!(true));
                    match auto_route {
                        ManageableField::Managed(value) => {
                            assert_eq!(config["tun"]["auto-route"], serde_json::json!(value))
                        }
                        ManageableField::Unmanaged => {
                            assert!(config["tun"].get("auto-route").is_none())
                        }
                    }
                } else {
                    assert!(config.get("tun").is_none());
                }
            }
        }
    }

    #[test]
    fn invalid_profiles_rejected_before_executor() {
        let mut input = base_input();
        let mut profiles = Profiles::default();
        profiles.set_current(Some(ProfileId("ghost".into())));
        input.profiles = Arc::new(profiles);
        assert!(matches!(
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner),
            Err(RuntimeBuildError::Validation(_))
        ));
    }

    #[test]
    fn transparent_capture_requires_service_mode_during_runtime_build() {
        let mut input = base_input();
        input.clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;

        assert!(matches!(
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner),
            Err(RuntimeBuildError::TransparentProxyRequiresServiceMode)
        ));
    }

    #[test]
    fn builtin_disabled_flag_empties_the_list() {
        let mut input = base_input();
        input.app.enable_builtin_enhanced = false;
        input.app.core = ClashCore::Mihomo;
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        let debug = format!("{:?}", artifact.graph);
        assert!(!debug.contains("verge_hy_alpn"));
    }
}
