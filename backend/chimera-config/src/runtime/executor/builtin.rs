//! WhitelistFieldFilter / GuardOverrides / Finalizing (spec 7.4).
//! Constants transplanted verbatim from enhance/field.rs:4-56.

use std::sync::Arc;

use indexmap::IndexMap;

use crate::runtime::{
    post_process::{OwnershipEntries, apply_defaults, apply_settings},
    value::{ConfigObject, ConfigValue},
};

use super::value_util::{obj_get, obj_insert};
#[cfg(test)]
use super::{GuardInputs, TunFlavor, TunParams, error::RuntimePipelineError, plan};
#[cfg(test)]
use crate::runtime::post_process::keep_inherited;

pub(super) const HANDLE_FIELDS: [&str; 9] = [
    "mode",
    "port",
    "socks-port",
    "mixed-port",
    "allow-lan",
    "log-level",
    "ipv6",
    "secret",
    "external-controller",
];

pub(super) const DEFAULT_FIELDS: [&str; 5] = [
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rules",
    "rule-providers",
];

/// Chimera Client's top-level MMDB path is a core-specific extension, not a
/// shared Mihomo-family field.
pub(super) const CHIMERA_CLIENT_FIELDS: [&str; 1] = ["mmdb"];

pub(super) const OTHERS_FIELDS: [&str; 31] = [
    "dns",
    "tun",
    "ebpf",
    "hosts",
    "script",
    "profile",
    "payload",
    "tunnels",
    "auto-redir",
    "experimental",
    "interface-name",
    "routing-mark",
    "redir-port",
    "tproxy-port",
    "iptables",
    "external-ui",
    "bind-address",
    "authentication",
    "tls",
    "sniffer",
    "geox-url",
    "listeners",
    "sub-rules",
    "geodata-mode",
    "unified-delay",
    "tcp-concurrent",
    "enable-process",
    "find-process-mode",
    "skip-auth-prefixes",
    "external-controller-tls",
    "global-client-fingerprint",
];

/// Default-core view retained for the legacy unit-test assertions.
#[cfg(test)]
pub(super) fn known_fields() -> impl Iterator<Item = &'static str> {
    known_fields_for_core(false)
}

/// Full whitelist in ref order, with Chimera Client's extension appended
/// only when the selected core supports it.
pub(super) fn known_fields_for_core(
    core_supports_mmdb: bool,
) -> impl Iterator<Item = &'static str> {
    DEFAULT_FIELDS
        .into_iter()
        .chain(core_supports_mmdb.then_some(CHIMERA_CLIENT_FIELDS[0]))
        .chain(HANDLE_FIELDS)
        .chain(OTHERS_FIELDS)
}

/// Default-core view retained for the legacy unit-test assertions.
#[cfg(test)]
pub(super) fn stage1_fields(valid: &[String]) -> Vec<String> {
    stage1_fields_for_core(valid, false)
}

/// Stage-1 list: (valid intersect OTHERS, lowercased) ++ DEFAULT and the
/// selected core's extensions; HANDLE never comes from profile output.
pub(super) fn stage1_fields_for_core(valid: &[String], core_supports_mmdb: bool) -> Vec<String> {
    let mut fields: Vec<String> = valid
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .filter(|field| OTHERS_FIELDS.contains(&field.as_str()))
        .collect();
    fields.extend(DEFAULT_FIELDS.iter().map(|field| (*field).to_string()));
    if core_supports_mmdb {
        fields.extend(
            CHIMERA_CLIENT_FIELDS
                .iter()
                .map(|field| (*field).to_string()),
        );
    }
    fields
}

pub(super) fn whitelist_filter(
    config: &ConfigValue,
    allow: &[String],
    enabled: bool,
) -> ConfigValue {
    if !enabled {
        return config.clone();
    }
    let Some(map) = config.as_object_arc() else {
        return config.clone();
    };
    let filtered: ConfigObject = map
        .iter()
        .filter(|(key, _)| allow.iter().any(|allowed| allowed.as_str() == key.as_ref()))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    ConfigValue::Object(Arc::new(filtered))
}

/// Legacy HANDLE overwrite (enhance/mod.rs:110-116): typed overrides + caller
/// resolved ports force-inserted at top level, bypassing stage-1 whitelist.
#[cfg(all(test, feature = "default-meta"))]
pub(super) fn apply_guard(
    config: &ConfigValue,
    guard: &GuardInputs<'_>,
) -> Result<ConfigValue, RuntimePipelineError> {
    apply_guard_with_source(config, config, guard)
}

