use crate::{
    config::profile::{
        item::{Profile, ProfileMetaGetter},
        item_type::ProfileUid,
    },
    enhance::utils::Logs,
};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Default, Debug, Clone, Serialize, Deserialize, specta::Type)]
/// 后处理输出
pub struct PostProcessingOutput {
    /// 1. 局部链的输出
    pub scopes: IndexMap<ProfileUid, IndexMap<ProfileUid, Logs>>,
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

impl From<&Profile> for Option<ChainItem> {
    fn from(item: &Profile) -> Self {
        let uid = item.uid().to_string();
        // let data = ChainTypeWrapper::try_from(item);
        Some(ChainItem { uid })
        /* match data {
            Err(_) => None,
            Ok(data) => Some(ChainItem { uid }),
        } */
    }
}
