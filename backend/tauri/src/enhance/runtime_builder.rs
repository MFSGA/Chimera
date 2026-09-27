//! ref-aligned runtime assembly.
//!
//! The builder is pure once its typed inputs and executor ports are prepared.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, tun_stack::TunStack},
    profile::Profiles,
    runtime::executor::{
        BuiltinTransform, ExecutionTarget, GuardInputs, PortError, ProfileContentSource,
        ResolvedPortBindings, RuntimeArtifact, RuntimePipelineError, RuntimePipelineInputs,
        ScriptRunner, TunFlavor, TunParams, execute,
    },
};

use crate::{
    config::chimera::ClashCore as LegacyClashCore,
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

/// Build from the committed or transaction-candidate Profiles snapshot.
///
/// Staged bytes are supplied by the ProfilesActor for file-first mutations;
/// they overlay disk only for this candidate build and never change the
/// materialized file source used by later builds.
pub(crate) async fn build_from_profiles_with_inspection(
    clash: &ClashConfig,
    core: LegacyClashCore,
    profiles: Arc<Profiles>,
    mut app: ChimeraAppConfig,
    resolved_ports: ResolvedPortBindings,
    staged_content: BTreeMap<String, String>,
) -> Result<(
    serde_yaml::Mapping,
    Vec<String>,
    crate::enhance::PostProcessingOutput,
    crate::client::runtime_inspection::RuntimeInspectionData,
)> {
    let core = map_core(core);
    app.core = core;
    let input = RuntimeBuildInput {
        profiles,
        clash: clash.clone(),
        app,
        resolved_ports,
    };
    let profiles_dir = dirs::app_profiles_dir()?;

    tokio::task::spawn_blocking(move || {
        let content = CandidateProfileContentSource {
            base: FsProfileContentSource::new(profiles_dir),
            staged: staged_content,
        };
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

struct CandidateProfileContentSource {
    base: FsProfileContentSource,
    staged: BTreeMap<String, String>,
}

impl ProfileContentSource for CandidateProfileContentSource {
    fn read(
        &self,
        path: &chimera_config::profile::ManagedProfilePath,
    ) -> Result<String, PortError> {
        self.staged
            .get(&path.to_string())
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.base.read(path))
    }
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
    use crate::enhance::{EnhanceScriptRunner, FsProfileContentSource};
    use chimera_config::{
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
            ScriptRuntime, ScriptTransform, TransformDefinition,
        },
        runtime::executor::{PortError, ScriptRunOutcome},
        runtime::value::ConfigValue,
    };
    use serde_json::json;

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
        assert_eq!(
            names(chimera_config::application::ClashCore::ChimeraClient),
            vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"]
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
    fn chimera_client_runtime_uses_its_custom_tun_contract_in_shared_executor() {
        let mut input = RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig::default(),
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        input.app.core = chimera_config::application::ClashCore::ChimeraClient;
        input.app.enable_builtin_enhanced = true;
        input.clash.enable_tun_mode = true;

        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        let config = artifact.final_config.to_json();

        assert!(format!("{:?}", artifact.graph).contains("verge_hy_alpn"));
        assert_eq!(config["tun"]["device-id"], json!("dev://utun1989"));
        assert_eq!(config["tun"]["route-all"], json!(true));
        assert_eq!(config["tun"]["dns-hijack"], json!(true));
        assert_eq!(config["tun"]["so-mark"], json!(7777));
        assert_eq!(
            config["dns"]["nameserver"],
            json!([
                "https://dns.alidns.com/dns-query",
                "114.114.114.114",
                "223.5.5.5",
                "8.8.8.8"
            ])
        );
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

    /// Exercises the same on-disk source and JavaScript runner used by the
    /// production builder, including scoped transform output and step logs.
    #[test]
    fn golden_selected_file_with_script_transform_end_to_end() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("cfg1.yaml"),
            "proxies: []\nmode: direct\nextra-key: keep\n",
        )
        .unwrap();
        std::fs::write(
            temp.path().join("scr1.js"),
            "function main(config) { config[\"mode\"] = \"rule\"; console.log(\"scoped ran\"); return config; }\n",
        )
        .unwrap();

        let managed = |name: &str| MaterializedFile {
            file: ManagedProfilePath::new(name).unwrap(),
            updated_at: None,
        };
        let mut profiles = Profiles::default();
        profiles.append_item(ProfileItem {
            uid: ProfileId("cfg1".into()),
            metadata: ProfileMetadata {
                name: "CFG1".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: managed("cfg1.yaml"),
                        },
                    },
                    transforms: vec![ProfileId("scr1".into())],
                }),
            },
        });
        profiles.append_item(ProfileItem {
            uid: ProfileId("scr1".into()),
            metadata: ProfileMetadata {
                name: "SCR1".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Transform {
                transform: TransformDefinition::Script(ScriptTransform {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: managed("scr1.js"),
                        },
                    },
                    runtime: ScriptRuntime::JavaScript,
                }),
            },
        });
        profiles.set_current(Some(ProfileId("cfg1".into())));

        let input = RuntimeBuildInput {
            profiles: Arc::new(profiles),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig {
                enable_builtin_enhanced: false,
                ..ChimeraAppConfig::default()
            },
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        let content = FsProfileContentSource::new(temp.path().to_path_buf());
        let scripts = EnhanceScriptRunner::new().unwrap();
        let artifact = RuntimeBuilder::build(&input, &content, &scripts).unwrap();
        let config = artifact.final_config.to_json();

        assert_eq!(config["mode"], json!("rule"));
        assert_eq!(config["extra-key"], json!("keep"));
        assert_eq!(config["mixed-port"], json!(7890));
        assert!(
            artifact.step_logs.iter().any(|log| {
                log.entries
                    .iter()
                    .any(|entry| entry.message.contains("scoped ran"))
            }),
            "script logs must be anchored for the postprocessing_output consumer: {:#?}",
            artifact.step_logs
        );
    }
}
