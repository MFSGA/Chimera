//! Tauri adapter for shared RuntimeArtifact -> runtime snapshot mapping.
//! Builtin script names remain bound to the Tauri runtime builder for now.

use crate::enhance::{PostProcessingOutput, runtime_builder::builtin_transforms_for};
use anyhow::Result;
use chimera_config::{
    application::ClashCore, profile::Profiles, runtime::executor::RuntimeArtifact,
};
use chimera_core::client::{
    artifact_snapshot::artifact_to_snapshot_data, runtime_inspection::RuntimeInspectionData,
};

pub(crate) fn artifact_to_legacy_output_with_inspection(
    artifact: RuntimeArtifact,
    profiles: &Profiles,
    core: ClashCore,
    builtin_enabled: bool,
) -> Result<(
    serde_yaml::Mapping,
    Vec<String>,
    PostProcessingOutput,
    RuntimeInspectionData,
)> {
    let builtin_names = if builtin_enabled {
        builtin_transforms_for(core)
            .into_iter()
            .map(|transform| transform.name)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let data = artifact_to_snapshot_data(artifact, profiles, &builtin_names)?;
    Ok((
        data.config,
        data.exists_keys,
        data.postprocessing_output,
        data.inspection,
    ))
}
