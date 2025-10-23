use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::config::profile::item::{Profile, ProfileMetaGetter};

#[derive(Default, Debug, Clone, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {}

#[derive(Debug, Clone)]
pub enum ChainTypeWrapper {
    Merge(Mapping),
    // Script(ScriptWrapper),
}

#[derive(Debug, Clone)]
pub struct ChainItem {
    pub uid: String,
    // pub data: ChainTypeWrapper,
}

impl TryFrom<&Profile> for ChainItem {
    type Error = anyhow::Error;

    fn try_from(item: &Profile) -> Result<Self, Self::Error> {
        let uid = item.uid().to_string();
        // let data = ChainTypeWrapper::try_from(item)?;
        Ok(Self { uid })
    }
}
