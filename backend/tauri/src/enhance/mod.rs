use futures_util::future::join_all;
use indexmap::IndexMap;
use serde_yaml::Mapping;

use crate::{
    config::{core::Config, profile::item::ProfileMetaGetter},
    enhance::{
        chain::PostProcessingOutput,
        field::{use_keys, use_valid_fields},
        utils::{merge_profiles, process_chain},
    },
};

/// 1
mod chain;
/// 3
mod field;
/// 4
mod script;
/// 2
mod utils;

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> (Mapping, Vec<String>, PostProcessingOutput) {
    // config.yaml 的配置
    let clash_config = { Config::clash().latest().0.clone() };

    let (clash_core, enable_tun, enable_builtin, enable_filter) = {
        let verge = Config::verge();
        let verge = verge.latest();
        (
            verge.clash_core,
            verge.enable_tun_mode.unwrap_or(false),
            // todo: will changed to true in the future
            verge.enable_builtin_enhanced.unwrap_or(false),
            verge.enable_clash_fields.unwrap_or(true),
        )
    };

    // 从profiles里拿东西
    let (profiles, profile_chain, global_chain, valid) = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();

        let profile_chain_mapping = profiles
            .get_current()
            .iter()
            .filter_map(|uid| profiles.get_item(uid).ok())
            .map(|item| {
                (
                    item.uid().to_string(),
                    match item {
                        // todo: local
                        /* profile if profile.is_local() => {
                            let profile = profile.as_local().unwrap();
                            utils::convert_uids_to_scripts(&profiles, &profile.chain)
                        } */
                        profile if profile.is_remote() => {
                            let profile = profile.as_remote().unwrap();
                            utils::convert_uids_to_scripts(&profiles, &profile.chain)
                        }
                        _ => vec![],
                    },
                )
            })
            .collect::<IndexMap<_, _>>();

        let current_mappings = profiles
            .current_mappings()
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<IndexMap<_, _>>();

        let global_chain = utils::convert_uids_to_scripts(&profiles, &profiles.chain);

        let valid = profiles.valid.clone();

        (current_mappings, profile_chain_mapping, global_chain, valid)
    };

    let mut postprocessing_output = PostProcessingOutput::default();

    let valid = use_valid_fields(&valid);

    // 执行 scoped chain
    let profiles_outputs = join_all(profiles.into_iter().map(|(uid, mapping)| async {
        let chain = profile_chain.get(&uid).map_or(&[] as &[_], |v| v);
        let output = process_chain(mapping, chain).await;
        (uid, output)
    }))
    .await;

    let mut profiles = IndexMap::new();
    for (uid, (config, output)) in profiles_outputs {
        postprocessing_output.scopes.insert(uid.to_string(), output);
        profiles.insert(uid.to_string(), config);
    }

    // 合并多个配置
    // TODO: 此步骤需要提供针对每个配置的 Meta 信息
    // TODO: 需要支持自定义合并逻辑
    let config = merge_profiles(profiles);

    // 记录当前配置包含的键
    let mut exists_keys = use_keys(&config);

    (config, exists_keys, postprocessing_output)
}
