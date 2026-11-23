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
            BuiltinTransform, DnsParams, ExecutionTarget, GuardInputs, ResolvedPortBindings,
            RuntimePipelineError, RuntimePipelineInputs, ScriptExecutionError, ScriptOperation,
            StepLogLevel, TunFlavor, TunParams, TunStackWrite, execute,
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
                stack: TunStackWrite::Selected(TunStack::Gvisor),
                congestion_controller: None,
            },
            windows_fake_ip_filter: false,
        },
        dns: DnsParams::default(),
        expand_include_all: true,
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
    // Ref §7.1: scoped → global → filter → inherited → managed → defaults → builtin.
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
    let graph = artifact.graph.materialize().unwrap();

    let tags: Vec<_> = graph.nodes.iter().map(|node| &node.tag).collect();
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
    for (index, kind) in [
        BuiltinStepKind::KeepUnmanaged,
        BuiltinStepKind::ManagedSettings,
        BuiltinStepKind::Defaults,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(
            tags[4 + index],
            OperatorTag::BuiltinStep { step, .. } if *step == kind
        ));
    }
    assert!(matches!(
        tags[7],
        OperatorTag::BuiltinTransform { step_index: 0, .. }
    ));
    for (index, kind) in [
        BuiltinStepKind::FinalFieldFilter,
        BuiltinStepKind::IncludeAllExpansion,
        BuiltinStepKind::Finalizing,
    ]
    .into_iter()
    .enumerate()
    {
        assert!(matches!(tags[8 + index], OperatorTag::BuiltinStep { step, .. } if *step == kind));
    }
    assert_eq!(graph.nodes.len(), 11);
    assert_eq!(artifact.final_node, 10);

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
fn unified_ownership_restores_unmanaged_fields_before_managed_and_defaults() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "proxies: []\nunified-delay: false\ntun: { auto-route: false, stack: system }\n",
    )]);
    let ov = overrides_with_patch("unified-delay: { kind: unmanaged }\n");
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    inputs.tun.overrides.auto_route = ManageableField::Unmanaged;

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let graph = artifact.graph.materialize().unwrap();
    let at =
        |kind| {
            graph.nodes.iter().find(|node| {
        matches!(node.tag, OperatorTag::BuiltinStep { step, .. } if step == kind)
    }).unwrap()
        };
    let filtered = at(BuiltinStepKind::WhitelistFieldFilter);
    let inherited = at(BuiltinStepKind::KeepUnmanaged);
    let managed = at(BuiltinStepKind::ManagedSettings);
    let defaults = at(BuiltinStepKind::Defaults);

    assert!(filtered.snapshot.config.get("tun").is_none());
    assert_eq!(inherited.snapshot.config["tun"]["auto-route"], json!(false));
    assert_eq!(inherited.snapshot.config["unified-delay"], json!(false));
    assert_eq!(managed.snapshot.config["tun"]["enable"], json!(true));
    assert_eq!(managed.snapshot.config["tun"]["stack"], json!("gvisor"));
    assert_eq!(managed.snapshot.config["dns"]["enable"], json!(true));
    assert_eq!(defaults.snapshot.config["tun"]["auto-route"], json!(false));
    assert_eq!(
        defaults.snapshot.config["dns"]["enhanced-mode"],
        json!("fake-ip")
    );
    assert_eq!(
        defaults.snapshot.config["profile"]["store-selected"],
        json!(true)
    );

    let inherited_id = graph
        .nodes
        .iter()
        .position(|node| node.tag.node_key() == inherited.key)
        .unwrap();
    let managed_id = graph
        .nodes
        .iter()
        .position(|node| node.tag.node_key() == managed.key)
        .unwrap();
    let defaults_id = graph
        .nodes
        .iter()
        .position(|node| node.tag.node_key() == defaults.key)
        .unwrap();
    assert!(inherited_id < managed_id && managed_id < defaults_id);
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
    let graph = artifact.graph.materialize().unwrap();
    let find = |kind| {
        graph
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
    let expansion = find(BuiltinStepKind::IncludeAllExpansion);
    let finalizer = find(BuiltinStepKind::Finalizing);

    assert!(filtered.snapshot.config.get("extra").is_none());
    assert!(defaults.snapshot.config["dns"]["enable"].as_bool().unwrap());
    assert!(defaults.snapshot.config["tun"]["enable"].as_bool().unwrap());
    assert_eq!(
        defaults.snapshot.config["profile"]["store-selected"],
        json!(true)
    );
    assert!(
        defaults.snapshot.config["proxy-groups"][0]
            .get("include-all")
            .is_some()
    );
    assert!(
        expansion.snapshot.config["proxy-groups"][0]
            .get("include-all")
            .is_none()
    );
    assert_eq!(
        serde_json::to_string(&finalizer.snapshot.config).unwrap(),
        serde_json::to_string(&artifact.final_config.to_json()).unwrap(),
    );
    let tags: Vec<_> = graph.nodes.iter().map(|node| &node.tag).collect();
    let default_position = tags
        .iter()
        .position(|tag| {
            matches!(
                tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::Defaults,
                    ..
                }
            )
        })
        .unwrap();
    let final_filter_position = tags
        .iter()
        .position(|tag| {
            matches!(
                tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::FinalFieldFilter,
                    ..
                }
            )
        })
        .unwrap();
    assert!(
        default_position < final_filter_position,
        "defaults must run before the final core filter"
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
    let graph = artifact.graph.materialize().unwrap();
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
    let nodes = &graph.nodes;
    let root = graph.root_id as usize;
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
    let graph = artifact.graph.materialize().unwrap();

    let root = graph.root_id as usize;
    assert!(matches!(
        &graph.nodes[root].tag,
        OperatorTag::CompositionRoot { base: None, .. }
    ));
    assert_eq!(graph.nodes[root].snapshot.config, json!({ "proxies": [] }));
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
    let graph = artifact.graph.materialize().unwrap();

    let tags: Vec<_> = graph.nodes.iter().map(|node| &node.tag).collect();
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
    let graph = artifact.graph.materialize().unwrap();

    // 透传：scoped 节点值 == root 值。
    assert_eq!(
        graph.nodes[1].snapshot.config,
        graph.nodes[0].snapshot.config
    );
    // error 日志锚定在该 ScopedTransform 节点键上。
    let scoped_key = graph.nodes[1].key.clone();
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

// Unit contract: a selected file with a JavaScript transform and an unavailable
// engine must reject construction. The former lenient implementation returned
// an artifact, silently leaving the transform unexecuted.
#[test]
fn missing_transform_engine_rejects_runtime_instead_of_keeping_the_old_config() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![
            config_file_item("sub-a", "sub-a.yaml", &["script"]),
            script_item("script", "script.js", ScriptRuntime::JavaScript),
        ],
    );
    let content = MapContentSource::from_pairs(&[
        ("sub-a.yaml", "proxies: []\n"),
        ("script.js", "script-source"),
    ]);
    let mut runner = FakeScriptRunner::default();
    runner.runs.insert(
        "script-source".to_string(),
        RunReply::MissingEngine(ScriptOperation::Transform),
    );
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);

    assert!(matches!(
        execute(&inputs, &content, &runner),
        Err(RuntimePipelineError::ExecuteScript {
            source: ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::JavaScript,
                operation: ScriptOperation::Transform
            }
        })
    ));
}

