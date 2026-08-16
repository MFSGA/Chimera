use ambassador::Delegate;
use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::config::profile::{
    item::{
        ProfileKindGetter, ProfileMetaGetter, ambassador_impl_ProfileMetaGetter,
        shared::{ProfileShared, ProfileSharedBuilder},
    },
    item_type::{ProfileItemType, ScriptType},
};

#[derive(
    Default, Delegate, Debug, Clone, Deserialize, Serialize, Builder, BuilderUpdate, specta::Type,
)]
#[builder(derive(Debug, Serialize, Deserialize, specta::Type))]
#[builder_update(patch_fn = "apply")]
#[delegate(ProfileMetaGetter, target = "shared")]
pub struct ScriptProfile {
    #[serde(flatten)]
    #[builder(field(
        ty = "ProfileSharedBuilder",
        build = "self.shared.build(&ProfileItemType::Script(self.script_type.unwrap_or_default())).map_err(|e| ScriptProfileBuilderError::from(e.to_string()))?"
    ))]
    #[builder_field_attr(serde(flatten))]
    #[builder_update(nested)]
    pub shared: ProfileShared,

    #[serde(default)]
    #[builder(default)]
    pub script_type: ScriptType,
}

impl ScriptProfile {
    pub fn builder(script_type: ScriptType) -> ScriptProfileBuilder {
        let kind = ProfileItemType::Script(script_type);
        let mut builder = ScriptProfileBuilder::default();
        builder
            .shared(ProfileShared::get_default_builder(&kind))
            .script_type(script_type);
        builder
    }
}

impl ScriptProfileBuilder {
    pub fn assign_managed_identity(&mut self, uid: String) {
        let kind = ProfileItemType::Script(self.script_type.unwrap_or_default());
        self.shared.assign_managed_identity(&kind, uid);
    }

    pub fn kind(&self) -> ProfileItemType {
        ProfileItemType::Script(self.script_type.unwrap_or_default())
    }
}

impl ProfileKindGetter for ScriptProfile {
    fn kind(&self) -> ProfileItemType {
        ProfileItemType::Script(self.script_type)
    }
}
