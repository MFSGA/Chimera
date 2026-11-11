//! Legacy Chimera `include-all` expansion, isolated as a pipeline step.
//!
//! This intentionally preserves the prior behavior: `include-all: true`
//! expands named Proxy groups using Profile proxy declaration order followed
//! by provider names; duplicate names and user entries retain their previous
//! handling. It is not the full Nyanpasu/Mihomo semantics for
//! `include-all-proxies`, `include-all-providers`, or regex filtering.
//! Those require a separate compatibility decision, not a silent replacement.

use std::sync::Arc;

use crate::runtime::value::ConfigValue;

use super::value_util::{obj_get, obj_insert};

fn string_value(text: &str) -> ConfigValue {
    ConfigValue::String(Arc::from(text))
}

/// Mirrors Chimera's original `enhance/mod.rs:163-248` processing.
pub(super) fn expand_legacy(config: &ConfigValue) -> ConfigValue {
    let mut names: Vec<String> = Vec::new();
    if let Some(proxies) = obj_get(config, "proxies").and_then(ConfigValue::as_array_arc) {
        for proxy in proxies.iter() {
            if let Some(ConfigValue::String(name)) =
                proxy.as_object_arc().and_then(|map| map.get("name"))
            {
                names.push(name.to_string());
            }
        }
    }
    if let Some(providers) = obj_get(config, "proxy-providers").and_then(ConfigValue::as_object_arc)
    {
        names.extend(providers.keys().map(|key| key.to_string()));
    }

    let Some(groups) = obj_get(config, "proxy-groups").and_then(ConfigValue::as_array_arc) else {
        return config.clone();
    };
    let rebuilt: Vec<ConfigValue> = groups
        .iter()
        .map(|group| {
            let Some(map) = group.as_object_arc() else {
                return group.clone();
            };
            let include_all = matches!(map.get("include-all"), Some(ConfigValue::Bool(true)));
            if !include_all || !map.contains_key("name") {
                return group.clone();
            }
            let existing: Vec<String> = map
                .get("proxies")
                .and_then(ConfigValue::as_array_arc)
                .map(|seq| {
                    seq.iter()
                        .filter_map(|value| match value {
                            ConfigValue::String(name) => Some(name.to_string()),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();

            let mut proxies: Vec<ConfigValue> =
                names.iter().map(|name| string_value(name)).collect();
            proxies.extend(
                existing
                    .iter()
                    .filter(|name| !names.contains(name))
                    .map(|name| string_value(name)),
            );

            let mut next = (**map).clone();
            next.insert(Arc::from("proxies"), ConfigValue::Array(Arc::from(proxies)));
            next.shift_remove("include-all");
            ConfigValue::Object(Arc::new(next))
        })
        .collect();
    obj_insert(
        config,
        "proxy-groups",
        ConfigValue::Array(Arc::from(rebuilt)),
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn legacy_expansion_preserves_user_order_and_appends_extra_entries() {
        let input = ConfigValue::from_json_value(json!({
            "proxies": [{"name":"Alpha"},{"name":"Beta"}],
            "proxy-providers": {"provider-b": {}, "provider-a": {}},
            "proxy-groups": [{
                "name":"Group", "type":"select", "include-all":true,
                "proxies":["DIRECT","Beta"]
            }],
        }));
        let result = expand_legacy(&input).to_json();
        assert_eq!(
            result["proxy-groups"][0]["proxies"],
            json!(["Alpha", "Beta", "provider-b", "provider-a", "DIRECT"]),
        );
        assert!(result["proxy-groups"][0].get("include-all").is_none());
    }

    #[test]
    fn untouched_reference_specific_flags_remain_for_the_core() {
        let input = ConfigValue::from_json_value(json!({
            "proxy-groups": [
                {"name":"Group", "include-all-proxies":true, "filter":"HK"},
                {"include-all":true,"proxies":["DIRECT"]},
            ],
        }));
        assert_eq!(
            serde_json::to_string(&expand_legacy(&input).to_json()).unwrap(),
            serde_json::to_string(&input.to_json()).unwrap(),
            "do not silently change unsupported include-all semantics"
        );
    }
}
