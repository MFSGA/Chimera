//! Deny-by-default classification of Mihomo runtime-config changes, and the
//! `GET /configs` projection used to verify a `PATCH`.
//!
//! Every field table here mirrors Mihomo's own config schema and its Clash
//! RESTful API surface, so nothing in this module generalizes to another core:
//! [`classify`] degrades every non-Mihomo kind to [`ConfigChange::Switch`]
//! (an identical config is still [`ConfigChange::Noop`]).
//! Other cores get their own sibling module on top of [`super::diff`].

use std::collections::BTreeSet;

use serde_yaml_ng::{Mapping, Value};

use crate::{
    Error, InstanceSpec,
    config::{
        clash,
        diff::{DiffEntry, collect_leaves, diff, value_at},
    },
    kind::CoreKind,
};

/// Mihomo's numeric inbound listeners. Zeroing them is what lets a bootstrap
/// epoch overlap the outgoing one instead of fighting it for ports.
const INBOUND_PORT_FIELDS: &[&str] = &[
    "port",
    "socks-port",
    "redir-port",
    "tproxy-port",
    "mixed-port",
];

const PATCH_FIELDS: &[&str] = &[
    "port",
    "socks-port",
    "redir-port",
    "tproxy-port",
    "mixed-port",
    "tun",
    "tuic-server",
    "ss-config",
    "vmess-config",
    "tcptun-config",
    "udptun-config",
    "allow-lan",
    "skip-auth-prefixes",
    "lan-allowed-ips",
    "lan-disallowed-ips",
    "bind-address",
    "mode",
    "log-level",
    "ipv6",
    "sniffing",
    "tcp-concurrent",
    "find-process-mode",
    "interface-name",
];

const TUN_PATCH_FIELDS: &[&str] = &[
    "enable",
    "device",
    "stack",
    "dns-hijack",
    "auto-route",
    "auto-detect-interface",
    "mtu",
    "gso",
    "gso-max-size",
    "inet6-address",
    "iproute2-table-index",
    "iproute2-rule-index",
    "auto-redirect",
    "auto-redirect-input-mark",
    "auto-redirect-output-mark",
    "auto-redirect-iproute2-fallback-rule-index",
    "loopback-address",
    "strict-route",
    "route-address",
    "route-address-set",
    "route-exclude-address",
    "route-exclude-address-set",
    "include-interface",
    "exclude-interface",
    "include-uid",
    "include-uid-range",
    "exclude-uid",
    "exclude-uid-range",
    "include-android-user",
    "include-package",
    "exclude-package",
    "include-mac-address",
    "exclude-mac-address",
    "endpoint-independent-nat",
    "udp-timeout",
    "icmp-timeout",
    "file-descriptor",
    "inet4-route-address",
    "inet6-route-address",
    "inet4-route-exclude-address",
    "inet6-route-exclude-address",
    "recvmsgx",
    "sendmsgx",
];

const TUIC_SERVER_PATCH_FIELDS: &[&str] = &[
    "enable",
    "listen",
    "token",
    "users",
    "certificate",
    "private-key",
    "congestion-controller",
    "max-idle-time",
    "authentication-timeout",
    "alpn",
    "max-udp-relay-packet-size",
    "cwnd",
    "bbr-profile",
];

const RELOAD_FIELDS: &[&str] = &[
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rule-providers",
    "providers",
    "rules",
    "hosts",
    "dns",
];

/// Zeroes Mihomo's inbound surface so a bootstrap epoch can run alongside the
/// outgoing one. Only reached through `prepare_bootstrap`, which the graceful
/// switch path gates on [`CoreKind::Mihomo`].
pub(super) fn zero_inbounds(document: &mut Mapping) {
    for key in INBOUND_PORT_FIELDS {
        zero_listener(document, key);
    }
    if let Some(tun) = document
        .get_mut(Value::String("tun".to_owned()))
        .and_then(Value::as_mapping_mut)
    {
        let enable = Value::String("enable".to_owned());
        if tun.get(&enable).and_then(Value::as_bool) == Some(true) {
            tun.insert(enable, Value::from(false));
        }
    }
}

fn zero_listener(document: &mut Mapping, key: &str) {
    let key = Value::String(key.to_owned());
    if document
        .get(&key)
        .and_then(Value::as_i64)
        .filter(|value| *value != 0)
        .is_some()
    {
        document.insert(key, Value::from(0));
    }
}

#[derive(Debug)]
pub(crate) enum ConfigChange {
    Noop,
    Patch {
        patch: Box<clash_api::ConfigPatch>,
        projection: RuntimeProjection,
    },
    Reload,
    Switch,
}

#[derive(Debug)]
pub(crate) struct RuntimeProjection {
    expected: Vec<(Vec<String>, Value)>,
}

