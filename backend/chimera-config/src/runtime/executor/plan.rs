//! The runtime config keys post-processing decides, derived from the
//! settings. Every key Chimera writes, removes or fills in is listed here;
//! the steps only walk these trees.

use std::sync::Arc;

use crate::runtime::{
    post_process::{OwnedFields, Ownership, OwnershipEntries},
    value::{ConfigObject, ConfigValue},
};

use super::{GuardInputs, TunFlavor, TunParams};

/// Every key post-processing decides: the guard's, then `tun` with its DNS
/// takeover, then the `profile` cache.
pub(super) fn ownership(guard: &GuardInputs<'_>, tun: &TunParams) -> OwnershipEntries {
    let mut entries = guard_ownership(guard);
    entries.extend(tun_ownership(tun));
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

/// `tun` and the DNS takeover it needs. While TUN is off, a `tun` the
/// profiles wrote is turned off and none is created.
pub(super) fn tun_ownership(tun: &TunParams) -> OwnershipEntries {
    let mut entries = Vec::with_capacity(2);
    if tun.enable {
        entries.push((
            "tun",
            Ownership::Table {
                entries: tun_entries(tun),
                create: true,
            },
        ));
        entries.push((
            "dns",
            Ownership::Table {
                entries: dns_entries(tun.windows_fake_ip_filter, tun.flavor),
                create: true,
            },
        ));
    } else {
        entries.push((
            "tun",
            Ownership::Table {
                entries: vec![("enable", Ownership::Forced(ConfigValue::Bool(false)))],
                create: false,
            },
        ));
    }
    entries
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
    entries.push(cache_ownership());
    entries
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
        TunFlavor::Standard { stack } => {
            entries.push(("stack", Ownership::Forced(string(stack.as_ref()))));
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
