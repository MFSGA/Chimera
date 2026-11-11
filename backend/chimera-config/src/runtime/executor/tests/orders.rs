use serde_json::json;

#[cfg(feature = "default-meta")]
use crate::clash::config::overrides::ClashGuardOverridesPatch;

use super::support::*;
use crate::{
    clash::config::{
        overrides::{ClashGuardOverrides, ManageableField, TunOverrides},
        tun_stack::TunStack,
    },
    profile::{Profiles, ScriptRuntime},
    runtime::{
        executor::{
            BuiltinTransform, ExecutionTarget, GuardInputs, ResolvedPortBindings,
            RuntimePipelineError, RuntimePipelineInputs, StepLogLevel, TunFlavor, TunParams,
            execute,
        },
        snapshot::{BuiltinStepKind, ConfigExecutionRole, OperatorTag},
    },
};

pub fn base_inputs<'a>(
    profiles: &'a Profiles,
    target: ExecutionTarget,
    overrides: &'a crate::clash::config::overrides::ClashGuardOverrides,
    builtin_transforms: &'a [BuiltinTransform],
) -> RuntimePipelineInputs<'a> {
    RuntimePipelineInputs {
        profiles,
        target,
        guard: GuardInputs {
            overrides,
            ports: ResolvedPortBindings {
                mixed_port: 7890,
                port: None,
                socks_port: None,
                redir_port: None,
                tproxy_port: None,
                external_controller: None,
            },
            routing_mark: None,
        },
        whitelist_enabled: true,
        core_supports_mmdb: false,
        tun: TunParams {
            enable: false,
            overrides: TunOverrides::default(),
            flavor: TunFlavor::Standard {
                stack: TunStack::Gvisor,
            },
            windows_fake_ip_filter: false,
        },
        builtin_transforms,
    }
}

fn overrides() -> ClashGuardOverrides {
    super::builtin::fixed_overrides()
}

#[cfg(feature = "default-meta")]
fn overrides_with_patch(yaml: &str) -> ClashGuardOverrides {
    use struct_patch::Patch;

    let mut overrides = overrides();
    let patch: ClashGuardOverridesPatch =
        serde_yaml_ng::from_str(yaml).expect("guard override patch must deserialize");
    overrides.apply(patch);
    overrides
}

#[test]
fn selected_file_config_records_canonical_mainline() {
    // §7.1：root → scoped → global → filter → guard/TUN inheritance and settings → builtin → finalizing
    let profiles = profiles_with(
        Some("sub-a"),
        &["global-fix"],
        &["dns"],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &["normalize"]),
            overlay_item("normalize", "normalize.yaml"),
            overlay_item("global-fix", "global-fix.yaml"),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        (
            "sub-a.yaml",
            "proxies:\n  - name: a1\nrules: [r1]\ndns: { enable: true }\n",
        ),
        ("normalize.yaml", "append__rules: [r-scoped]\n"),
        ("global-fix.yaml", "append__rules: [r-global]\n"),
    ]);
    let ov = overrides();
    let builtins = vec![BuiltinTransform {
        name: "config_fixer".to_string(),
        runtime: ScriptRuntime::JavaScript,
        source: "builtin-src".to_string(),
    }];
    let inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("sub-a")),
        &ov,
        &builtins,
    );

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let tags: Vec<_> = artifact.graph.nodes.iter().map(|node| &node.tag).collect();
    assert!(matches!(
        tags[0],
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::Selected,
            ..
        }
    ));
    assert!(matches!(
        tags[1],
        OperatorTag::ScopedTransform { step_index: 0, .. }
    ));
    assert!(matches!(
        tags[2],
        OperatorTag::GlobalTransform { step_index: 0, .. }
    ));
    assert!(matches!(
        tags[3],
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::WhitelistFieldFilter,
            ..
        }
    ));
    for (offset, kind) in [
        BuiltinStepKind::KeepUnmanaged,
        BuiltinStepKind::ManagedSettings,
        BuiltinStepKind::TunKeepUnmanaged,
        BuiltinStepKind::TunManagedSettings,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            tags[4 + offset],
            OperatorTag::BuiltinStep { step, .. } if *step == kind
        ));
    }
    assert!(matches!(
        tags[8],
        OperatorTag::BuiltinTransform { step_index: 0, .. }
    ));
    for (index, kind) in [
        BuiltinStepKind::FinalFieldFilter,
        BuiltinStepKind::Defaults,
        BuiltinStepKind::Finalizing,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(tags[9 + index], OperatorTag::BuiltinStep { step, .. } if *step == kind));
    }
    assert_eq!(artifact.graph.nodes.len(), 12);

    // 语义断言：scoped + global 追加都生效，guard 覆盖 mode。
    let config = artifact.final_config.to_json();
    assert_eq!(config["rules"], json!(["r1", "r-scoped", "r-global"]));
    assert_eq!(config["mode"], json!("rule"));
    assert_eq!(config["mixed-port"], json!(7890));

    // applied_fields：全局变换后采样(proxies/rules/dns)，确定序。
    let applied: Vec<&str> = artifact.applied_fields.iter().map(String::as_str).collect();
    assert_eq!(applied, vec!["proxies", "rules", "dns"]);
}

