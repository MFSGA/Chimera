use ambassador::Delegate;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::profile::{
    item::{
        ProfileKindGetter, ProfileMetaGetter, ProfileMetaSetter, ambassador_impl_ProfileMetaGetter,
        ambassador_impl_ProfileMetaSetter,
        shared::{ProfileShared, ProfileSharedBuilder},
    },
    item_type::{ProfileItemType, ProfileUid},
};

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Local;

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
#[delegate(ProfileMetaSetter, target = "shared")]
pub struct LocalProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build(&PROFILE_TYPE).map_err(|e| LocalProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(setter(strip_option), default)]
    pub symlinks: Option<PathBuf>,
    #[builder(default)]
    #[serde(alias = "chains", default)]
    #[builder_field_attr(serde(alias = "chains", default))]
    pub chain: Vec<ProfileUid>,
}

impl LocalProfile {
    pub fn builder() -> LocalProfileBuilder {
        let mut builder = LocalProfileBuilder::default();
        builder.shared(ProfileShared::get_default_builder(&PROFILE_TYPE));
        builder
    }
}

impl ProfileKindGetter for LocalProfile {
    fn kind(&self) -> ProfileItemType {
        PROFILE_TYPE
    }
}
