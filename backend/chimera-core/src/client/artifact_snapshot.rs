//! Pure RuntimeArtifact mapping following ref client/artifact_snapshot.rs.
//! The runtime artifact and shared inspection retain the reference
//! keyframe/delta graph; legacy UI output stays a separate projection.

use anyhow::{Context, Result};
use chimera_config::{
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::{
        executor::{RuntimeArtifact, StepLog, StepLogLevel},
        snapshot::SnapshotNodeKey,
    },
};

use super::runtime_inspection::RuntimeInspectionData;

pub use chimera_profile_transform::{LogSpan, Logs, PostProcessingOutput};

fn span(level: StepLogLevel) -> LogSpan {
    match level {
        StepLogLevel::Log => LogSpan::Log,
        StepLogLevel::Info => LogSpan::Info,
        StepLogLevel::Warn => LogSpan::Warn,
        StepLogLevel::Error => LogSpan::Error,
    }
}

fn transform_uid_of(profiles: &Profiles, host: &ProfileId, step_index: u32) -> Option<String> {
    let item = profiles.items.get(host)?;
    let list = match &item.definition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(file),
        } => &file.transforms,
        ProfileDefinition::Config {
            config: ConfigDefinition::Composition(composition),
        } => &composition.transforms,
        _ => return None,
    };
    list.get(step_index as usize).map(|uid| uid.0.clone())
}

pub fn map_postprocessing(
    step_logs: &[StepLog],
    profiles: &Profiles,
    builtin_names: &[String],
) -> PostProcessingOutput {
    let mut output = PostProcessingOutput::default();
    for log in step_logs {
        let logs: Logs = log
            .entries
            .iter()
            .map(|entry| (span(entry.level), entry.message.clone()))
            .collect();
        if logs.is_empty() {
            continue;
        }
        match &log.key {
            SnapshotNodeKey::ScopedTransform {
                host_profile_id,
                step_index,
                ..
            } => {
                let transform = transform_uid_of(profiles, host_profile_id, *step_index)
                    .unwrap_or_else(|| format!("step-{step_index}"));
                output
                    .scopes
                    .entry(host_profile_id.0.clone())
                    .or_default()
                    .insert(transform, logs);
            }
            SnapshotNodeKey::GlobalTransform { step_index, .. } => {
                let uid = profiles
                    .global_transforms
                    .get(*step_index as usize)
                    .map(|uid| uid.0.clone())
                    .unwrap_or_else(|| format!("global-{step_index}"));
                output.global.insert(uid, logs);
            }
            SnapshotNodeKey::BuiltinTransform { step_index, .. } => {
                let name = builtin_names
                    .get(*step_index as usize)
                    .cloned()
                    .unwrap_or_else(|| format!("builtin-{step_index}"));
                output.global.insert(name, logs);
            }
            _ => output.advice.extend(logs),
        }
    }
    output
}

/// The validated runtime build's legacy projection, without any I/O or
/// dependency on Tauri's transform/script execution layer.
#[derive(Debug, Clone)]
pub struct ArtifactSnapshotData {
    pub config: serde_yaml::Mapping,
    pub exists_keys: Vec<String>,
    pub postprocessing_output: PostProcessingOutput,
    pub inspection: RuntimeInspectionData,
}