#[cfg(feature = "default-meta")]
#[test]
fn unmanaged_guard_fields_keep_profile_values_after_field_filtering() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "proxies: []\nunified-delay: false\ntcp-concurrent: false\n",
    )]);
    let ov = overrides_with_patch(
        "unified-delay: { kind: unmanaged }\ntcp-concurrent: { kind: unmanaged }\n",
    );
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    assert_eq!(config["unified-delay"], json!(false));
    assert_eq!(config["tcp-concurrent"], json!(false));
}

#[cfg(feature = "default-meta")]
#[test]
fn unmanaged_guard_fields_do_not_create_missing_profile_values() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n")]);
    let ov = overrides_with_patch(
        "unified-delay: { kind: unmanaged }\ntcp-concurrent: { kind: unmanaged }\n",
    );
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    assert!(config.get("unified-delay").is_none());
    assert!(config.get("tcp-concurrent").is_none());
}

#[cfg(feature = "default-meta")]
#[test]
fn managed_guard_fields_override_profile_values() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "proxies: []\nunified-delay: false\ntcp-concurrent: false\n",
    )]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    assert_eq!(config["unified-delay"], json!(true));
    assert_eq!(config["tcp-concurrent"], json!(true));
}

#[cfg(feature = "default-meta")]
#[test]
fn staged_ownership_matches_legacy_builtin_input_exactly() {
    use crate::runtime::{executor::builtin, value::ConfigValue};

    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "proxies: []
unified-delay: false
tun: { auto-route: false, stack: system }
",
    )]);
    let ov = overrides_with_patch(
        "unified-delay: { kind: unmanaged }
",
    );
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    inputs.tun.overrides.auto_route = ManageableField::Unmanaged;
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let root = &artifact.graph.nodes[0].snapshot.config;
    let filtered = artifact
        .graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::WhitelistFieldFilter,
                    ..
                }
            )
        })
        .unwrap();
    let managed_tun = artifact
        .graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::TunManagedSettings,
                    ..
                }
            )
        })
        .unwrap();
    let root = ConfigValue::from_json_value(root.clone());
    let filtered = ConfigValue::from_json_value(filtered.snapshot.config.clone());
    let original_guard = builtin::apply_guard_with_source(&filtered, &root, &inputs.guard).unwrap();
    let original = builtin::apply_tun_overrides_with_source(&original_guard, &root, &inputs.tun);
    assert_eq!(
        serde_json::to_string(&original.to_json()).unwrap(),
        serde_json::to_string(&managed_tun.snapshot.config).unwrap(),
        "staging must preserve even field insertion order seen by builtin transforms"
    );
}

#[test]
fn defaults_are_inspectable_without_changing_final_yaml() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &["tun", "dns"],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "proxies: []
proxy-groups: [{ name: Group, type: select, include-all: true }]
extra: ignored
",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let find = |kind| {
        artifact
            .graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.tag, OperatorTag::BuiltinStep { step, .. } if step == kind
                )
            })
            .unwrap()
    };
    let filtered = find(BuiltinStepKind::FinalFieldFilter);
    let defaults = find(BuiltinStepKind::Defaults);
    let finalizer = find(BuiltinStepKind::Finalizing);

    assert!(filtered.snapshot.config.get("extra").is_none());
    assert!(defaults.snapshot.config["dns"]["enable"].as_bool().unwrap());
    assert!(defaults.snapshot.config["tun"]["enable"].as_bool().unwrap());
    assert_eq!(
        defaults.snapshot.config["profile"]["store-selected"],
        json!(true)
    );
    assert_eq!(
        serde_json::to_string(&finalizer.snapshot.config).unwrap(),
        serde_json::to_string(&artifact.final_config.to_json()).unwrap(),
    );
    assert_eq!(
        serde_json::to_string(&artifact.final_config.to_json()).unwrap(),
        serde_json::to_string(
            &super::super::builtin::finalize_for_core(
                &crate::runtime::value::ConfigValue::from_json_value(
                    find(BuiltinStepKind::TunManagedSettings)
                        .snapshot
                        .config
                        .clone()
                ),
                &inputs.tun,
                inputs.whitelist_enabled,
                inputs.core_supports_mmdb,
            )
            .to_json()
        )
        .unwrap(),
        "split steps must preserve the finalizing output and key order"
    );
}

