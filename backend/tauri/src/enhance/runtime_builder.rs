//! ref-aligned runtime assembly.
//!
//! The builder is pure once its typed inputs and executor ports are prepared.

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Result;
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, tun_stack::TunStack},
    profile::Profiles,
    runtime::executor::{
        BuiltinTransform, ProfileContentSource, ResolvedPortBindings, RuntimeArtifact,
        ScriptRunner, TunFlavor,
    },
};

pub use chimera_core::client::runtime_build::{RuntimeBuildError, RuntimeBuildInput};

use crate::{
    config::chimera::ClashCore as LegacyClashCore,
    enhance::{
        EnhanceScriptRunner, FsProfileContentSource,
        artifact_bridge::artifact_to_legacy_output_with_inspection,
    },
    utils::dirs,
};

const MIHOMO_FAMILY: &[chimera_config::application::ClashCore] = &[
    chimera_config::application::ClashCore::Mihomo,
    chimera_config::application::ClashCore::MihomoAlpha,
    chimera_config::application::ClashCore::ChimeraClient,
    chimera_config::application::ClashCore::ChimeraClientAlpha,
];
const ALL_CORES: &[chimera_config::application::ClashCore] = &[
    chimera_config::application::ClashCore::ClashPremium,
    chimera_config::application::ClashCore::ClashRs,
    chimera_config::application::ClashCore::Mihomo,
    chimera_config::application::ClashCore::MihomoAlpha,
    chimera_config::application::ClashCore::ClashRsAlpha,
    chimera_config::application::ClashCore::ChimeraClient,
    chimera_config::application::ClashCore::ChimeraClientAlpha,
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

/// Legacy TUN derivation delegated to the shared ref-style builder.
pub fn derive_tun_flavor(
    core: chimera_config::application::ClashCore,
    stack: TunStack,
) -> TunFlavor {
    chimera_core::client::runtime_build::derive_tun_flavor(core, stack)
}

/// Tauri shell: chooses bundled scripts, delegates the pure build to Core.
pub struct RuntimeBuilder;

impl RuntimeBuilder {
    pub fn validate_transforms(artifact: &RuntimeArtifact) -> Result<(), RuntimeBuildError> {
        chimera_core::client::runtime_build::RuntimeBuilder::validate_transforms(artifact)
    }

    pub fn build(
        input: &RuntimeBuildInput,
        content: &dyn ProfileContentSource,
        scripts: &dyn ScriptRunner,
    ) -> Result<RuntimeArtifact, RuntimeBuildError> {
        let builtins = if input.app.enable_builtin_enhanced {
            builtin_transforms_for(input.app.core)
        } else {
            Vec::new()
        };
        chimera_core::client::runtime_build::RuntimeBuilder::build(
            input, &builtins, content, scripts,
        )
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
    strict_transforms: bool,
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

    let builtin_transforms = if input.app.enable_builtin_enhanced {
        builtin_transforms_for(input.app.core)
    } else {
        Vec::new()
    };
    // Ref runtime_build.rs: host ports are injected once, not discovered
    // behind a build. The script runner is created inside spawn_blocking.
    let builder = chimera_core::client::runtime_build::RuntimeBuildService::new(
        Arc::new(FsProfileContentSource::new(profiles_dir)),
        Arc::new(|| Ok(Box::new(EnhanceScriptRunner::new()?))),
    );
    let data = builder
        .build(input, staged_content, builtin_transforms, strict_transforms)
        .await?;
    Ok((
        data.config,
        data.exists_keys,
        data.postprocessing_output,
        data.inspection,
    ))
}

fn map_core(core: LegacyClashCore) -> chimera_config::application::ClashCore {
    match core {
        LegacyClashCore::ClashPremium => chimera_config::application::ClashCore::ClashPremium,
        LegacyClashCore::ClashRs => chimera_config::application::ClashCore::ClashRs,
        LegacyClashCore::Mihomo => chimera_config::application::ClashCore::Mihomo,
        LegacyClashCore::MihomoAlpha => chimera_config::application::ClashCore::MihomoAlpha,
        LegacyClashCore::ClashRsAlpha => chimera_config::application::ClashCore::ClashRsAlpha,
        LegacyClashCore::ChimeraClient => chimera_config::application::ClashCore::ChimeraClient,
        LegacyClashCore::ChimeraClientAlpha => {
            chimera_config::application::ClashCore::ChimeraClientAlpha
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enhance::{EnhanceScriptRunner, FsProfileContentSource};
    #[cfg(target_os = "linux")]
    use chimera_config::clash::config::TransparentProxyMode;
    use chimera_config::{
        clash::config::overrides::ManageableField,
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
            ScriptRuntime, ScriptTransform, TransformDefinition,
        },
        runtime::{
            executor::{PortError, StepLogEntry, TransformFailure},
            snapshot::OperatorTag,
            value::ConfigValue,
        },
    };
    use serde_json::json;

    struct EmptyContent;

    impl ProfileContentSource for EmptyContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            Err(format!("no content for {path}").into())
        }
    }

    struct StaticContent(String);

    impl ProfileContentSource for StaticContent {
        fn read(&self, _path: &ManagedProfilePath) -> Result<String, PortError> {
            Ok(self.0.clone())
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
    async fn shared_runtime_service_respects_staged_content_without_mutating_files() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("active.yaml");
        let disk = "proxies: []\nmode: direct\nextra-key: disk\n";
        std::fs::write(&path, disk).unwrap();

        let mut profiles = Profiles::default();
        profiles.append_item(ProfileItem {
            uid: ProfileId("active".into()),
            metadata: ProfileMetadata {
                name: "Active".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("active.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        });
        profiles.set_current(Some(ProfileId("active".into())));
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
        // A second candidate verifies that the runner cannot see source
        // changes made after the capture phase on the same blocking thread.
        let after_input = RuntimeBuildInput {
            profiles: Arc::clone(&input.profiles),
            clash: input.clash.clone(),
            app: input.app.clone(),
            resolved_ports: input.resolved_ports.clone(),
        };
        let staged = "proxies: []\nmode: global\nextra-key: staged\n".to_string();
        let data = chimera_core::client::runtime_build::RuntimeBuildService::build_with_capture(
            input,
            FsProfileContentSource::new(directory.path().to_path_buf()),
            BTreeMap::from([("active.yaml".into(), staged)]),
            Vec::new(),
            || Ok(Box::new(EchoRunner)),
            true,
        )
        .await
        .unwrap();
        assert_eq!(
            data.config
                .get(serde_yaml::Value::String("extra-key".into())),
            Some(&serde_yaml::Value::String("staged".into()))
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), disk);
        assert!(data.inspection.final_node < data.inspection.graph.nodes.len() as u32);

        let path_to_remove = path.clone();
        // Exercise the new injected service that the production Tauri builder
        // also uses, rather than only the earlier static compatibility helper.
        let builder = chimera_core::client::runtime_build::RuntimeBuildService::new(
            Arc::new(FsProfileContentSource::new(directory.path().to_path_buf())),
            Arc::new(move || {
                // This closure is called on the blocking build thread, after
                // the managed file has been captured into FrozenProfileContent.
                std::fs::remove_file(&path_to_remove)?;
                Ok(Box::new(EchoRunner))
            }),
        );
        let frozen = builder
            .capture_content(Arc::clone(&after_input.profiles), BTreeMap::new())
            .await
            .unwrap();
        assert_eq!(
            frozen
                .read(&ManagedProfilePath::new("active.yaml").unwrap())
                .unwrap(),
            disk,
        );
        let second = builder
            .build(after_input, BTreeMap::new(), Vec::new(), true)
            .await
            .unwrap();
        assert_eq!(
            second
                .config
                .get(serde_yaml::Value::String("extra-key".into())),
            Some(&serde_yaml::Value::String("disk".into()))
        );
        assert!(!path.exists());
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
        let expected = vec!["verge_hy_alpn", "verge_meta_guard", "config_fixer"];
        assert_eq!(
            names(chimera_config::application::ClashCore::ChimeraClient),
            expected
        );
        assert_eq!(
            names(chimera_config::application::ClashCore::ChimeraClientAlpha),
            expected
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
    #[cfg(target_os = "linux")]
    fn transparent_capture_requires_service_mode_and_builds_managed_listener() {
        let mut input = RuntimeBuildInput {
            profiles: Arc::new(Profiles::default()),
            clash: ClashConfig::default(),
            app: ChimeraAppConfig::default(),
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        input.clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert!(matches!(
            RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner),
            Err(RuntimeBuildError::TransparentProxyRequiresServiceMode)
        ));

        input.app.enable_service_mode = true;
        input.app.core = chimera_config::application::ClashCore::Mihomo;
        input.clash.tproxy_port = Some(
            chimera_config::clash::config::clash_strategy::PortStrategy::new_allow_fallback(7894),
        );
        input.resolved_ports.tproxy_port = Some(7894);
        let artifact = RuntimeBuilder::build(&input, &EmptyContent, &EchoRunner).unwrap();
        let config = artifact.final_config.to_json();
        assert_eq!(config["tproxy-port"], json!(7894));
        assert_eq!(config["routing-mark"], json!(0x20000));
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
    fn mihomo_runtime_builder_drops_chimera_client_mmdb_extension() {
        let mut profiles = Profiles::default();
        profiles.current = Some(ProfileId("profile".into()));
        profiles.append_item(ProfileItem {
            uid: ProfileId("profile".into()),
            metadata: ProfileMetadata {
                name: "Profile".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("profile.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        });

        let input = RuntimeBuildInput {
            profiles: Arc::new(profiles),
            clash: ClashConfig {
                enable_clash_fields: true,
                ..ClashConfig::default()
            },
            app: ChimeraAppConfig {
                core: chimera_config::application::ClashCore::Mihomo,
                enable_builtin_enhanced: false,
                ..ChimeraAppConfig::default()
            },
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        let artifact = RuntimeBuilder::build(
            &input,
            &StaticContent("mmdb: Country.mmdb\nproxies: []\n".into()),
            &EchoRunner,
        )
        .unwrap();

        assert!(
            artifact.final_config.to_json().get("mmdb").is_none(),
            "stock Mihomo must not receive Chimera Client's top-level MMDB extension"
        );

        let (runtime_config, _, _, _) = artifact_to_legacy_output_with_inspection(
            artifact,
            &input.profiles,
            input.app.core,
            input.app.enable_builtin_enhanced,
        )
        .unwrap();
        let runtime_yaml = serde_yaml::to_string(&runtime_config).unwrap();
        assert!(
            !runtime_yaml.contains("mmdb:"),
            "the stock Mihomo runtime YAML must not contain Chimera Client's MMDB field: {runtime_yaml}"
        );
    }

    #[test]
    fn chimera_client_runtime_preserves_explicit_mmdb_in_runtime_yaml() {
        let mut profiles = Profiles::default();
        profiles.current = Some(ProfileId("profile".into()));
        profiles.append_item(ProfileItem {
            uid: ProfileId("profile".into()),
            metadata: ProfileMetadata {
                name: "Profile".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("profile.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        });

        let input = RuntimeBuildInput {
            profiles: Arc::new(profiles),
            clash: ClashConfig {
                enable_clash_fields: true,
                ..ClashConfig::default()
            },
            app: ChimeraAppConfig {
                core: chimera_config::application::ClashCore::ChimeraClient,
                enable_builtin_enhanced: false,
                ..ChimeraAppConfig::default()
            },
            resolved_ports: ResolvedPortBindings {
                mixed_port: 7890,
                ..Default::default()
            },
        };
        let artifact = RuntimeBuilder::build(
            &input,
            &StaticContent("mmdb: Country.mmdb\nproxies: []\n".into()),
            &EchoRunner,
        )
        .unwrap();

        assert_eq!(
            artifact.final_config.to_json()["mmdb"],
            json!("Country.mmdb"),
            "Chimera Client must retain its supported top-level MMDB path"
        );

        let (runtime_config, _, _, _) = artifact_to_legacy_output_with_inspection(
            artifact,
            &input.profiles,
            input.app.core,
            input.app.enable_builtin_enhanced,
        )
        .unwrap();
        let runtime_yaml = serde_yaml::to_string(&runtime_config).unwrap();
        assert!(
            runtime_yaml.contains("mmdb: Country.mmdb"),
            "the runtime YAML must retain Chimera Client's MMDB path: {runtime_yaml}"
        );
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
    fn runtime_builder_threads_tun_auto_route_ownership_into_executor() {
        for (ownership, expected) in [
            (ManageableField::Managed(false), json!(false)),
            (ManageableField::Unmanaged, json!("global")),
        ] {
            let mut profiles = Profiles::default();
            profiles.current = Some(ProfileId("profile".into()));
            profiles.append_item(ProfileItem {
                uid: ProfileId("profile".into()),
                metadata: ProfileMetadata {
                    name: "Profile".into(),
                    desc: None,
                    custom_name: true,
                },
                definition: ProfileDefinition::Config {
                    config: ConfigDefinition::File(FileConfig {
                        source: ProfileSource::Local {
                            binding: LocalBinding::Managed {
                                materialized: MaterializedFile {
                                    file: ManagedProfilePath::new("profile.yaml").unwrap(),
                                    updated_at: None,
                                },
                            },
                        },
                        transforms: Vec::new(),
                    }),
                },
            });

            let input = RuntimeBuildInput {
                profiles: Arc::new(profiles),
                clash: ClashConfig {
                    enable_tun_mode: true,
                    enable_clash_fields: true,
                    tun: chimera_config::clash::config::overrides::TunOverrides {
                        auto_route: ownership,
                    },
                    ..ClashConfig::default()
                },
                app: ChimeraAppConfig {
                    core: chimera_config::application::ClashCore::Mihomo,
                    enable_builtin_enhanced: false,
                    ..ChimeraAppConfig::default()
                },
                resolved_ports: ResolvedPortBindings {
                    mixed_port: 7890,
                    ..Default::default()
                },
            };
            let artifact = RuntimeBuilder::build(
                &input,
                &StaticContent("proxies: []\ntun: { auto-route: global }\n".into()),
                &EchoRunner,
            )
            .unwrap();

            assert_eq!(
                artifact.final_config.to_json()["tun"]["auto-route"],
                expected
            );
        }
    }

    #[test]
    fn runtime_builder_threads_include_all_toggle_to_executor() {
        let mut profiles = Profiles::default();
        profiles.current = Some(ProfileId("profile".into()));
        profiles.append_item(ProfileItem {
            uid: ProfileId("profile".into()),
            metadata: ProfileMetadata {
                name: "Profile".into(),
                desc: None,
                custom_name: true,
            },
            definition: ProfileDefinition::Config {
                config: ConfigDefinition::File(FileConfig {
                    source: ProfileSource::Local {
                        binding: LocalBinding::Managed {
                            materialized: MaterializedFile {
                                file: ManagedProfilePath::new("profile.yaml").unwrap(),
                                updated_at: None,
                            },
                        },
                    },
                    transforms: Vec::new(),
                }),
            },
        });

        for enabled in [true, false] {
            let input = RuntimeBuildInput {
                profiles: Arc::new(profiles.clone()),
                clash: ClashConfig {
                    enable_clash_fields: false,
                    expand_include_all: enabled,
                    ..ClashConfig::default()
                },
                app: ChimeraAppConfig {
                    core: chimera_config::application::ClashCore::Mihomo,
                    enable_builtin_enhanced: false,
                    ..ChimeraAppConfig::default()
                },
                resolved_ports: ResolvedPortBindings {
                    mixed_port: 7890,
                    ..Default::default()
                },
            };
            let artifact = RuntimeBuilder::build(
                &input,
                &StaticContent(
                    "proxies:
  - name: z-proxy
  - name: a-proxy
proxy-groups:
  - name: G
    type: select
    include-all: true
    proxies: [DIRECT]
"
                    .into(),
                ),
                &EchoRunner,
            )
            .unwrap();
            let config = artifact.final_config.to_json();
            let group = &config["proxy-groups"][0];
            if enabled {
                assert_eq!(group["proxies"], json!(["DIRECT", "a-proxy", "z-proxy"]));
                assert!(group.get("include-all").is_none());
            } else {
                assert_eq!(group["proxies"], json!(["DIRECT"]));
                assert_eq!(group["include-all"], json!(true));
            }
            assert_eq!(
                artifact.graph.nodes.iter().any(|node| matches!(
                    node.tag,
                    chimera_config::runtime::snapshot::OperatorTag::BuiltinStep {
                        step:
                            chimera_config::runtime::snapshot::BuiltinStepKind::IncludeAllExpansion,
                        ..
                    }
                )),
                enabled,
                "the persisted toggle must control whether the expansion stage runs"
            );
        }
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

        std::fs::write(
            temp.path().join("scr1.js"),
            "function main() { throw new Error('candidate script failed'); }\n",
        )
        .unwrap();
        let failed = RuntimeBuilder::build(&input, &content, &scripts).unwrap();
        assert_eq!(
            failed.transform_failures,
            vec![TransformFailure::Profile { id: "scr1".into() }],
            "candidate artifact: {failed:#?}"
        );

        let error = RuntimeBuilder::validate_transforms(&failed).unwrap_err();
        let RuntimeBuildError::TransformsFailed { failures, logs } = error else {
            panic!("expected failed candidate validation");
        };
        assert_eq!(
            failures,
            vec![TransformFailure::Profile { id: "scr1".into() }]
        );
        assert!(logs.iter().any(|log| {
            matches!(
                &log.tag,
                OperatorTag::ScopedTransform {
                    host_profile_id,
                    transform_profile_id,
                    ..
                } if host_profile_id.0 == "cfg1" && transform_profile_id.0 == "scr1"
            ) && log
                .entries
                .iter()
                .any(|entry| entry.message.contains("candidate script failed"))
        }));
    }
}
