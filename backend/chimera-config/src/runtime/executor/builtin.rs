//! WhitelistFieldFilter / GuardOverrides / Finalizing (spec 7.4).
//! Constants transplanted verbatim from enhance/field.rs:4-56.

use std::sync::Arc;

use indexmap::IndexMap;
use json_patch::{Patch, PatchOperation, RemoveOperation, jsonptr::PointerBuf};

#[cfg(test)]
use crate::runtime::post_process::{OwnershipEntries, apply_defaults, apply_settings};
use crate::runtime::value::{ConfigObject, ConfigValue};

#[cfg(test)]
use super::{GuardInputs, TunParams, error::RuntimePipelineError, plan};
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

/// Keeps the top-level keys in `allow`, with the patch that removes the rest.
pub(super) fn filter_fields(config: &ConfigValue, allow: &[String]) -> (ConfigValue, Patch) {
    let Some(map) = config.as_object_arc() else {
        return (config.clone(), Patch(Vec::new()));
    };
    let mut removed = Vec::new();
    let mut filtered = ConfigObject::with_capacity(map.len());
    for (key, value) in map.iter() {
        if allow.iter().any(|allowed| allowed.as_str() == key.as_ref()) {
            filtered.insert(key.clone(), value.clone());
        } else {
            removed.push(PatchOperation::Remove(RemoveOperation {
                path: PointerBuf::from_tokens([key.as_ref()]),
            }));
        }
    }
    if removed.is_empty() {
        return (config.clone(), Patch(removed));
    }
    (ConfigValue::Object(Arc::new(filtered)), Patch(removed))
}

pub(super) fn whitelist_filter(
    config: &ConfigValue,
    allow: &[String],
    enabled: bool,
) -> ConfigValue {
    if !enabled {
        return config.clone();
    }
    filter_fields(config, allow).0
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
    finalize_post_defaults(&super::include_all::expand_include_all(&defaults).0)
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

/// Unit helper for inspecting one declarative ownership plan.
#[cfg(test)]
pub(super) fn apply_runtime_defaults(
    config: &ConfigValue,
    entries: &OwnershipEntries,
) -> ConfigValue {
    let (settings, _) = apply_settings(config, entries);
    apply_defaults(&settings, entries).0
}

/// The last postprocessing pass changes only the top-level field ordering.
pub(super) fn finalize_post_defaults(config: &ConfigValue) -> ConfigValue {
    apply_sort(config)
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