#[test]
fn composition_with_base_matches_recording_contract() {
    // §7.3 + spec §8.1：base/成员分支独立嫁接，extend 主线推进。
    let profiles = profiles_with(
        Some("all"),
        &[],
        &[],
        vec![
            config_file_item("base", "base.yaml", &[]),
            config_file_item("member", "member.yaml", &[]),
            composition_item("all", Some("base"), &["member"], &[]),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        ("base.yaml", "proxies:\n  - name: b1\nrules: [r1]\n"),
        ("member.yaml", "proxies:\n  - name: m1\nextra: dropped\n"),
    ]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("all")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();

    // 11 条规则：只提取 proxies、按序追加、不合并其他字段、不去重。
    let names: Vec<&str> = config["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["b1", "m1"]);
    assert_eq!(config["rules"], json!(["r1"]));
    assert!(config.get("extra").is_none());

    // 图形状（对齐 snapshot.rs composition 测试期望形态）。
    let nodes = &artifact.graph.nodes;
    let root = artifact.graph.root_id as usize;
    assert!(matches!(
        &nodes[root].tag,
        OperatorTag::CompositionRoot { base: Some(_), .. }
    ));
    let root_children = nodes[root].next.as_deref().unwrap();
    assert_eq!(root_children.len(), 3); // base 分支根、member 分支根、extend 步
    let base_branch = root_children[0] as usize;
    assert!(matches!(
        &nodes[base_branch].tag,
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::CompositionBase { .. },
            ..
        }
    ));
    let member_branch = root_children[1] as usize;
    assert!(matches!(
        &nodes[member_branch].tag,
        OperatorTag::FileConfigRoot {
            role: ConfigExecutionRole::CompositionContributor { .. },
            ..
        }
    ));
    let extend = root_children[2] as usize;
    assert!(matches!(
        &nodes[extend].tag,
        OperatorTag::ExtendProxiesStep {
            contributor_index: 0,
            ..
        }
    ));
}

#[test]
fn composition_without_base_starts_from_clean_seed() {
    // §7.4：clean seed = {proxies: []}，不隐式注入其他字段。
    let profiles = profiles_with(
        Some("clean"),
        &[],
        &[],
        vec![
            config_file_item("member", "member.yaml", &[]),
            composition_item("clean", None, &["member"], &[]),
        ],
    );
    let content = MapContentSource::from_pairs(&[("member.yaml", "proxies:\n  - name: m1\n")]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("clean")), &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let root = artifact.graph.root_id as usize;
    assert!(matches!(
        &artifact.graph.nodes[root].tag,
        OperatorTag::CompositionRoot { base: None, .. }
    ));
    assert_eq!(
        artifact.graph.nodes[root].snapshot.config,
        json!({ "proxies": [] })
    );
    let final_config = artifact.final_config.to_json();
    let names: Vec<&str> = final_config["proxies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["m1"]);
}

#[test]
fn bare_target_reuses_shared_tail_with_none_selected() {
    // §7.1 场景⑤：BareRoot → global(选中=None) → 尾部四步；产出可用裸配置。
    let profiles = profiles_with(
        None,
        &["global-fix"],
        &[],
        vec![overlay_item("global-fix", "g.yaml")],
    );
    let content = MapContentSource::from_pairs(&[("g.yaml", "injected: { a: 1 }\n")]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Bare, &ov, &[]);

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    let tags: Vec<_> = artifact.graph.nodes.iter().map(|node| &node.tag).collect();
    assert!(matches!(tags[0], OperatorTag::BareRoot));
    assert!(matches!(
        tags[1],
        OperatorTag::GlobalTransform {
            selected_profile_id: None,
            ..
        }
    ));
    assert!(matches!(
        tags[2],
        OperatorTag::BuiltinStep {
            selected_profile_id: None,
            step: BuiltinStepKind::WhitelistFieldFilter
        }
    ));

    let config = artifact.final_config.to_json();
    assert_eq!(config["mode"], json!("rule")); // guard 生效
    assert_eq!(config["mixed-port"], json!(7890));
    assert!(config.get("injected").is_none()); // whitelist 过滤了非法注入
    assert_eq!(config["profile"]["store-selected"], json!(true)); // cache 生效
}

#[test]
fn script_transform_failure_is_lenient_with_anchored_error_log() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &["boom"]),
            script_item("boom", "boom.js", ScriptRuntime::JavaScript),
        ],
    );
    let content =
        MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n"), ("boom.js", "explode")]);
    let mut runner = FakeScriptRunner::default();
    runner.runs.insert(
        "explode".to_string(),
        RunReply::Fail("script exploded".to_string(), vec![]),
    );
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let artifact = execute(&inputs, &content, &runner).unwrap();

    // 透传：scoped 节点值 == root 值。
    assert_eq!(
        artifact.graph.nodes[1].snapshot.config,
        artifact.graph.nodes[0].snapshot.config
    );
    // error 日志锚定在该 ScopedTransform 节点键上。
    let scoped_key = artifact.graph.nodes[1].key.clone();
    let log = artifact
        .step_logs
        .iter()
        .find(|log| log.key == scoped_key)
        .unwrap();
    assert!(
        log.entries
            .iter()
            .any(|entry| entry.level == StepLogLevel::Error)
    );
    assert_eq!(
        artifact.transform_failures,
        vec![crate::runtime::executor::TransformFailure::Profile {
            id: "boom".to_string(),
        }]
    );
}

