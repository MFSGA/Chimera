use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type)]
#[builder(
    derive(Debug, serde::Serialize, serde::Deserialize, specta::Type),
    build_fn(skip)
)]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    /// Profile ID
    pub uid: String,
}
