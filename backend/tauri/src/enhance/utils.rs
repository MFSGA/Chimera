use std::borrow::Borrow;

use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

use crate::{
    config::profile::{item_type::ProfileUid, profiles::Profiles},
    enhance::chain::{ChainItem, ChainTypeWrapper},
};

pub fn resolve_transform_chain(profiles: &Profiles, uids: &[ProfileUid]) -> Result<Vec<ChainItem>> {
    uids.iter()
        .map(|uid| {
            let item = profiles
                .get_item(uid)
                .with_context(|| format!("transform profile {uid} does not exist"))?;
            ChainItem::try_from(item)
                .with_context(|| format!("failed to resolve transform profile {uid}"))
        })
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

fn merge_value(target: &mut Value, patch: Value) {
    match (target, patch) {
        (Value::Mapping(target), Value::Mapping(patch)) => {
            for (key, value) in patch {
                if let Some(existing) = target.get_mut(&key) {
                    merge_value(existing, value);
                } else {
                    target.insert(key, value);
                }
            }
        }
        (target, patch) => *target = patch,
    }
}

fn apply_merge_mapping(config: &mut Mapping, patch: Mapping) {
    for (key, value) in patch {
        if let Some(existing) = config.get_mut(&key) {
            merge_value(existing, value);
        } else {
            config.insert(key, value);
        }
    }
}

/// Apply transform profiles in chain order.
pub async fn process_chain(
    mut config: Mapping,
    nodes: &[ChainItem],
) -> Result<(Mapping, IndexMap<ProfileUid, Logs>)> {
    let mut result_map = IndexMap::new();

    for node in nodes {
        match &node.data {
            ChainTypeWrapper::Merge(mapping) => {
                apply_merge_mapping(&mut config, mapping.clone());
                result_map.insert(node.uid.clone(), Vec::new());
            }
            ChainTypeWrapper::Script {
                script_type,
                source,
            } => {
                let _ = source;
                bail!(
                    "script transform {} ({script_type:?}) cannot run until the script runtime is available",
                    node.uid
                );
            }
        }
    }

    Ok((config, result_map))
}

/// Merge selected source profiles while preserving the legacy behavior:
/// the first profile provides the complete config and later profiles append proxies.
pub fn merge_profiles<T: Borrow<String>>(mappings: IndexMap<T, Mapping>) -> Result<Mapping> {
    let mut mappings = mappings.into_iter();
    let Some((_first_uid, first)) = mappings.next() else {
        return Ok(Mapping::new());
    };
    let mut merged = first;

    for (uid, mapping) in mappings {
        let Some(proxies) = mapping.get("proxies") else {
            continue;
        };
        let proxies = proxies
            .as_sequence()
            .with_context(|| format!("profile {} has a non-sequence proxies field", uid.borrow()))?
            .clone();
        if proxies.is_empty() {
            continue;
        }

        match merged.get_mut("proxies") {
            Some(value) => value
                .as_sequence_mut()
                .with_context(|| "the primary profile has a non-sequence proxies field")?
                .extend(proxies),
            None => {
                merged.insert(Value::String("proxies".into()), Value::Sequence(proxies));
            }
        }
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::item_type::ScriptType;

    fn mapping(source: &str) -> Mapping {
        serde_yaml::from_str(source).unwrap()
    }

    #[test]
    fn merge_profiles_appends_secondary_proxies_without_panicking_on_missing_fields() {
        let mappings = IndexMap::from([
            (
                "first".to_string(),
                mapping(
                    r#"
mode: rule
proxy-groups:
  - name: main
    type: select
"#,
                ),
            ),
            (
                "second".to_string(),
                mapping(
                    r#"
proxies:
  - name: node-a
    type: direct
"#,
                ),
            ),
            ("third".to_string(), mapping("rules: []\n")),
        ]);

        let merged = merge_profiles(mappings).unwrap();
        assert_eq!(merged.get("mode").unwrap().as_str(), Some("rule"));
        let proxies = merged.get("proxies").unwrap().as_sequence().unwrap();
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].get("name").unwrap().as_str(), Some("node-a"));
    }

    #[test]
    fn merge_profiles_rejects_invalid_proxy_shapes() {
        let mappings = IndexMap::from([
            ("first".to_string(), mapping("proxies: []\n")),
            ("second".to_string(), mapping("proxies: invalid\n")),
        ]);

        let error = merge_profiles(mappings).unwrap_err().to_string();
        assert!(error.contains("second"));
        assert!(error.contains("non-sequence proxies"));
    }

    #[tokio::test]
    async fn merge_transform_recursively_overrides_mappings_and_replaces_sequences() {
        let config = mapping(
            r#"
mode: rule
dns:
  enable: false
  nameserver:
    - 1.1.1.1
rules:
  - MATCH,DIRECT
"#,
        );
        let patch = mapping(
            r#"
dns:
  enable: true
  enhanced-mode: fake-ip
rules:
  - DOMAIN,example.com,DIRECT
"#,
        );
        let chain = vec![ChainItem {
            uid: "m-test".into(),
            data: ChainTypeWrapper::Merge(patch),
        }];

        let (merged, output) = process_chain(config, &chain).await.unwrap();
        let dns = merged.get("dns").unwrap().as_mapping().unwrap();
        assert_eq!(dns.get("enable").unwrap().as_bool(), Some(true));
        assert_eq!(dns.get("enhanced-mode").unwrap().as_str(), Some("fake-ip"));
        assert_eq!(
            dns.get("nameserver").unwrap().as_sequence().unwrap().len(),
            1
        );
        let rules = merged.get("rules").unwrap().as_sequence().unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].as_str(), Some("DOMAIN,example.com,DIRECT"));
        assert!(output.contains_key("m-test"));
    }

    #[tokio::test]
    async fn script_transform_fails_closed_until_runner_is_available() {
        let chain = vec![ChainItem {
            uid: "s-test".into(),
            data: ChainTypeWrapper::Script {
                script_type: ScriptType::JavaScript,
                source: "export default (config) => config".into(),
            },
        }];

        let error = process_chain(Mapping::new(), &chain)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("s-test"));
        assert!(error.contains("script runtime"));
    }
}
