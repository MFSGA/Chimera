use serde::{Deserialize, Serialize};

pub use chimera_core::client::runtime::ScriptType;

#[derive(Serialize, Deserialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "kind", content = "variant", rename_all = "snake_case")]
pub enum ProfileItemType {
    #[serde(rename = "remote")]
    Remote,
    #[serde(rename = "local")]
    #[default]
    Local,
    #[serde(rename = "merge")]
    Merge,
    #[serde(rename = "script")]
    Script(ScriptType),
}

impl ProfileItemType {
    pub fn is_config(self) -> bool {
        matches!(self, Self::Remote | Self::Local)
    }

    pub fn is_transform(self) -> bool {
        matches!(self, Self::Merge | Self::Script(_))
    }

    pub fn is_runtime_transform_supported(self) -> bool {
        self.is_transform()
    }
}

pub type ProfileUid = String;