// Unit contract: an unavailable engine also stops the built-in transform path.
// This guards against swallowing the error after the user profile has built.
#[test]
fn missing_builtin_engine_rejects_runtime_instead_of_logging_and_continuing() {
    let profiles = Profiles::default();
    let content = MapContentSource::from_pairs(&[]);
    let mut runner = FakeScriptRunner::default();
    runner.runs.insert(
        "builtin-source".to_string(),
        RunReply::MissingEngine(ScriptOperation::Transform),
    );
    let builtins = [BuiltinTransform {
        name: "builtin".to_string(),
        runtime: ScriptRuntime::JavaScript,
        source: "builtin-source".to_string(),
    }];
    let ov = overrides();
    let inputs = base_inputs(&profiles, ExecutionTarget::Bare, &ov, &builtins);

    assert!(matches!(
        execute(&inputs, &content, &runner),
        Err(RuntimePipelineError::ExecuteScript {
            source: ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::JavaScript,
                operation: ScriptOperation::Transform
            }
        })
    ));
}

// Unit contract: the executor publishes a compact, parent-relative graph,
// not an expanded copy. Every node must restore byte-equivalent ordered JSON
// and the last main-line node must be the configuration actually built.
// This would fail when direct ownership patches replay with reordered keys.
#[test]
fn runtime_pipeline_stored_graph_restores_every_node_and_its_final_config() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &["tun", "dns"],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "mode: global\nunknown: removed\ntun: { stack: system }\nproxies: []\nrules: [MATCH,DIRECT]\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    crate::runtime::snapshot::assert_restore_matches_materialize(&artifact.graph);

    let final_node = &artifact.graph.nodes[artifact.final_node as usize];
    assert!(matches!(
        final_node.tag,
        OperatorTag::BuiltinStep {
            step: BuiltinStepKind::Finalizing,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_string(&artifact.graph.restore(artifact.final_node).unwrap()).unwrap(),
        serde_json::to_string(&artifact.final_config.to_json()).unwrap(),
        "the stored final node must be the exact ordered generated config"
    );

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
    assert!(matches!(
        filtered.snapshot.payload,
        crate::runtime::snapshot::SnapshotPayload::Delta(_)
    ));
    assert!(
        filtered
            .changed_fields
            .as_ref()
            .unwrap()
            .contains("unknown")
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

// Contract: the built-in script sees the postprocessed Defaults node as
// input and is not silently overwritten by a second ownership pass. This
// detects the previous Chimera order even if final YAML happens to parse.
#[test]
fn builtin_transform_runs_after_unified_defaults_without_late_rewrites() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &["tun", "dns"],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[("sub-a.yaml", "proxies: []\n")]);
    let builtins = [BuiltinTransform {
        name: "builtin-override".to_owned(),
        runtime: ScriptRuntime::JavaScript,
        source: "override-source".to_owned(),
    }];
    let mut runner = FakeScriptRunner::default();
    runner.runs.insert(
        "override-source".to_owned(),
        RunReply::Replace(
            json!({
                "mode": "direct",
                "mixed-port": 8123,
                "tun": {"enable": false},
                "dns": {"enable": false},
                "profile": {"store-selected": false},
                "proxies": []
            }),
            vec![],
        ),
    );
    let ov = overrides();
    let mut inputs = base_inputs(
        &profiles,
        ExecutionTarget::Selected(pid("sub-a")),
        &ov,
        &builtins,
    );
    inputs.tun.enable = true;
    let artifact = execute(&inputs, &content, &runner).unwrap();
    let graph = artifact.graph.materialize().unwrap();
    let node = |kind| {
        graph
            .nodes
            .iter()
            .position(|node| {
                matches!(
                    node.tag, OperatorTag::BuiltinStep { step, .. } if step == kind
                )
            })
            .unwrap()
    };
    let defaults_id = node(BuiltinStepKind::Defaults);
    let builtin_id = graph
        .nodes
        .iter()
        .position(|node| matches!(node.tag, OperatorTag::BuiltinTransform { .. }))
        .unwrap();
    assert_eq!(builtin_id, defaults_id + 1);
    assert_eq!(
        graph.nodes[defaults_id].snapshot.config["tun"]["enable"],
        json!(true)
    );
    assert_eq!(
        graph.nodes[defaults_id].snapshot.config["dns"]["enable"],
        json!(true)
    );
    assert_eq!(
        graph.nodes[defaults_id].snapshot.config["mixed-port"],
        json!(7890)
    );

    let config = artifact.final_config.to_json();
    assert_eq!(config["mixed-port"], json!(8123));
    assert_eq!(config["tun"]["enable"], json!(false));
    assert_eq!(config["dns"]["enable"], json!(false));
    assert_eq!(config["profile"]["store-selected"], json!(false));
    assert_eq!(
        serde_json::to_string(&artifact.graph.restore(artifact.final_node).unwrap()).unwrap(),
        serde_json::to_string(&config).unwrap()
    );
}

/// Ref #5678: unfiltered subscriptions cannot reopen disabled listeners.
/// When MIPS has no managed congestion controller, remove a profile's
/// controller rather than leaving an option that the core would read.
#[test]
fn unset_mips_controller_removes_profile_value_and_keeps_selected_mips_stack() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &["tun"],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "tun: { enable: true, stack: gvisor, congestion-controller: bbr3 }\nproxies: []\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.tun.enable = true;
    inputs.tun.flavor = TunFlavor::Standard {
        stack: TunStackWrite::Selected(TunStack::Mips),
        congestion_controller: None,
    };
    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let config = artifact.final_config.to_json();
    assert_eq!(config["tun"]["stack"], "mips");
    assert_eq!(config["tun"]["enable"], true);
    assert!(config["tun"].get("congestion-controller").is_none());

    inputs.tun.flavor = TunFlavor::Standard {
        stack: TunStackWrite::Selected(TunStack::Mips),
        congestion_controller: Some(crate::clash::config::tun_stack::TunCongestionController::Reno),
    };
    let configured = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert_eq!(configured["tun"]["congestion-controller"], "reno");
}

