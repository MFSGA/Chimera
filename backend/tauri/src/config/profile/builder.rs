use crate::config::profile::item::{
    local::LocalProfileBuilder, merge::MergeProfileBuilder, remote::RemoteProfileBuilder,
    script::ScriptProfileBuilder,
};
use crate::config::profile::item_type::ProfileItemType;

// todo: add the serde
#[derive(Debug, serde::Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProfileBuilder {
    Remote(RemoteProfileBuilder),
    Local(LocalProfileBuilder),
    Merge(MergeProfileBuilder),
    Script(ScriptProfileBuilder),
}

impl ProfileBuilder {
    pub fn kind(&self) -> ProfileItemType {
        match self {
            Self::Remote(_) => ProfileItemType::Remote,
            Self::Local(_) => ProfileItemType::Local,
            Self::Merge(_) => ProfileItemType::Merge,
            Self::Script(builder) => builder.kind(),
        }
    }

    pub fn assign_managed_identity(&mut self, uid: String) {
        match self {
            Self::Remote(builder) => builder.assign_managed_identity(uid),
            Self::Local(builder) => builder.assign_managed_identity(uid),
            Self::Merge(builder) => builder.assign_managed_identity(uid),
            Self::Script(builder) => builder.assign_managed_identity(uid),
        }
    }
}
