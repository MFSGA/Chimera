//! The runtime config keys post-processing decides, derived from the
//! settings. Every key Chimera writes, removes or fills in is listed here;
//! the steps only walk these trees.

use std::sync::Arc;

use crate::{
    clash::{config::tun_stack::TunStack, features::TunKey},
    runtime::{
        post_process::{OwnedFields, Ownership, OwnershipEntries},
        value::{ConfigObject, ConfigValue},
    },
};

use super::{DnsParams, GuardInputs, StepLogEntry, TunFlavor, TunParams, TunStackWrite};

/// Profile DNS resolver keys that a Chimera-owned table replaces.
const PROFILE_RESOLVER_KEYS: [&str; 3] = [
    "nameserver-policy",
    "proxy-server-nameserver",
    "proxy-server-nameserver-policy",
];

/// Every key post-processing decides: the guard's, then `tun`, `dns`,
/// `hosts`, and the `profile` cache.
pub(super) fn ownership(
    guard: &GuardInputs<'_>,
    tun: &TunParams,
    dns: &DnsParams,
) -> OwnershipEntries {
    let mut entries = guard_ownership(guard);
    entries.extend(tun_ownership(tun));
    entries.extend(dns_ownership(tun, dns));
    entries.extend(hosts_ownership(dns));
    entries.push(cache_ownership());
    entries
}

/// The top-level keys the guard owns: the typed overrides, then the resolved
/// ports. Chimera follows ref and owns `port` and `socks-port` like `mixed-port`: an unset
/// one is removed, so a profile cannot open a listener the user turned off or
/// take the `mixed-port` before the mixed listener binds (#5678).
pub(super) fn guard_ownership(guard: &GuardInputs<'_>) -> OwnershipEntries {
    let mut entries = guard.overrides.ownership_entries();
    entries.push((
        "mixed-port",
        Ownership::Forced(number(guard.ports.mixed_port)),
    ));
    for (key, port) in [
        ("port", guard.ports.port),
        ("socks-port", guard.ports.socks_port),
    ] {
        entries.push((
            key,
            port.map_or(Ownership::Removed, |port| Ownership::Forced(number(port))),
        ));
    }
    for (key, port) in [
        ("redir-port", guard.ports.redir_port),
        ("tproxy-port", guard.ports.tproxy_port),
    ] {
        if let Some(port) = port {
            entries.push((key, Ownership::Forced(number(port))));
        }
    }
    if let Some(mark) = guard.routing_mark {
        entries.push((
            "routing-mark",
            Ownership::Forced(ConfigValue::Number(mark.into())),
        ));
    }
    if let Some(controller) = &guard.ports.external_controller {
        entries.push(("external-controller", Ownership::Forced(string(controller))));
    }
    entries
}

/// `tun`. While TUN is off, a `tun` the profiles wrote is turned off and none
/// is created.
pub(super) fn tun_ownership(tun: &TunParams) -> OwnershipEntries {
    let fields = if tun.enable {
        tun_entries(tun)
    } else {
        vec![("enable", Ownership::Forced(ConfigValue::Bool(false)))]
    };
    vec![(
        "tun",
        Ownership::Table {
            entries: fields,
            create: tun.enable,
        },
    )]
}

/// DNS replaces the profile table when Chimera owns it. TUN answers DNS
/// through the core, so DNS is enabled while TUN is active and only missing
/// fallback keys are supplied when DNS remains profile-owned.
pub(super) fn dns_ownership(tun: &TunParams, dns: &DnsParams) -> OwnershipEntries {
    match (&dns.dns, tun.enable) {
        (Some(table), true) => vec![("dns", Ownership::Forced(with_dns_enabled(table)))],
        (Some(table), false) => vec![("dns", Ownership::Forced(table.clone()))],
        (None, true) => vec![(
            "dns",
            Ownership::Table {
                entries: dns_entries(tun.windows_fake_ip_filter, tun.flavor),
                create: true,
            },
        )],
        (None, false) => Vec::new(),
    }
}

