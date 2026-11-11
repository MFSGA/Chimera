//! Keys that the Chimera runtime owns, derived from typed settings.
//!
//! The pure plan is independent of a running Core. Unlike the reference,
//! Chimera retains optional listener ports (an absent override does not remove
//! a profile listener) and its separate ChimeraClient TUN contract.

use std::sync::Arc;

use crate::runtime::{
    post_process::{OwnedFields, Ownership, OwnershipEntries},
    value::{ConfigObject, ConfigValue},
};

use super::{GuardInputs, TunFlavor, TunParams};

/// Keep guard/ports and TUN ownership as ordered, separate operations: the
/// existing Chimera pipeline applies guard settings before restoring TUN keys.
/// Combining both inheritance passes would reorder keys seen by built-in
/// transforms even when the final sorted YAML remains the same.
pub(super) struct RuntimePlan {
    pub guard: OwnershipEntries,
    pub tun: OwnershipEntries,
    /// Applied only after built-in transforms; preserves existing TUN/DNS
    /// fallback values and the profile cache's create-if-absent semantics.
    pub final_defaults: OwnershipEntries,
}

pub(super) fn ownership(guard: &GuardInputs<'_>, tun: &TunParams) -> RuntimePlan {
    RuntimePlan {
        guard: guard_ownership(guard),
        tun: tun_ownership(tun),
        final_defaults: final_defaults_ownership(tun),
    }
}

pub(super) fn guard_ownership(guard: &GuardInputs<'_>) -> OwnershipEntries {
    let mut entries = guard.overrides.ownership_entries();
    entries.push(("mixed-port", number(guard.ports.mixed_port)));
    for (field, port) in [
        ("port", guard.ports.port),
        ("socks-port", guard.ports.socks_port),
        ("redir-port", guard.ports.redir_port),
        ("tproxy-port", guard.ports.tproxy_port),
    ] {
        if let Some(port) = port {
            entries.push((field, number(port)));
        }
    }
    if let Some(mark) = guard.routing_mark {
        entries.push((
            "routing-mark",
            Ownership::Forced(ConfigValue::Number(serde_json::Number::from(mark))),
        ));
    }
    if let Some(controller) = &guard.ports.external_controller {
        entries.push((
            "external-controller",
            Ownership::Forced(ConfigValue::String(Arc::from(controller.as_str()))),
        ));
    }
    entries
}

pub(super) fn tun_ownership(tun: &TunParams) -> OwnershipEntries {
    let mut entries = vec![("enable", Ownership::Forced(ConfigValue::Bool(tun.enable)))];
    if tun.enable && !matches!(tun.flavor, TunFlavor::ChimeraClient) {
        entries.extend(tun.overrides.ownership_entries());
    }
    vec![(
        "tun",
        Ownership::Table {
            entries,
            create: tun.enable,
        },
    )]
}

/// Post-builtin ownership. These entries intentionally use the existing
/// Chimera ordering: TUN, then DNS takeover (only while enabled), then the
/// profile cache. Unmanaged profile keys remain untouched at every level.
pub(super) fn final_defaults_ownership(tun: &TunParams) -> OwnershipEntries {
    let mut entries = Vec::with_capacity(3);
    let mut tun_fields = vec![("enable", forced(ConfigValue::Bool(tun.enable)))];
    if tun.enable {
        match tun.flavor {
            TunFlavor::ClashRs => {
                tun_fields.push(("device-id", default(string("dev://utun1989"))));
            }
            TunFlavor::ChimeraClient => {
                tun_fields.extend([
                    ("device-id", default(string("dev://utun1989"))),
                    ("route-all", default(ConfigValue::Bool(true))),
                    ("dns-hijack", default(ConfigValue::Bool(true))),
                    (
                        "so-mark",
                        default(ConfigValue::Number(serde_json::Number::from(7777))),
                    ),
                ]);
            }
            TunFlavor::Standard { stack } => {
                tun_fields.extend([
                    ("stack", default(string(stack.as_ref()))),
                    ("dns-hijack", default(strings(&["any:53"]))),
                    ("auto-detect-interface", default(ConfigValue::Bool(true))),
                ]);
            }
        }
    }
    entries.push((
        "tun",
        Ownership::Table {
            entries: tun_fields,
            create: tun.enable,
        },
    ));
    if tun.enable {
        let mut dns_fields = vec![
            ("enable", forced(ConfigValue::Bool(true))),
            ("enhanced-mode", default(string("fake-ip"))),
        ];
        if tun.flavor == TunFlavor::ChimeraClient {
            dns_fields.extend([
                (
                    "nameserver",
                    default(strings(&[
                        "https://dns.alidns.com/dns-query",
                        "114.114.114.114",
                        "223.5.5.5",
                        "8.8.8.8",
                    ])),
                ),
                (
                    "default-nameserver",
                    default(strings(&["114.114.114.114", "1.1.1.1", "8.8.8.8"])),
                ),
            ]);
        } else {
            dns_fields.extend([
                ("fake-ip-range", default(string("198.18.0.1/16"))),
                (
                    "nameserver",
                    default(strings(&["114.114.114.114", "223.5.5.5", "8.8.8.8"])),
                ),
                ("fallback", default(strings(&[]))),
            ]);
        }
        if tun.windows_fake_ip_filter {
            dns_fields.push((
                "fake-ip-filter",
                default(strings(&[
                    "dns.msftncsi.com",
                    "www.msftncsi.com",
                    "www.msftconnecttest.com",
                ])),
            ));
        }
        entries.push((
            "dns",
            Ownership::Table {
                entries: dns_fields,
                create: true,
            },
        ));
    }
    entries.push((
        "profile",
        default(object([
            ("store-selected", ConfigValue::Bool(true)),
            ("store-fake-ip", ConfigValue::Bool(false)),
        ])),
    ));
    entries
}

