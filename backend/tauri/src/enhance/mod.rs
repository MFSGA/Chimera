use futures_util::future::join_all;
use indexmap::IndexMap;
use serde_yaml::Mapping;

use crate::{
    config::{
        core::Config,
        profile::item::{Profile, ProfileMetaGetter},
    },
    enhance::{
        chain::PostProcessingOutput,
        field::{HANDLE_FIELDS, use_keys, use_valid_fields, use_whitelist_fields_filter},
        utils::{convert_uids_to_scripts, merge_profiles, process_chain},
    },
};

/// 1
mod chain;
/// 3
mod field;
/// 5
mod tun;
/// 2
mod utils;

#[cfg(feature = "clash-rs-compat")]
fn apply_clash_rs_compat(config: &mut Mapping) {
    use serde_yaml::Value;

    if config.get("allow-lan") == Some(&Value::Bool(true)) {
        // config.remove("allow-lan");
        config.insert("bind-address".into(), Value::String("0.0.0.0".into()));
    }
}

/// Enhance mode
/// 返回最终配置、该配置包含的键、和script执行的结果
pub async fn enhance() -> anyhow::Result<(Mapping, Vec<String>, PostProcessingOutput)> {
    // config.yaml 的配置
    let clash_config = { Config::clash().latest().0.clone() };

    let (enable_tun, enable_filter) = {
        let verge = Config::verge();
        let verge = verge.latest();
        (
            verge.enable_tun_mode.unwrap_or(false),
            verge.enable_clash_fields.unwrap_or(true),
        )
    };

    // 从profiles里拿东西·
    let (profiles, profile_chain, valid) = {
        let profiles = Config::profiles();
        let profiles = profiles.latest();

        let profile_chain_mapping = profiles
            .get_current()
            .iter()
            .map(|uid| {
                let item = profiles.get_item(uid)?;
                let chain = match item {
                    Profile::Remote(profile) => &profile.chain,
                    Profile::Local(profile) => &profile.chain,
                };
                Ok((
                    item.uid().to_string(),
                    convert_uids_to_scripts(&profiles, chain)?,
                ))
            })
            .collect::<anyhow::Result<IndexMap<_, _>>>()?;

        let current_mappings = profiles
            .current_mappings()?
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect::<IndexMap<_, _>>();

        let valid = profiles.valid.clone();

        (current_mappings, profile_chain_mapping, valid)
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
    let mut config = merge_profiles(profiles)?;

    // 执行全局 chain
    // let (mut config, global_chain_output) = process_chain(config, &global_chain).await;
    // postprocessing_output.global = global_chain_output;

    // 记录当前配置包含的键
    let exists_keys = use_keys(&config);
    config = use_whitelist_fields_filter(config, &valid, enable_filter);

    // 合并默认的config
    clash_config
        .iter()
        // only guarded fields should be overwritten
        .filter(|(k, _)| HANDLE_FIELDS.contains(&k.as_str().unwrap_or_default()))
        .for_each(|(key, value)| {
            config.insert(key.to_owned(), value.clone());
        });

    config = tun::use_tun(config, enable_tun)?;

    #[cfg(feature = "clash-rs-compat")]
    apply_clash_rs_compat(&mut config);

    Ok((config, exists_keys, postprocessing_output))
}

#[cfg(all(test, feature = "clash-rs-compat"))]
mod tests {
    use super::apply_clash_rs_compat;
    use serde_yaml::{Mapping, Value};

    #[test]
    fn converts_allow_lan_to_bind_address() {
        let mut config = Mapping::new();
        config.insert("allow-lan".into(), Value::Bool(true));

        apply_clash_rs_compat(&mut config);

        assert!(!config.contains_key("allow-lan"));
        assert_eq!(
            config.get("bind-address"),
            Some(&Value::String("0.0.0.0".into()))
        );
    }

    #[test]
    fn preserves_disabled_allow_lan() {
        let mut config = Mapping::new();
        config.insert("allow-lan".into(), Value::Bool(false));

        apply_clash_rs_compat(&mut config);

        assert_eq!(config.get("allow-lan"), Some(&Value::Bool(false)));
        assert!(!config.contains_key("bind-address"));
    }
}
