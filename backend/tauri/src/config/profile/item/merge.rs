use ambassador::Delegate;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::config::profile::{
    item::{
        ProfileKindGetter, ProfileMetaGetter, ambassador_impl_ProfileMetaGetter,
        shared::{ProfileShared, ProfileSharedBuilder},
    },
    item_type::ProfileItemType,
};

const PROFILE_TYPE: ProfileItemType = ProfileItemType::Merge;

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
pub struct MergeProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build(&PROFILE_TYPE).map_err(|e| MergeProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,
}

impl MergeProfile {
    pub fn builder() -> MergeProfileBuilder {
        let mut builder = MergeProfileBuilder::default();
        builder.shared(ProfileShared::get_default_builder(&PROFILE_TYPE));
        builder
    }
}

impl MergeProfileBuilder {
    pub fn assign_managed_identity(&mut self, uid: String) {
        self.shared.assign_managed_identity(&PROFILE_TYPE, uid);
    }
}

impl ProfileKindGetter for MergeProfile {
    fn kind(&self) -> ProfileItemType {
        PROFILE_TYPE
    }
}
