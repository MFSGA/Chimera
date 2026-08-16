use std::borrow::Borrow;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde_yaml::{Mapping, Value};

use crate::{
    config::profile::{item_type::ProfileUid, profiles::Profiles},
    enhance::{
        chain::{ChainItem, ChainTypeWrapper, Logs},
        script::runner::{RunnerManager, ScriptRunRequest},
    },
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
    config: Mapping,
    nodes: &[ChainItem],
) -> Result<(Mapping, IndexMap<ProfileUid, Logs>)> {
    process_chain_with_runner(config, nodes, &RunnerManager::new()).await
}

async fn process_chain_with_runner(
    mut config: Mapping,
    nodes: &[ChainItem],
    runner: &RunnerManager,
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
                let output = runner
                    .run(
                        *script_type,
                        ScriptRunRequest {
                            uid: node.uid.clone(),
                            source: source.clone(),
                            config,
                        },
                    )
                    .await
                    .map_err(|error| {
                        anyhow::anyhow!(
                            "failed to execute script transform {}: {error:#}",
                            node.uid
                        )
                    })?;
                config = output.config;
                result_map.insert(node.uid.clone(), output.logs);
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
    use std::sync::Arc;

    use async_trait::async_trait;

    use super::*;
    use crate::{
        config::profile::item_type::ScriptType,
        enhance::{
            chain::LogSpan,
            script::runner::{ScriptRunOutput, ScriptRunner},
        },
    };

    struct TestScriptRunner;

    #[async_trait]
    impl ScriptRunner for TestScriptRunner {
        async fn run(&self, mut request: ScriptRunRequest) -> Result<ScriptRunOutput> {
            assert_eq!(request.uid, "sj-test");
            assert_eq!(request.source, "test-script");
            assert_eq!(
                request.config.get("before").and_then(Value::as_bool),
                Some(true)
            );
            request
                .config
                .insert(Value::String("script-ran".into()), Value::Bool(true));
            Ok(ScriptRunOutput {
                config: request.config,
                logs: vec![(LogSpan::Info, "script executed".into())],
            })
        }
    }

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
    async fn injected_script_runner_receives_current_config_and_returns_logs() {
        let runner =
            RunnerManager::new().with_runner(ScriptType::JavaScript, Arc::new(TestScriptRunner));
        let chain = vec![
            ChainItem {
                uid: "m-before".into(),
                data: ChainTypeWrapper::Merge(mapping("before: true\n")),
            },
            ChainItem {
                uid: "sj-test".into(),
                data: ChainTypeWrapper::Script {
                    script_type: ScriptType::JavaScript,
                    source: "test-script".into(),
                },
            },
            ChainItem {
                uid: "m-after".into(),
                data: ChainTypeWrapper::Merge(mapping("after: true\n")),
            },
        ];

        let (config, output) = process_chain_with_runner(Mapping::new(), &chain, &runner)
            .await
            .unwrap();

        assert_eq!(config.get("before").and_then(Value::as_bool), Some(true));
        assert_eq!(
            config.get("script-ran").and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(config.get("after").and_then(Value::as_bool), Some(true));
        assert_eq!(
            output.get("sj-test"),
            Some(&vec![(LogSpan::Info, "script executed".into())])
        );
    }

    #[tokio::test]
    async fn lua_transform_runs_through_the_default_runner() {
        let chain = vec![ChainItem {
            uid: "sl-test".into(),
            data: ChainTypeWrapper::Script {
                script_type: ScriptType::Lua,
                source: r#"
config["unified-delay"] = true
info("lua transform ran")
return config
"#
                .into(),
            },
        }];

        let (config, output) = process_chain(mapping("unified-delay: false\n"), &chain)
            .await
            .unwrap();
        assert_eq!(
            config
                .get("unified-delay")
                .and_then(serde_yaml::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            output.get("sl-test"),
            Some(&vec![(LogSpan::Info, "lua transform ran".into())])
        );
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
