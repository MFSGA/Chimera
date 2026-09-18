//! ref-aligned runtime assembly.
//!
//! The builder is pure once its inputs and executor ports are prepared. The
//! legacy global profile store is converted at the boundary and is not read by
//! the executor itself.

use std::sync::Arc;

use anyhow::{Context, Result};
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, tun_stack::TunStack},
    profile::Profiles,
    runtime::executor::{
        BuiltinTransform, ExecutionTarget, GuardInputs, ProfileContentSource, ResolvedPortBindings,
        RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs, ScriptRunner, TunFlavor,
        TunParams, execute,
    },
};

use crate::{
    config::{
        chimera::ClashCore as LegacyClashCore, core::Config,
        profile::ref_adapter::to_runtime_profiles,
    },
    enhance::{
        EnhanceScriptRunner, FsProfileContentSource,
        artifact_bridge::artifact_to_legacy_output_with_inspection,
    },
    utils::dirs,
};

#[derive(Debug, thiserror::Error)]
pub enum RuntimeBuildError {
    #[error("profiles snapshot failed validation: {0:?}")]
    Validation(Vec<chimera_config::profile::ProfileValidationError>),
    #[error(transparent)]
    Pipeline(#[from] RuntimePipelineError),
}

pub struct RuntimeBuildInput {
    pub profiles: Arc<Profiles>,
    pub clash: ClashConfig,
    pub app: ChimeraAppConfig,
    pub resolved_ports: ResolvedPortBindings,
}

const MIHOMO_FAMILY: &[chimera_config::application::ClashCore] = &[
    chimera_config::application::ClashCore::Mihomo,
    chimera_config::application::ClashCore::MihomoAlpha,
    chimera_config::application::ClashCore::ChimeraClient,
];
const ALL_CORES: &[chimera_config::application::ClashCore] = &[
    chimera_config::application::ClashCore::ClashPremium,
    chimera_config::application::ClashCore::ClashRs,
    chimera_config::application::ClashCore::Mihomo,
    chimera_config::application::ClashCore::MihomoAlpha,
    chimera_config::application::ClashCore::ClashRsAlpha,
    chimera_config::application::ClashCore::ChimeraClient,
];
const CLASH_RS_ONLY: &[chimera_config::application::ClashCore] =
    &[chimera_config::application::ClashCore::ClashRs];

/// Builtin transform table kept in ref order.
pub fn builtin_transforms_for(
    core: chimera_config::application::ClashCore,
) -> Vec<BuiltinTransform> {
    let table: [(
        &[chimera_config::application::ClashCore],
        &str,
        chimera_config::profile::ScriptRuntime,
        &str,
    ); 4] = [
        (
            MIHOMO_FAMILY,
            "verge_hy_alpn",
            chimera_config::profile::ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_hy_alpn.js"),
        ),
        (
            MIHOMO_FAMILY,
            "verge_meta_guard",
            chimera_config::profile::ScriptRuntime::JavaScript,
            include_str!("./builtin/meta_guard.js"),
        ),
        (
            ALL_CORES,
            "config_fixer",
            chimera_config::profile::ScriptRuntime::JavaScript,
            include_str!("./builtin/config_fixer.js"),
        ),
        (
            CLASH_RS_ONLY,
            "clash_rs_comp",
            chimera_config::profile::ScriptRuntime::Lua,
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

/// Legacy TUN derivation, including Premium+Mixed -> Gvisor.
pub fn derive_tun_flavor(
    core: chimera_config::application::ClashCore,
    stack: TunStack,
) -> TunFlavor {
    if core == chimera_config::application::ClashCore::ClashRs {
        return TunFlavor::ClashRs;
    }
    if core == chimera_config::application::ClashCore::ChimeraClient {
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
    pub fn build(
        input: &RuntimeBuildInput,
        content: &dyn ProfileContentSource,
        scripts: &dyn ScriptRunner,
    ) -> Result<RuntimeArtifact, RuntimeBuildError> {
        input
            .profiles
            .validate()
            .map_err(RuntimeBuildError::Validation)?;

        let target = match &input.profiles.current {
            Some(uid) => ExecutionTarget::Selected(uid.clone()),
            None => ExecutionTarget::Bare,
        };
        let builtin_transforms = if input.app.enable_builtin_enhanced {
            builtin_transforms_for(input.app.core)
        } else {
            Vec::new()
        };
        let inputs = RuntimePipelineInputs {
            profiles: &input.profiles,
            target,
            guard: GuardInputs {
                overrides: &input.clash.overrides,
                ports: input.resolved_ports.clone(),
            },
            whitelist_enabled: input.clash.enable_clash_fields,
            tun: TunParams {
                enable: input.clash.enable_tun_mode,
                flavor: derive_tun_flavor(input.app.core, input.clash.tun_stack),
                windows_fake_ip_filter: cfg!(windows),
            },
            builtin_transforms: &builtin_transforms,
        };
        execute(&inputs, content, scripts).map_err(RuntimeBuildError::Pipeline)
    }
}

/// Build the ref pipeline from the existing persisted Chimera state. The
/// Chimera Client core remains on the legacy path until its custom TUN
/// contract is represented in the shared executor.
pub async fn build_from_legacy(
    clash: &ClashConfig,
    core: LegacyClashCore,
) -> Result<(serde_yaml::Mapping, crate::enhance::PostProcessingOutput)> {
    let client_info = Config::clash().latest().get_client_info();
    let resolved_ports = ResolvedPortBindings {
        mixed_port: client_info.port,
        external_controller: Some(client_info.server),
        ..ResolvedPortBindings::default()
    };
    build_from_legacy_with_inspection(clash, core, resolved_ports)
        .await
        .map(|(mapping, _exists_keys, output, _inspection)| (mapping, output))
}

pub(crate) async fn build_from_legacy_with_inspection(
    clash: &ClashConfig,
    core: LegacyClashCore,
    resolved_ports: ResolvedPortBindings,
) -> Result<(
    serde_yaml::Mapping,
    Vec<String>,
    crate::enhance::PostProcessingOutput,
    crate::client::runtime_inspection::RuntimeInspectionData,
)> {
    let profiles = Arc::new(to_runtime_profiles(&Config::profiles().latest())?);
    let mut app = ChimeraAppConfig::default();
    app.core = map_core(core);
    app.enable_builtin_enhanced = Config::verge()
        .latest()
        .enable_builtin_enhanced
        .unwrap_or(true);

    let input = RuntimeBuildInput {
        profiles: profiles.clone(),
        clash: clash.clone(),
        app,
        resolved_ports,
    };
    let profiles_dir = dirs::app_profiles_dir()?;

    tokio::task::spawn_blocking(move || {
        let content = FsProfileContentSource::new(profiles_dir);
        let scripts = EnhanceScriptRunner::new()?;
        let artifact = RuntimeBuilder::build(&input, &content, &scripts)
            .map_err(|error| anyhow::anyhow!(error))?;
        artifact_to_legacy_output_with_inspection(
            artifact,
            &input.profiles,
            input.app.core,
            input.app.enable_builtin_enhanced,
        )
    })
    .await
    .context("runtime builder task failed")?
}

fn map_core(core: LegacyClashCore) -> chimera_config::application::ClashCore {
    match core {
        LegacyClashCore::ClashPremium => chimera_config::application::ClashCore::ClashPremium,
        LegacyClashCore::ClashRs => chimera_config::application::ClashCore::ClashRs,
        LegacyClashCore::Mihomo => chimera_config::application::ClashCore::Mihomo,
        LegacyClashCore::MihomoAlpha => chimera_config::application::ClashCore::MihomoAlpha,
        LegacyClashCore::ClashRsAlpha => chimera_config::application::ClashCore::ClashRsAlpha,
        LegacyClashCore::ChimeraClient => chimera_config::application::ClashCore::ChimeraClient,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::{
        profile::{ManagedProfilePath, ProfileId, ScriptRuntime},
        runtime::executor::{PortError, ScriptRunOutcome},
        runtime::value::ConfigValue,
    };

    struct EmptyContent;

    impl ProfileContentSource for EmptyContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            Err(format!("no content for {path}").into())
        }
    }

    struct EchoRunner;

    impl ScriptRunner for EchoRunner {
        fn run(&self, _: ScriptRuntime, _: &str, config: &ConfigValue) -> ScriptRunOutcome {
            ScriptRunOutcome {
                result: Ok(config.clone()),
                logs: Vec::new(),
            }
        }

        fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
            Ok(true)
        }

        fn eval_item_expr(&self, _: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
            Ok(item.clone())
        }
    }

    #[test]
    fn builtin_gating_matches_ref_table() {
        let names = |core| {
            builtin_transforms_for(core)
                .into_iter()
                .map(|item| item.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(chimera_config::application::ClashCore::Mihomo),
            vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"]
        );
        assert_eq!(
            names(chimera_config::application::ClashCore::ClashRs),
            vec!["config_fixer", "clash_rs_comp"]
        );
        assert_eq!(
            names(chimera_config::application::ClashCore::ClashRsAlpha),
            vec!["config_fixer"]
        );
    }

    #[test]
    fn tun_flavor_derivation_matches_ref_quirks() {
        assert_eq!(
            derive_tun_flavor(
                chimera_config::application::ClashCore::ClashRs,
                TunStack::Mixed
            ),
            TunFlavor::ClashRs
        );
        assert_eq!(
            derive_tun_flavor(
                chimera_config::application::ClashCore::ClashRsAlpha,
                TunStack::Mixed
            ),
            TunFlavor::Standard {
                stack: TunStack::Mixed
            }
        );
        assert_eq!(
            derive_tun_flavor(
                chimera_config::application::ClashCore::ClashPremium,
                TunStack::Mixed
            ),
            TunFlavor::Standard {
                stack: TunStack::Gvisor
            }
        );
    }

    #[test]
    fn builtin_disabled_flag_empties_the_list() {
        let mut input = RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig::default(),
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        input.app.enable_builtin_enhanced = false;
        input.app.core = chimera_config::application::ClashCore::Mihomo;
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        assert!(!format!("{:?}", artifact.graph).contains("verge_hy_alpn"));
    }

    #[test]
    fn invalid_profiles_rejected_before_executor() {
        let mut profiles = Profiles::default();
        profiles.current = Some(ProfileId("ghost".into()));
        let input = RuntimeBuildInput {
            profiles: Arc::new(profiles),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig::default(),
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        assert!(matches!(
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner),
            Err(RuntimeBuildError::Validation(_))
        ));
    }
}
