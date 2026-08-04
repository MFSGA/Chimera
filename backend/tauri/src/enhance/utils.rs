use std::borrow::Borrow;

use anyhow::{Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use crate::{
    config::profile::{item_type::ProfileUid, profiles::Profiles},
    enhance::chain::ChainItem,
};

pub fn convert_uids_to_scripts(profiles: &Profiles, uids: &[ProfileUid]) -> Result<Vec<ChainItem>> {
    uids.iter()
        .map(|uid| profiles.get_item(uid).and_then(ChainItem::try_from))
        .collect()
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
    config: Mapping,
    _nodes: &[ChainItem],
) -> (Mapping, IndexMap<ProfileUid, Logs>) {
    log::debug!("todo: impl script_runner");
    (config, IndexMap::new())
}

/// 合并多个配置
// TODO: 可能移动到其他地方
// TODO: 增加自定义合并逻辑
// TODO: 添加元信息
pub fn merge_profiles<T: Borrow<String>>(mappings: IndexMap<T, Mapping>) -> Result<Mapping> {
    let mut mappings = mappings.into_iter();
    let Some((first_uid, mut merged)) = mappings.next() else {
        return Ok(Mapping::new());
    };

    if merged
        .get("proxies")
        .is_some_and(|proxies| !proxies.is_sequence())
    {
        bail!(
            "profile {} has a non-sequence `proxies` field",
            first_uid.borrow()
        );
    }

    for (uid, mapping) in mappings {
        let proxies = mapping
            .get("proxies")
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "profile {} does not contain a `proxies` field",
                    uid.borrow()
                )
            })?
            .as_sequence()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "profile {} has a non-sequence `proxies` field",
                    uid.borrow()
                )
            })?
            .clone();

        if !merged.contains_key("proxies") {
            merged.insert(Value::String("proxies".into()), Value::Sequence(Vec::new()));
        }
        let merged_proxies = merged
            .get_mut("proxies")
            .and_then(Value::as_sequence_mut)
            .expect("the first profile proxies field was validated");
        merged_proxies.extend(proxies);
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use serde_yaml::{Mapping, Value};

    use super::{convert_uids_to_scripts, merge_profiles};
    use crate::config::profile::{
        item::{Profile, local::LocalProfile},
        profiles::Profiles,
    };

    fn mapping(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).expect("valid profile mapping fixture")
    }

    fn local_profile(uid: &str) -> Profile {
        let mut profile = LocalProfile::builder()
            .build()
            .expect("failed to build chain profile fixture");
        profile.shared.uid = uid.to_string();
        profile.shared.file = format!("{uid}.yaml");
        Profile::Local(profile)
    }

    #[test]
    fn chain_conversion_preserves_reference_order() {
        let profiles = Profiles {
            items: vec![local_profile("profile-a"), local_profile("profile-b")],
            ..Profiles::default()
        };

        let chain = convert_uids_to_scripts(&profiles, &["profile-b".into(), "profile-a".into()])
            .expect("valid chain references must convert");

        assert_eq!(
            chain.into_iter().map(|item| item.uid).collect::<Vec<_>>(),
            vec!["profile-b", "profile-a"]
        );
    }

    #[test]
    fn chain_conversion_rejects_missing_references() {
        let profiles = Profiles {
            items: vec![local_profile("profile-a")],
            ..Profiles::default()
        };

        let error = convert_uids_to_scripts(&profiles, &["missing".into()])
            .expect_err("missing chain reference must not be silently ignored");

        assert!(error.to_string().contains("uid:missing"));
    }

    #[test]
    fn merging_no_profiles_returns_an_empty_mapping() {
        let mappings = IndexMap::<String, Mapping>::new();

        assert!(
            merge_profiles(mappings)
                .expect("empty profile merge must succeed")
                .is_empty()
        );
    }

    #[test]
    fn merging_one_profile_preserves_all_fields() {
        let mut mappings = IndexMap::new();
        mappings.insert(
            "profile-a".to_string(),
            mapping("mixed-port: 7890\nproxies:\n  - name: first\n"),
        );

        let merged = merge_profiles(mappings).expect("single profile merge must succeed");

        assert_eq!(merged.get("mixed-port"), Some(&Value::Number(7890.into())));
        assert_eq!(
            merged
                .get("proxies")
                .and_then(Value::as_sequence)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn merging_profiles_appends_proxy_sequences_in_order() {
        let mut mappings = IndexMap::new();
        mappings.insert(
            "profile-a".to_string(),
            mapping("mixed-port: 7890\nproxies:\n  - name: first\n"),
        );
        mappings.insert(
            "profile-b".to_string(),
            mapping("proxies:\n  - name: second\n  - name: third\n"),
        );

        let merged = merge_profiles(mappings).expect("valid profile merge must succeed");
        let names = merged
            .get("proxies")
            .and_then(Value::as_sequence)
            .expect("merged proxies must be a sequence")
            .iter()
            .map(|proxy| {
                proxy
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("proxy fixture must have a name")
            })
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["first", "second", "third"]);
    }

    #[test]
    fn merging_adds_a_proxy_sequence_when_the_first_profile_has_none() {
        let mut mappings = IndexMap::new();
        mappings.insert(
            "profile-a".to_string(),
            mapping("proxy-providers:\n  provider: {}\n"),
        );
        mappings.insert(
            "profile-b".to_string(),
            mapping("proxies:\n  - name: second\n"),
        );

        let merged = merge_profiles(mappings)
            .expect("later proxies must be merged into a provider-only base profile");

        assert_eq!(
            merged
                .get("proxies")
                .and_then(Value::as_sequence)
                .map(Vec::len),
            Some(1)
        );
        assert!(merged.contains_key("proxy-providers"));
    }

    #[test]
    fn merging_rejects_a_non_sequence_first_proxy_field() {
        let mut mappings = IndexMap::new();
        mappings.insert("profile-a".to_string(), mapping("proxies: invalid\n"));

        let error = merge_profiles(mappings)
            .expect_err("non-sequence first proxies field must be rejected");

        assert!(error.to_string().contains("profile-a"));
        assert!(error.to_string().contains("non-sequence"));
    }

    #[test]
    fn merging_rejects_a_later_profile_without_proxies() {
        let mut mappings = IndexMap::new();
        mappings.insert("profile-a".to_string(), mapping("proxies: []\n"));
        mappings.insert("profile-b".to_string(), mapping("dns: {}\n"));

        let error =
            merge_profiles(mappings).expect_err("later profile without proxies must be rejected");

        assert!(error.to_string().contains("profile-b"));
        assert!(error.to_string().contains("does not contain"));
    }

    #[test]
    fn merging_rejects_a_later_non_sequence_proxy_field() {
        let mut mappings = IndexMap::new();
        mappings.insert("profile-a".to_string(), mapping("proxies: []\n"));
        mappings.insert("profile-b".to_string(), mapping("proxies: invalid\n"));

        let error = merge_profiles(mappings)
            .expect_err("later non-sequence proxies field must be rejected");

        assert!(error.to_string().contains("profile-b"));
        assert!(error.to_string().contains("non-sequence"));
    }
}
