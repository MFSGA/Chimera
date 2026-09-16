//! Mapping from the ref RuntimeArtifact to Chimera's existing runtime output.

use anyhow::{Context, Result};
use chimera_config::{
    application::ClashCore,
    profile::{ConfigDefinition, ProfileDefinition, ProfileId, Profiles},
    runtime::{
        executor::{RuntimeArtifact, StepLog, StepLogLevel},
        snapshot::SnapshotNodeKey,
    },
};

use crate::{
    client::runtime_inspection::RuntimeInspectionData,
    enhance::{
        PostProcessingOutput,
        chain::{LogSpan, Logs},
        runtime_builder::builtin_transforms_for,
    },
};

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

pub(crate) fn map_postprocessing(
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
            _ => {}
        }
    }
    output
}

pub(crate) fn artifact_to_legacy_output(
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> Result<(serde_yaml::Mapping, PostProcessingOutput)> {
    let (mapping, output, _) =
        artifact_to_legacy_output_with_inspection(artifact, profiles, core, builtin_enabled)?;
    Ok((mapping, output))
}

pub(crate) fn artifact_to_legacy_output_with_inspection(
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> Result<(
    serde_yaml::Mapping,
    PostProcessingOutput,
    RuntimeInspectionData,
)> {
    let RuntimeArtifact {
        final_config,
        graph,
        step_logs,
        ..
    } = artifact;
    let value = serde_yaml::to_value(final_config.to_json())
        .context("failed to serialize final runtime config")?;
    let mapping = value
        .as_mapping()
        .cloned()
        .context("final runtime config is not a mapping")?;
    let builtin_names = if builtin_enabled {
        builtin_transforms_for(core)
            .into_iter()
            .map(|item| item.name)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let output = map_postprocessing(&step_logs, profiles, &builtin_names);
    Ok((mapping, output, RuntimeInspectionData { graph, step_logs }))
}
