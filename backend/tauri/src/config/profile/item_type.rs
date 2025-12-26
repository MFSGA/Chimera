use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "variant", rename_all = "snake_case")]
pub enum ProfileItemType {
    #[serde(rename = "remote")]
    Remote,
    #[serde(rename = "local")]
    #[default]
    Local,
}

pub type ProfileUid = String;