/// Convert a built executor artifact to the promoted snapshot data model.
/// `builtin_names` come from the caller's core-specific builtin table.
pub fn artifact_to_snapshot_data(
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    builtin_names: &[String],
) -> Result<ArtifactSnapshotData> {
    let RuntimeArtifact {
        final_config,
        graph,
        final_node,
        step_logs,
        applied_fields,
        ..
    } = artifact;
    let value = serde_yaml::to_value(final_config.to_json())
        .context("failed to serialize final runtime config")?;
    let mapping = value
        .as_mapping()
        .cloned()
        .context("final runtime config is not a mapping")?;
    anyhow::ensure!(
        matches!(
            graph.nodes.get(final_node as usize).map(|node| &node.tag),
            Some(
                chimera_config::runtime::snapshot::OperatorTag::BuiltinStep {
                    step: chimera_config::runtime::snapshot::BuiltinStepKind::Finalizing,
                    ..
                }
            )
        ),
        "runtime inspection graph has no finalizing step"
    );
    // Keep ref's compact graph in the shared inspection model.
    // Validate the promoted final node against the generated configuration.
    let restored = graph
        .restore(final_node)
        .context("runtime inspection final config cannot be restored")?;
    anyhow::ensure!(
        restored == final_config.to_json(),
        "runtime inspection final node differs from the generated config"
    );
    let output = map_postprocessing(&step_logs, profiles, builtin_names);
    let exists_keys = applied_fields.into_iter().collect();
    Ok(ArtifactSnapshotData {
        config: mapping,
        exists_keys,
        postprocessing_output: output,
        inspection: RuntimeInspectionData {
            final_node,
            graph,
            step_logs,
        },
    })
}

/// Ref's candidate-to-snapshot boundary. One canonical serialized product;
/// retain the Mapping only for existing legacy config publication.
pub fn runtime_snapshot_data_from_artifact(
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    core: chimera_config::application::ClashCore,
    builtin_enabled: bool,
) -> std::result::Result<
    super::runtime::RuntimeSnapshotData,
    chimera_profile_transform::RuntimeBuildError,