/// Compatibility oracle for the previous unseparated guard pass.
#[cfg(test)]
pub(super) fn apply_guard_with_source(
    config: &ConfigValue,
    source: &ConfigValue,
    guard: &GuardInputs<'_>,
) -> Result<ConfigValue, RuntimePipelineError> {
    let ownership = plan::guard_ownership(guard);
    let (inherited, _) = keep_inherited(config, source, &ownership);
    let (next, _) = apply_settings(&inherited, &ownership);

    Ok(next)
}

/// Compatibility oracle for the previous unseparated TUN pass.
#[cfg(test)]
pub(super) fn apply_tun_overrides_with_source(
    config: &ConfigValue,
    source: &ConfigValue,
    tun: &TunParams,
) -> ConfigValue {
    let ownership = plan::tun_ownership(tun);
    let (inherited, _) = keep_inherited(config, source, &ownership);
    apply_settings(&inherited, &ownership).0
}

/// Old finalizer behavior retained for comparison with stage-separated execution.
#[cfg(test)]
pub(super) fn finalize(
    config: &ConfigValue,
    tun: &TunParams,
    whitelist_enabled: bool,
) -> ConfigValue {
    let config = apply_tun_overrides_with_source(config, config, tun);
    finalize_for_core(&config, tun, whitelist_enabled, false)
}

#[cfg(test)]
pub(super) fn finalize_for_core(
    config: &ConfigValue,
    tun: &TunParams,
    whitelist_enabled: bool,
    core_supports_mmdb: bool,
) -> ConfigValue {
    let filtered = final_field_filter(config, whitelist_enabled, core_supports_mmdb);
    let defaults = apply_runtime_defaults(&filtered, &plan::final_defaults_ownership(tun));
    finalize_post_defaults(&defaults)
}

/// Second core-specific field filter. Never pass Chimera Client's `mmdb`
/// extension to a core that does not support it, even if filtering is off.
pub(super) fn final_field_filter(
    config: &ConfigValue,
    whitelist_enabled: bool,
    core_supports_mmdb: bool,
) -> ConfigValue {
    let full: Vec<String> = known_fields_for_core(core_supports_mmdb)
        .map(str::to_string)
        .collect();
    let mut next = whitelist_filter(config, &full, whitelist_enabled);
    if !core_supports_mmdb {
        next = strip_chimera_client_fields(&next);
    }
    next
}

/// Apply the declarative plan's mandatory settings and then its defaults.
/// Runs only after built-in transforms, retaining the original field order.
pub(super) fn apply_runtime_defaults(
    config: &ConfigValue,
    entries: &OwnershipEntries,
) -> ConfigValue {
    let (settings, _) = apply_settings(config, entries);
    apply_defaults(&settings, entries).0
}

/// The pre-plan finalizer, used to verify the new ownership path exactly.
#[cfg(test)]
pub(super) fn legacy_runtime_defaults(config: &ConfigValue, tun: &TunParams) -> ConfigValue {
    let next = apply_tun(config, tun);
    apply_cache(&next)
}

/// The last postprocessing pass changes only include-all groups and key order.
pub(super) fn finalize_post_defaults(config: &ConfigValue) -> ConfigValue {
    let next = apply_include_all(config);
    apply_sort(&next)
}

fn strip_chimera_client_fields(config: &ConfigValue) -> ConfigValue {
    let Some(map) = config.as_object_arc() else {
        return config.clone();
    };
    if !map.contains_key("mmdb") {
        return config.clone();
    }

    let mut filtered = (**map).clone();
    filtered.retain(|key, _| key.as_ref() != "mmdb");
    ConfigValue::Object(Arc::new(filtered))
}

#[cfg(test)]
fn object_of(value: Option<&ConfigValue>) -> ConfigObject {
    value
        .and_then(ConfigValue::as_object_arc)
        .map(|map| (**map).clone())
        .unwrap_or_default()
}

#[cfg(test)]
fn append_default(map: &mut ConfigObject, key: &str, value: ConfigValue) {
    if !map.contains_key(key) {
        map.insert(Arc::from(key), value);
    }
}

fn string_value(text: &str) -> ConfigValue {
    ConfigValue::String(Arc::from(text))
}

#[cfg(test)]
fn string_list(items: &[&str]) -> ConfigValue {
    ConfigValue::Array(Arc::from(
        items
            .iter()
            .map(|item| string_value(item))
            .collect::<Vec<_>>(),
    ))
}