#[test]
fn missing_selected_content_is_strict_error() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "missing.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let error = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap_err();
    assert!(matches!(error, RuntimePipelineError::ContentSource { .. }));
}

#[test]
fn tun_auto_route_ownership_survives_profile_field_filtering() {
    for (override_field, expected) in [
        (ManageableField::Managed(false), json!(false)),
        (ManageableField::Managed(true), json!(true)),
        (ManageableField::Unmanaged, json!("global")),
    ] {
        let profiles = profiles_with(
            Some("sub-a"),
            &[],
            &[],
            vec![config_file_item("sub-a", "sub-a.yaml", &[])],
        );
        let content = MapContentSource::from_pairs(&[(
            "sub-a.yaml",
            "proxies: []\ntun: { auto-route: global, mtu: 1400 }\n",
        )]);
        let ov = overrides();
        let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
        inputs.tun.enable = true;
        inputs.tun.overrides = TunOverrides {
            auto_route: override_field,
        };

        let config = execute(&inputs, &content, &FakeScriptRunner::default())
            .unwrap()
            .final_config
            .to_json();
        assert_eq!(config["tun"]["auto-route"], expected);
        assert_eq!(config["tun"]["enable"], json!(true));
        assert!(config["tun"].get("mtu").is_none());
    }
}

#[test]
fn unmanaged_auto_route_does_not_create_a_missing_profile_field() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content =
        MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\ntun: { mtu: 1400 }\n")]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    inputs.tun.overrides = TunOverrides {
        auto_route: ManageableField::Unmanaged,
    };

    let config = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert!(config["tun"].get("auto-route").is_none());
    assert_eq!(config["tun"]["enable"], json!(true));
}

#[test]
fn mmdb_path_survives_enabled_field_filter_without_being_rewritten() {
    for mmdb in ["Country.mmdb", "/home/user/chimera/geo/Country.mmdb"] {
        let profiles = profiles_with(
            Some("sub-a"),
            &[],
            &[],
            vec![config_file_item("sub-a", "sub-a.yaml", &[])],
        );
        let yaml = format!("mmdb: {mmdb}\nproxies: []\n");
        let content = MapContentSource::from_pairs(&[("sub-a.yaml", &yaml)]);
        let ov = overrides();
        let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
        inputs.core_supports_mmdb = true;

        // base_inputs enables the same two-pass field filter used by the app.
        let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
        assert_eq!(
            artifact.final_config.to_json()["mmdb"],
            json!(mmdb),
            "explicit MMDB path must survive unchanged"
        );
    }
}

#[test]
fn unsupported_core_drops_top_level_mmdb_even_when_filtering_is_disabled() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content =
        MapContentSource::from_pairs(&[("sub-a.yaml", "mmdb: Country.mmdb\nproxies: []\n")]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;

    let config = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert!(config.get("mmdb").is_none());
}

#[test]
fn absent_mmdb_is_not_synthesized_by_field_filtering() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n")]);
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    let config = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert!(config.get("mmdb").is_none());
}

#[test]
fn applied_fields_keep_prefilter_known_keys_and_drop_unknown_even_unfiltered() {
    // D9：采样点在全局变换后；按所选内核支持的字段集合取交集
    // （enable_filter=false 时 config 保留未知键但 applied_fields 不列）。
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "mode: from-profile\ncustom-unknown: 1\nproxies: []\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();

    // mode ∈ HANDLE ⊂ 45 → 计入(即使 stage-1 会滤掉它——采样在过滤前)；
    // custom-unknown ∉ 45 → 不计入，但 config 里保留(whitelist 关闭)。
    assert!(artifact.applied_fields.contains("mode"));
    assert!(artifact.applied_fields.contains("proxies"));
    assert!(!artifact.applied_fields.contains("custom-unknown"));
    assert_eq!(artifact.final_config.to_json()["custom-unknown"], json!(1));
}