fn forced(value: ConfigValue) -> Ownership {
    Ownership::Forced(value)
}

fn default(value: ConfigValue) -> Ownership {
    Ownership::Default(value)
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
    let values: ConfigObject = entries
        .into_iter()
        .map(|(key, value)| (Arc::from(key), value))
        .collect();
    ConfigValue::Object(Arc::new(values))
}

fn number(port: u16) -> Ownership {
    Ownership::Forced(ConfigValue::Number(serde_json::Number::from(port)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::ResolvedPortBindings;
    use super::*;
    use crate::clash::config::{
        overrides::{ClashGuardOverrides, ManageableField, TunOverrides},
        tun_stack::TunStack,
    };

    #[test]
    fn optional_listeners_and_routing_mark_keep_chimera_contract() {
        let overrides = ClashGuardOverrides::default();
        let guard = GuardInputs {
            overrides: &overrides,
            ports: ResolvedPortBindings {
                mixed_port: 7890,
                socks_port: Some(7891),
                ..Default::default()
            },
            routing_mark: Some(7777),
        };
        let entries = guard_ownership(&guard);
        assert!(
            entries
                .iter()
                .any(|(key, entry)| *key == "mixed-port" && *entry == number(7890))
        );
        assert!(
            entries
                .iter()
                .any(|(key, entry)| *key == "socks-port" && *entry == number(7891))
        );
        assert!(entries.iter().any(|(key, entry)| *key == "routing-mark"
            && *entry == Ownership::Forced(ConfigValue::from_json_value(json!(7777)))));
        for key in ["port", "redir-port", "tproxy-port", "external-controller"] {
            assert!(
                !entries.iter().any(|(field, _)| *field == key),
                "unset {key} must not be removed"
            );
        }
    }

    #[test]
    fn only_mihomo_family_tun_owns_auto_route_while_enabled() {
        let tun = TunParams {
            enable: true,
            overrides: TunOverrides {
                auto_route: ManageableField::Unmanaged,
            },
            flavor: TunFlavor::Standard {
                stack: TunStack::Gvisor,
            },
            windows_fake_ip_filter: false,
        };
        let entries = tun_ownership(&tun);
        let Ownership::Table { entries, create } = &entries[0].1 else {
            panic!("TUN must own a table");
        };
        assert!(*create);
        assert!(entries.contains(&("auto-route", Ownership::Inherited)));

        let client = TunParams {
            flavor: TunFlavor::ChimeraClient,
            ..tun
        };
        let Ownership::Table { entries, .. } = &tun_ownership(&client)[0].1 else {
            panic!()
        };
        assert!(!entries.iter().any(|(field, _)| *field == "auto-route"));
        let off = TunParams {
            enable: false,
            ..tun
        };
        let Ownership::Table { entries, create } = &tun_ownership(&off)[0].1 else {
            panic!()
        };
        assert!(!create);
        assert_eq!(
            entries,
            &vec![("enable", Ownership::Forced(ConfigValue::Bool(false)))]
        );
    }
}
