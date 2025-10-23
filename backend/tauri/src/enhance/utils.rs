use std::borrow::Borrow;

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

/// 合并多个配置
// TODO: 可能移动到其他地方
// TODO: 增加自定义合并逻辑
// TODO: 添加元信息
pub fn merge_profiles<T: Borrow<String>>(mappings: IndexMap<T, Mapping>) -> Mapping {
    mappings
        .into_iter()
        .enumerate()
        .fold(Mapping::new(), |mut acc, (idx, (_key, value))| {
            // full extend the first one, others just extend proxies
            // TODO: custom merge logic
            // TODO: add meta info
            if idx == 0 {
                acc.extend(value);
            } else {
                let proxies = value.get("proxies").unwrap().as_sequence().unwrap().clone();
                let acc_proxies = acc.get_mut("proxies").unwrap().as_sequence_mut().unwrap();
                acc_proxies.extend(proxies);
            }
            acc
        })
}
