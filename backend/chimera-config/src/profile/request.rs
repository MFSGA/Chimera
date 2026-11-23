//! Inputs for creating a profile in its owning profiles document.

use super::{FolderId, ProfileDefinition, ProfileMetadata};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct NewProfileRequest {
    pub metadata: ProfileMetadata,
    /// Add rewrites the materialized path to `{uid}.{ext}`.
    pub definition: ProfileDefinition,
    /// Folder the new Config profile joins in the same commit.
    #[serde(default)]
    #[specta(optional)] // Chimera legacy IPC accepts requests without this field.
    pub folder: Option<FolderId>,
}