#[test]
fn managed_dns_and_hosts_replace_profile_tables_in_shared_settings_step() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "dns:\n  enable: false\n  nameserver-policy:\n    geosite:cn: https://profile.example/dns-query\nhosts:\n  old.test: 192.0.2.1\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;
    inputs.dns = DnsParams {
        dns: Some(crate::runtime::value::ConfigValue::from_json_value(json!({
            "enable": false,
            "nameserver": ["https://1.1.1.1/dns-query"],
        }))),
        hosts: Some(crate::runtime::value::ConfigValue::from_json_value(json!({
            "new.test": "192.0.2.2",
        }))),
        warnings: vec!["selected core omits dns.prefer-h3".to_owned()],
    };

    let artifact = execute(&inputs, &content, &FakeScriptRunner::default()).unwrap();
    let graph = artifact.graph.materialize().unwrap();
    let managed = graph
        .nodes
        .iter()
        .find(|node| {
            matches!(
                node.tag,
                OperatorTag::BuiltinStep {
                    step: BuiltinStepKind::ManagedSettings,
                    ..
                }
            )
        })
        .unwrap();
    let config = &managed.snapshot.config;
    assert_eq!(config["dns"]["enable"], json!(false));
    assert_eq!(
        config["dns"]["nameserver"],
        json!(["https://1.1.1.1/dns-query"])
    );
    assert!(config["dns"].get("nameserver-policy").is_none());
    assert_eq!(config["hosts"], json!({"new.test": "192.0.2.2"}));

    let logs = artifact
        .step_logs
        .iter()
        .find(|logs| logs.key == managed.key)
        .unwrap();
    assert!(logs.entries.iter().any(|entry| {
        entry.level == StepLogLevel::Warn && entry.message.contains("dns.prefer-h3")
    }));
    assert!(logs.entries.iter().any(|entry| {
        entry.level == StepLogLevel::Warn && entry.message.contains("nameserver-policy")
    }));
}

#[test]
fn unfiltered_profile_cannot_open_managed_proxy_ports() {
    let profiles = profiles_with(
        Some("sub-a"),
        &[],
        &[],
        vec![config_file_item("sub-a", "sub-a.yaml", &[])],
    );
    let content = MapContentSource::from_pairs(&[(
        "sub-a.yaml",
        "port: 7890\nsocks-port: 7891\ncustom-unknown: 1\nproxies: []\n",
    )]);
    let ov = overrides();
    let mut inputs = base_inputs(&profiles, ExecutionTarget::Selected(pid("sub-a")), &ov, &[]);
    inputs.whitelist_enabled = false;

    let unset = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert!(unset.get("port").is_none());
    assert!(unset.get("socks-port").is_none());
    assert_eq!(unset["mixed-port"], json!(7890));
    assert_eq!(unset["custom-unknown"], json!(1));

    inputs.guard.ports.port = Some(7892);
    inputs.guard.ports.socks_port = Some(7893);
    let configured = execute(&inputs, &content, &FakeScriptRunner::default())
        .unwrap()
        .final_config
        .to_json();
    assert_eq!(configured["port"], json!(7892));
    assert_eq!(configured["socks-port"], json!(7893));
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