/// Mirrors enhance/tun.rs:26-109; `revise!` overwrites and `append!` fills
/// missing keys only.
#[cfg(test)]
fn apply_tun(config: &ConfigValue, params: &TunParams) -> ConfigValue {
    let existing = obj_get(config, "tun");
    if !params.enable && existing.is_none() {
        return config.clone();
    }

    let mut tun = object_of(existing);
    tun.insert(Arc::from("enable"), ConfigValue::Bool(params.enable));
    if params.enable {
        match params.flavor {
            TunFlavor::ClashRs => {
                append_default(&mut tun, "device-id", string_value("dev://utun1989"));
            }
            TunFlavor::ChimeraClient => {
                // These fields belong to Chimera Client's legacy core contract.
                append_default(&mut tun, "device-id", string_value("dev://utun1989"));
                append_default(&mut tun, "route-all", ConfigValue::Bool(true));
                append_default(&mut tun, "dns-hijack", ConfigValue::Bool(true));
                append_default(
                    &mut tun,
                    "so-mark",
                    ConfigValue::Number(serde_json::Number::from(7777)),
                );
            }
            TunFlavor::Standard { stack } => {
                append_default(&mut tun, "stack", string_value(stack.as_ref()));
                append_default(&mut tun, "dns-hijack", string_list(&["any:53"]));
                append_default(&mut tun, "auto-detect-interface", ConfigValue::Bool(true));
            }
        }
    }

    let mut next = obj_insert(config, "tun", ConfigValue::Object(Arc::new(tun)));
    if params.enable {
        next = apply_tun_dns(&next, params.windows_fake_ip_filter, params.flavor);
    }
    next
}

#[cfg(test)]
fn apply_tun_dns(
    config: &ConfigValue,
    windows_fake_ip_filter: bool,
    flavor: TunFlavor,
) -> ConfigValue {
    let mut dns = object_of(obj_get(config, "dns"));
    dns.insert(Arc::from("enable"), ConfigValue::Bool(true));
    append_default(&mut dns, "enhanced-mode", string_value("fake-ip"));
    if flavor == TunFlavor::ChimeraClient {
        // Keep the Client DNS defaults separate from the reference core flavors.
        append_default(
            &mut dns,
            "nameserver",
            string_list(&[
                "https://dns.alidns.com/dns-query",
                "114.114.114.114",
                "223.5.5.5",
                "8.8.8.8",
            ]),
        );
        append_default(
            &mut dns,
            "default-nameserver",
            string_list(&["114.114.114.114", "1.1.1.1", "8.8.8.8"]),
        );
    } else {
        append_default(&mut dns, "fake-ip-range", string_value("198.18.0.1/16"));
        append_default(
            &mut dns,
            "nameserver",
            string_list(&["114.114.114.114", "223.5.5.5", "8.8.8.8"]),
        );
        append_default(
            &mut dns,
            "fallback",
            ConfigValue::Array(Arc::from(Vec::<ConfigValue>::new())),
        );
    }
    if windows_fake_ip_filter {
        append_default(
            &mut dns,
            "fake-ip-filter",
            string_list(&[
                "dns.msftncsi.com",
                "www.msftncsi.com",
                "www.msftconnecttest.com",
            ]),
        );
    }
    obj_insert(config, "dns", ConfigValue::Object(Arc::new(dns)))
}

/// Mirrors enhance/mod.rs:163-248.
fn apply_include_all(config: &ConfigValue) -> ConfigValue {
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

/// Mirrors enhance/mod.rs:250-261: existing `profile` keys are not merged.
#[cfg(test)]
fn apply_cache(config: &ConfigValue) -> ConfigValue {
    if obj_get(config, "profile").is_some() {
        return config.clone();
    }
    let mut profile: ConfigObject = IndexMap::new();
    profile.insert(Arc::from("store-selected"), ConfigValue::Bool(true));
    profile.insert(Arc::from("store-fake-ip"), ConfigValue::Bool(false));
    obj_insert(config, "profile", ConfigValue::Object(Arc::new(profile)))
}

/// Mirrors field.rs:115-147: known keys use HANDLE++OTHERS++DEFAULT order;
/// unknown keys keep their original relative order at the end.
fn apply_sort(config: &ConfigValue) -> ConfigValue {
    let Some(map) = config.as_object_arc() else {
        return config.clone();
    };
    let mut next: ConfigObject = IndexMap::new();
    for key in HANDLE_FIELDS
        .into_iter()
        .chain(OTHERS_FIELDS)
        .chain(DEFAULT_FIELDS)
    {
        if let Some(value) = map.get(key) {
            next.insert(Arc::from(key), value.clone());
        }
    }
    for (key, value) in map.iter() {
        if !next.contains_key(key.as_ref()) {
            next.insert(key.clone(), value.clone());
        }
    }
    ConfigValue::Object(Arc::new(next))
}
