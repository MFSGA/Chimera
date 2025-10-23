use indexmap::IndexMap;
use serde_yaml::Mapping;

use crate::{
    config::{core::Config, profile::item::ProfileMetaGetter},
    enhance::{chain::PostProcessingOutput, field::use_valid_fields},
};

/// 1
mod chain;
/// 3
mod field;
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

    todo!()
}
