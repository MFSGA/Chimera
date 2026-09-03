use anyhow::{Context, Result, bail};
use chimera_config::clash::config::ClashConfig;
use futures_util::future::join_all;
use indexmap::IndexMap;
use serde_yaml::Mapping;

use crate::{
    config::{
        core::Config,
        profile::item::{Profile, ProfileMetaGetter},
    },
    enhance::{
        field::{HANDLE_FIELDS, use_keys, use_valid_fields, use_whitelist_fields_filter},
        utils::{merge_profiles, process_chain},
    },
};

/// 1
mod chain;
/// 3
mod field;
/// 4
mod script;
/// 5
mod tun;
/// 2
mod utils;

pub use chain::PostProcessingOutput;
pub(crate) use chain::TransformFailureError;

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance(clash: &ClashConfig) -> Result<(Mapping, Vec<String>, PostProcessingOutput)> {
    // config.yaml 的配置
    let clash_config = { Config::clash().latest().0.clone() };

    let (profiles, profile_chain, global_chain, valid) = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();

        let profile_chain_mapping = profiles
            .get_current()
            .iter()
            .map(|uid| {
                let item = profiles
                    .get_item(uid)
                    .with_context(|| format!("selected profile {uid} does not exist"))?;
                let chain = match item {
                    Profile::Local(profile) => {
                        utils::resolve_transform_chain(&profiles, &profile.chain, Some(uid))?
                    }
                    Profile::Remote(profile) => {
                        utils::resolve_transform_chain(&profiles, &profile.chain, Some(uid))?
                    }
                    Profile::Merge(_) | Profile::Script(_) => {
                        bail!("transform profile {uid} cannot be selected as a source profile")
                    }
                };
                Ok((item.uid().to_string(), chain))
            })
            .collect::<Result<IndexMap<_, _>>>()?;

        let current_mappings = profiles
            .current_mappings()
            .context("failed to load selected profile mappings")?
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<IndexMap<_, _>>();

        let global_chain = utils::resolve_transform_chain(&profiles, &profiles.chain, None)
            .context("failed to resolve global transform chain")?;
        let valid = profiles.valid.clone();

        (current_mappings, profile_chain_mapping, global_chain, valid)
    };

    let mut postprocessing_output = PostProcessingOutput::default();
    let valid = use_valid_fields(&valid);

    // Execute per-profile transform chains before combining selected profiles.
    let profiles_outputs = join_all(profiles.into_iter().map(|(uid, mapping)| async {
        let chain = profile_chain.get(&uid).map_or(&[] as &[_], |v| v);
        let output = process_chain(mapping, chain, Some(&uid)).await;
        (uid, output)
    }))
    .await;

    let mut profiles = IndexMap::new();
    for (uid, output) in profiles_outputs {
        let (config, output) =
            output.with_context(|| format!("failed to process transform chain for {uid}"))?;
        postprocessing_output.scopes.insert(uid.clone(), output);
        profiles.insert(uid, config);
    }

    // Preserve the existing multi-profile behavior: use the first full mapping and append
    // proxies from subsequent selected profiles.
    let config = merge_profiles(profiles).context("failed to merge selected profiles")?;

    // Global transforms run after selected profiles have been combined.
    let (mut config, global_chain_output) = process_chain(config, &global_chain, None)
        .await
        .context("failed to process global transform chain")?;
    postprocessing_output.global = global_chain_output;

    // 记录当前配置包含的键
    let exists_keys = use_keys(&config);
    config = use_whitelist_fields_filter(config, &valid, clash.enable_clash_fields);

    // 合并默认的config
    clash_config
        .iter()
        // only guarded fields should be overwritten
        .filter(|(k, _)| HANDLE_FIELDS.contains(&k.as_str().unwrap_or_default()))
        .for_each(|(key, value)| {
            config.insert(key.to_owned(), value.clone());
        });

    config = tun::use_tun(config, clash);

    Ok((config, exists_keys, postprocessing_output))
}
