use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

#[derive(Default, Debug, Clone)]
pub struct IRuntime {
    pub config: Option<Mapping>,
}

impl IRuntime {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct PatchRuntimeConfig {
    #[serde(default)]
    pub mode: Option<String>,
}