impl RuntimeProjection {
    pub(crate) fn verify(&self, actual: &clash_api::RuntimeConfig) -> Result<bool, Error> {
        let actual = serde_yaml_ng::to_value(actual)?;
        Ok(self.expected.iter().all(|(path, expected)| {
            value_at(&actual, path).is_some_and(|actual| actual == expected)
        }))
    }
}

pub(crate) fn restoration_patch(
    bootstrap: &Mapping,
    desired: &Mapping,
) -> Result<Option<(Box<clash_api::ConfigPatch>, RuntimeProjection)>, Error> {
    match classify_documents(bootstrap, desired)? {
        ConfigChange::Noop => Ok(None),
        ConfigChange::Patch { patch, projection } => Ok(Some((patch, projection))),
        ConfigChange::Reload | ConfigChange::Switch => Err(Error::InvalidConfig(
            "graceful bootstrap cannot be restored losslessly with ConfigPatch".into(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlapBlock {
    DnsListen,
    InboundSurface,
}

/// One side of a classification: the two documents an epoch was planned from
/// and the process spec it would run under. Bundling them keeps each side
/// together instead of interleaving six references in which the four
/// `&Mapping` are mutually interchangeable and so are the two `&InstanceSpec`.
#[derive(Clone, Copy)]
pub(crate) struct ConfigClassificationView<'a> {
    pub source: &'a Mapping,
    pub effective: &'a Mapping,
    pub spec: &'a InstanceSpec,
}

pub(crate) fn classify(
    current: ConfigClassificationView<'_>,
    desired: ConfigClassificationView<'_>,
) -> Result<ConfigChange, Error> {
    if process_spec_changed(current.spec, desired.spec) {
        return Ok(ConfigChange::Switch);
    }

    let source_diff = diff(current.source, desired.source);
    if desired.spec.core.kind != CoreKind::Mihomo {
        // Non-mihomo kinds degrade to Switch. An identical config is still a
        // Noop — but only when the *effective* documents also match: a
        // capability change (e.g. the binary was replaced in place and now
        // resolves a different version) can alter the derived config and the
        // resolved controller without any source edit, and that must restart.
        let unchanged =
            source_diff.is_empty() && diff(current.effective, desired.effective).is_empty();
        return Ok(if unchanged {
            ConfigChange::Noop
        } else {
            ConfigChange::Switch
        });
    }

    if source_diff.iter().any(|entry| {
        entry
            .path
            .first()
            .is_some_and(|root| clash::CONTROLLER_FIELDS.contains(&root.as_str()))
            || is_dns_listen(&entry.path)
    }) {
        return Ok(ConfigChange::Switch);
    }
    classify_documents(current.effective, desired.effective)
}

fn process_spec_changed(current: &InstanceSpec, desired: &InstanceSpec) -> bool {
    current.core.kind != desired.core.kind
        || current.core.binary_path != desired.core.binary_path
        || current.core.version != desired.core.version
        || current.core.features != desired.core.features
        || current.working_dir != desired.working_dir
        || current.options.startup_timeout != desired.options.startup_timeout
        || current.options.health != desired.options.health
        || current.options.restart_policy != desired.options.restart_policy
        || current.options.backoff != desired.options.backoff
}

fn classify_documents(current: &Mapping, desired: &Mapping) -> Result<ConfigChange, Error> {
    if dns_listen(current) != dns_listen(desired) {
        return Ok(ConfigChange::Switch);
    }
    let changes = diff(current, desired);
    if changes.is_empty() {
        return Ok(ConfigChange::Noop);
    }

    let patchable = changes.iter().all(|entry| {
        entry.new.is_some()
            && entry.path.first().is_some_and(|root| {
                PATCH_FIELDS.contains(&root.as_str()) && patch_nested_path_is_supported(&entry.path)
            })
    });
    if patchable {
        return build_patch(desired, &changes);
    }

    let reloadable = changes.iter().all(|entry| {
        entry.path.first().is_some_and(|root| {
            RELOAD_FIELDS.contains(&root.as_str())
                && !is_dns_listen(&entry.path)
                && !(root == "dns" && entry.path.len() == 1)
        })
    });
    Ok(if reloadable {
        ConfigChange::Reload
    } else {
        ConfigChange::Switch
    })
}

fn patch_nested_path_is_supported(path: &[String]) -> bool {
    match path.first().map(String::as_str) {
        Some("tun") => path
            .get(1)
            .is_some_and(|field| TUN_PATCH_FIELDS.contains(&field.as_str())),
        Some("tuic-server") => path
            .get(1)
            .is_some_and(|field| TUIC_SERVER_PATCH_FIELDS.contains(&field.as_str())),
        Some(_) => path.len() == 1,
        None => false,
    }
}

fn build_patch(desired: &Mapping, changes: &[DiffEntry]) -> Result<ConfigChange, Error> {
    let roots: BTreeSet<&str> = changes
        .iter()
        .filter_map(|entry| entry.path.first().map(String::as_str))
        .collect();
    let mut document = Mapping::new();
    for root in roots {
        let key = Value::String(root.to_owned());
        let Some(value) = desired.get(&key) else {
            return Ok(ConfigChange::Switch);
        };
        let value = match root {
            "tun" => filter_mapping(value, TUN_PATCH_FIELDS)?,
            "tuic-server" => filter_mapping(value, TUIC_SERVER_PATCH_FIELDS)?,
            _ => value.clone(),
        };
        document.insert(key, value);
    }
    for required_enable in ["tun", "tuic-server"] {
        if document.contains_key(Value::String(required_enable.to_owned()))
            && document
                .get(Value::String(required_enable.to_owned()))
                .and_then(Value::as_mapping)
                .and_then(|mapping| mapping.get(Value::String("enable".to_owned())))
                .and_then(Value::as_bool)
                .is_none()
        {
            return Ok(ConfigChange::Switch);
        }
    }
    let Ok(patch) = serde_yaml_ng::from_value::<clash_api::ConfigPatch>(Value::Mapping(document))
    else {
        return Ok(ConfigChange::Switch);
    };
    let serialized = serde_yaml_ng::to_value(&patch)?;
    let mut expected = Vec::new();
    collect_leaves(&serialized, &mut Vec::new(), &mut expected);
    Ok(ConfigChange::Patch {
        patch: Box::new(patch),
        projection: RuntimeProjection { expected },
    })
}

fn filter_mapping(value: &Value, allowed: &[&str]) -> Result<Value, Error> {
    let mapping = value
        .as_mapping()
        .ok_or_else(|| Error::InvalidConfig("patchable nested config must be a mapping".into()))?;
    let mut filtered = Mapping::new();
    for (key, value) in mapping {
        let Some(key_text) = key.as_str() else {
            return Err(Error::InvalidConfig("config keys must be strings".into()));
        };
        if allowed.contains(&key_text) {
            filtered.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Mapping(filtered))
}

fn is_dns_listen(path: &[String]) -> bool {
    path.first().is_some_and(|value| value == "dns")
        && path.get(1).is_some_and(|value| value == "listen")
}

fn dns_listen(document: &Mapping) -> Option<&Value> {
    document
        .get(Value::String("dns".into()))
        .and_then(Value::as_mapping)
        .and_then(|dns| dns.get(Value::String("listen".into())))
}

pub(crate) fn overlap_block(document: &Mapping) -> Option<OverlapBlock> {
    if let Some(listen) = document
        .get(Value::String("dns".into()))
        .and_then(Value::as_mapping)
        .and_then(|dns| dns.get(Value::String("listen".into())))
    {
        return match listen.as_str() {
            Some("") => None,
            Some(_) => Some(OverlapBlock::DnsListen),
            None if listen.is_null() => None,
            None => Some(OverlapBlock::InboundSurface),
        };
    }
    for key in INBOUND_PORT_FIELDS {
        if document
            .get(Value::String((*key).into()))
            .is_some_and(|value| value.as_i64().is_none())
        {
            return Some(OverlapBlock::InboundSurface);
        }
    }
    if let Some(tun) = document.get(Value::String("tun".into())) {
        let Some(tun) = tun.as_mapping() else {
            return Some(OverlapBlock::InboundSurface);
        };
        if tun
            .get(Value::String("enable".into()))
            .is_some_and(|enable| enable.as_bool().is_none())
        {
            return Some(OverlapBlock::InboundSurface);
        }
    }
    if let Some(tuic) = document.get(Value::String("tuic-server".into())) {
        let Some(tuic) = tuic.as_mapping() else {
            return Some(OverlapBlock::InboundSurface);
        };
        match tuic.get(Value::String("enable".into())) {
            Some(enable) if enable.as_bool() == Some(false) => {}
            None if tuic.is_empty() => {}
            Some(_) | None => return Some(OverlapBlock::InboundSurface),
        }
    }
    for key in [
        "ss-config",
        "vmess-config",
        "tcptun-config",
        "udptun-config",
    ] {
        if document
            .get(Value::String(key.into()))
            .is_some_and(|value| value.as_str() != Some(""))
        {
            return Some(OverlapBlock::InboundSurface);
        }
    }
    for key in ["listeners", "tunnels"] {
        if document
            .get(Value::String(key.into()))
            .is_some_and(nonempty_collection)
        {
            return Some(OverlapBlock::InboundSurface);
        }
    }
    for key in document.keys().filter_map(Value::as_str) {
        let inbound_like = key.ends_with("-port")
            || key.contains("listener")
            || key.contains("inbound")
            || key.contains("tunnel");
        if inbound_like && !PATCH_FIELDS.contains(&key) && !matches!(key, "listeners" | "tunnels") {
            return Some(OverlapBlock::InboundSurface);
        }
    }
    None
}

fn nonempty_collection(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Sequence(value) => !value.is_empty(),
        Value::Mapping(value) => !value.is_empty(),
        Value::Bool(_) | Value::Number(_) | Value::Tagged(_) => true,
    }
}