/// Hosts replaces the profile table when Chimera owns it; an empty table
/// removes the inherited profile entries.
pub(super) fn hosts_ownership(dns: &DnsParams) -> OwnershipEntries {
    match &dns.hosts {
        Some(table) if table.as_object_arc().is_some_and(|map| map.is_empty()) => {
            vec![("hosts", Ownership::Removed)]
        }
        Some(table) => vec![("hosts", Ownership::Forced(table.clone()))],
        None => Vec::new(),
    }
}

fn with_dns_enabled(table: &ConfigValue) -> ConfigValue {
    let mut map = table
        .as_object_arc()
        .map(|map| (**map).clone())
        .unwrap_or_default();
    map.insert(Arc::from("enable"), ConfigValue::Bool(true));
    ConfigValue::Object(Arc::new(map))
}

/// The `profile` cache. An existing `profile` is kept whole; its keys are not
/// filled in.
pub(super) fn cache_ownership() -> (&'static str, Ownership) {
    (
        "profile",
        Ownership::Default(object([
            ("store-selected", ConfigValue::Bool(true)),
            ("store-fake-ip", ConfigValue::Bool(false)),
        ])),
    )
}

// Compatibility oracle used only by the older finalizer unit tests. The
// production pipeline uses ownership() once before built-in transforms.
#[cfg(test)]
pub(super) fn final_defaults_ownership(tun: &TunParams) -> OwnershipEntries {
    let mut entries = tun_ownership(tun);
    entries.extend(dns_ownership(tun, &DnsParams::default()));
    entries.push(cache_ownership());
    entries
}

/// Warnings are computed from the pre-owned candidate so they can describe
/// profile values that the managed settings step will replace.
pub(super) fn managed_settings_logs(
    tun: &TunParams,
    dns: &DnsParams,
    config: &ConfigValue,
) -> Vec<StepLogEntry> {
    let mut logs: Vec<_> = dns
        .warnings
        .iter()
        .cloned()
        .map(StepLogEntry::warn)
        .collect();
    if let TunFlavor::Standard {
        stack: TunStackWrite::Downgraded { selected, written },
        ..
    } = tun.flavor
        && tun.enable
    {
        logs.push(StepLogEntry::warn(format!(
            "the selected core does not support the TUN stack `{}`; `{}` is used instead",
            selected.as_ref(),
            written.as_ref(),
        )));
    }
    if let Some(table) = &dns.dns {
        let disabled = table
            .as_object_arc()
            .and_then(|map| map.get("enable"))
            .is_some_and(|enable| *enable == ConfigValue::Bool(false));
        if tun.enable && disabled {
            logs.push(StepLogEntry::warn(
                "TUN mode answers DNS through the core, so `dns.enable` is written as true",
            ));
        }
        let replaced: Vec<String> = config
            .as_object_arc()
            .and_then(|root| root.get("dns"))
            .and_then(ConfigValue::as_object_arc)
            .map(|profile_dns| {
                PROFILE_RESOLVER_KEYS
                    .into_iter()
                    .filter(|key| profile_dns.get(*key).is_some_and(|value| !is_empty(value)))
                    .map(|key| format!("`{key}`"))
                    .collect()
            })
            .unwrap_or_default();
        if !replaced.is_empty() {
            logs.push(StepLogEntry::warn(format!(
                "Chimera's DNS settings replace the profile's `dns`, including its {}",
                replaced.join(", ")
            )));
        }
    }
    logs
}

fn is_empty(value: &ConfigValue) -> bool {
    match value {
        ConfigValue::Null => true,
        ConfigValue::Array(items) => items.is_empty(),
        ConfigValue::Object(map) => map.is_empty(),
        _ => false,
    }
}

