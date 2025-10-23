use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;

use crate::{
    config::profile::{item_type::ProfileUid, profiles::Profiles},
    enhance::{chain::ChainItem, script::runner::RunnerManager},
};

pub fn convert_uids_to_scripts(profiles: &Profiles, uids: &[ProfileUid]) -> Vec<ChainItem> {
    uids.iter()
        .filter_map(|uid| profiles.get_item(uid).ok())
        .filter_map(<Option<ChainItem>>::from)
        .collect::<Vec<ChainItem>>()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum LogSpan {
    Log,
    Info,
    Warn,
    Error,
}

pub type Logs = Vec<(LogSpan, String)>;

/// 处理链
pub async fn process_chain(
    mut config: Mapping,
    nodes: &[ChainItem],
) -> (Mapping, IndexMap<ProfileUid, Logs>) {
    let mut result_map = IndexMap::new();

    let mut script_runner = RunnerManager::new();
    log::debug!("todo: impl script_runner");
    (config, result_map)
}