> {
    use chimera_profile_transform::{RuntimeBuildError, builtin_transforms_for};
    let builtin_names: Vec<String> = if builtin_enabled {
        builtin_transforms_for(core)
            .into_iter()
            .map(|builtin| builtin.name)
            .collect()
    } else {
        Vec::new()
    };
    let data = artifact_to_snapshot_data(artifact, profiles, &builtin_names)
        .map_err(RuntimeBuildError::ProjectArtifact)?;
    let bytes = super::runtime::render_runtime_bytes(&data.config)
        .map_err(RuntimeBuildError::SerializeFinalConfig)?;
    // serde_yaml produces UTF-8; the header is also UTF-8. Reuse the
    // exact serialized product as the one canonical text allocation.
    let config_text = String::from_utf8(bytes).expect("serialized YAML is UTF-8");
    Ok(super::runtime::RuntimeSnapshotData {
        config_text: config_text.into(),
        config: data.config,
        exists_keys: data.exists_keys,
        postprocessing_output: data.postprocessing_output,
        inspection: std::sync::Arc::new(data.inspection),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chimera_config::runtime::{
        executor::{StepLog, StepLogEntry},
        snapshot::{BuiltinStepKind, ConfigExecutionRole, ConfigSnapshotsBuilder, OperatorTag},
        value::ConfigValue,
    };
    use indexmap::IndexSet;

    use super::*;

    fn log(key: SnapshotNodeKey, level: StepLogLevel, message: &str) -> StepLog {
        StepLog {
            key,
            entries: vec![StepLogEntry::new(level, message)],
        }
    }

    #[test]
    fn ref_log_keys_keep_chimera_legacy_scope_global_and_builtin_layout() {
        let profiles = Profiles::default();
        let host = ProfileId("source".into());
        let logs = vec![
            log(
                SnapshotNodeKey::ScopedTransform {
                    host_profile_id: host.clone(),
                    role: ConfigExecutionRole::Selected,
                    step_index: 2,
                },
                StepLogLevel::Warn,
                "source transform",
            ),
            log(
                SnapshotNodeKey::GlobalTransform {
                    selected_profile_id: Some(host.clone()),
                    step_index: 1,
                },
                StepLogLevel::Error,
                "global transform",
            ),
            log(
                SnapshotNodeKey::BuiltinTransform {
                    selected_profile_id: Some(host),
                    step_index: 0,
                },
                StepLogLevel::Info,
                "built-in transform",
            ),
            log(
                SnapshotNodeKey::BareRoot,
                StepLogLevel::Log,
                "nontransform diagnostic",
            ),
        ];
        let output = map_postprocessing(&logs, &profiles, &["config_fixer".to_owned()]);
        assert_eq!(
            output.scopes["source"]["step-2"],
            [(LogSpan::Warn, "source transform".into())]
        );
        assert_eq!(
            output.global["global-1"],
            [(LogSpan::Error, "global transform".into())]
        );
        assert_eq!(
            output.global["config_fixer"],
            [(LogSpan::Info, "built-in transform".into())]
        );
        assert_eq!(output.global.len(), 2);
        assert_eq!(output.scopes.len(), 1);
        assert_eq!(
            output.advice,
            [(LogSpan::Log, "nontransform diagnostic".into())]
        );
        let wire = serde_json::to_value(&output).unwrap();
        assert_eq!(wire["advice"][0][1], "nontransform diagnostic");
        // Old persisted output can still be decoded.
        let legacy: PostProcessingOutput =
            serde_json::from_value(serde_json::json!({"scopes": {}, "global": {}})).unwrap();
        assert!(legacy.advice.is_empty());
    }

    #[test]
    fn artifact_mapping_keeps_finalizing_node_and_applied_field_order() {
        let root = Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "rule"})).unwrap());
        let final_config =
            Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "global"})).unwrap());
        let mut builder = ConfigSnapshotsBuilder::new_root(root, OperatorTag::BareRoot);
        builder
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                final_config.clone(),
            )
            .unwrap();
        let (graph, final_node) = builder.build_stored_with_current().unwrap();
        let fields: IndexSet<String> = ["mixed-port", "tun", "dns"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let data = artifact_to_snapshot_data(
            RuntimeArtifact {
                final_config,
                graph,
                final_node,
                step_logs: Vec::new(),
                applied_fields: fields,
                transform_failures: Vec::new(),
            },
            &Profiles::default(),
            &[],
        )
        .unwrap();
        assert_eq!(
            data.config.get(serde_yaml::Value::String("mode".into())),
            Some(&serde_yaml::Value::String("global".into()))
        );
        assert_eq!(data.exists_keys, ["mixed-port", "tun", "dns"]);
        assert_eq!(data.inspection.final_node, 1);
        assert!(data.postprocessing_output.scopes.is_empty());
        assert!(data.postprocessing_output.global.is_empty());
    }

    #[test]
    fn a_finalizing_node_mismatched_with_runtime_yaml_cannot_be_published() {
        let root = Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "rule"})).unwrap());
        let selected =
            Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "global"})).unwrap());
        let mut builder = ConfigSnapshotsBuilder::new_root(root, OperatorTag::BareRoot);
        builder
            .push(
                OperatorTag::BuiltinStep {
                    selected_profile_id: None,
                    step: BuiltinStepKind::Finalizing,
                },
                selected,
            )
            .unwrap();
        let (graph, final_node) = builder.build_stored_with_current().unwrap();
        let incorrect_final =
            Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "direct"})).unwrap());
        let error = artifact_to_snapshot_data(
            RuntimeArtifact {
                final_config: incorrect_final,
                graph,
                final_node,
                step_logs: Vec::new(),
                applied_fields: IndexSet::new(),
                transform_failures: Vec::new(),
            },
            &Profiles::default(),
            &[],
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("differs from the generated config")
        );
    }

    #[test]
    fn an_artifact_without_finalizing_node_fails_instead_of_publishing() {
        let config = Arc::new(ConfigValue::try_from(serde_json::json!({"mode": "rule"})).unwrap());
        let (graph, final_node) =
            ConfigSnapshotsBuilder::new_root(config.clone(), OperatorTag::BareRoot)
                .build_stored_with_current()
                .unwrap();
        let error = artifact_to_snapshot_data(
            RuntimeArtifact {
                final_config: config,
                graph,
                final_node,
                step_logs: Vec::new(),
                applied_fields: IndexSet::new(),
                transform_failures: Vec::new(),
            },
            &Profiles::default(),
            &[],
        )
        .unwrap_err();
        assert!(error.to_string().contains("no finalizing step"));
    }
}