fn tun_entries(tun: &TunParams) -> OwnershipEntries {
    let mut entries = vec![("enable", Ownership::Forced(ConfigValue::Bool(true)))];
    if !matches!(tun.flavor, TunFlavor::ChimeraClient) {
        entries.extend(tun.overrides.ownership_entries());
    }
    match tun.flavor {
        TunFlavor::ClashRs => {
            entries.push(("device-id", Ownership::Default(string("dev://utun1989"))));
        }
        TunFlavor::ChimeraClient => {
            entries.extend([
                ("device-id", Ownership::Default(string("dev://utun1989"))),
                ("route-all", Ownership::Default(ConfigValue::Bool(true))),
                ("dns-hijack", Ownership::Default(ConfigValue::Bool(true))),
                (
                    "so-mark",
                    Ownership::Default(ConfigValue::Number(7777.into())),
                ),
            ]);
        }
        TunFlavor::Standard {
            stack,
            congestion_controller,
        } => {
            // The stack is Nyanpasu's: providers write their configs for
            // mihomo, so a profile's stack may not suit the selected core.
            match stack {
                TunStackWrite::Omitted => {}
                TunStackWrite::Selected(stack)
                | TunStackWrite::Downgraded { written: stack, .. } => entries.push((
                    TunKey::Stack.as_str(),
                    Ownership::Forced(string(stack.as_ref())),
                )),
            }
            match (stack, congestion_controller) {
                (_, Some(controller)) => entries.push((
                    TunKey::CongestionController.as_str(),
                    Ownership::Forced(string(controller.as_ref())),
                )),
                (TunStackWrite::Selected(TunStack::Mips), None) => {
                    entries.push((TunKey::CongestionController.as_str(), Ownership::Removed));
                }
                _ => {}
            }
            entries.extend([
                ("dns-hijack", Ownership::Default(strings(&["any:53"]))),
                (
                    "auto-detect-interface",
                    Ownership::Default(ConfigValue::Bool(true)),
                ),
            ]);
        }
    }
    entries
}

fn dns_entries(windows_fake_ip_filter: bool, flavor: TunFlavor) -> OwnershipEntries {
    let mut entries = vec![
        ("enable", Ownership::Forced(ConfigValue::Bool(true))),
        ("enhanced-mode", Ownership::Default(string("fake-ip"))),
    ];
    if matches!(flavor, TunFlavor::ChimeraClient) {
        entries.extend([
            (
                "nameserver",
                Ownership::Default(strings(&[
                    "https://dns.alidns.com/dns-query",
                    "114.114.114.114",
                    "223.5.5.5",
                    "8.8.8.8",
                ])),
            ),
            (
                "default-nameserver",
                Ownership::Default(strings(&["114.114.114.114", "1.1.1.1", "8.8.8.8"])),
            ),
        ]);
    } else {
        entries.extend([
            ("fake-ip-range", Ownership::Default(string("198.18.0.1/16"))),
            (
                "nameserver",
                Ownership::Default(strings(&["114.114.114.114", "223.5.5.5", "8.8.8.8"])),
            ),
            ("fallback", Ownership::Default(strings(&[]))),
        ]);
    }
    if windows_fake_ip_filter {
        entries.push((
            "fake-ip-filter",
            Ownership::Default(strings(&[
                "dns.msftncsi.com",
                "www.msftncsi.com",
                "www.msftconnecttest.com",
            ])),
        ));
    }
    entries
}

fn number(value: u16) -> ConfigValue {
    ConfigValue::Number(serde_json::Number::from(value))
}

fn string(text: &str) -> ConfigValue {
    ConfigValue::String(Arc::from(text))
}

fn strings(items: &[&str]) -> ConfigValue {
    ConfigValue::Array(Arc::from(
        items.iter().map(|item| string(item)).collect::<Vec<_>>(),
    ))
}

fn object<const N: usize>(entries: [(&str, ConfigValue); N]) -> ConfigValue {
    let map: ConfigObject = entries
        .into_iter()
        .map(|(key, value)| (Arc::from(key), value))
        .collect();
    ConfigValue::Object(Arc::new(map))
}
