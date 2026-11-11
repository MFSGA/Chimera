//! Keys that the Chimera runtime owns, derived from typed settings.
//!
//! The pure plan is independent of a running Core. Unlike the reference,
//! Chimera retains optional listener ports (an absent override does not remove
//! a profile listener) and its separate ChimeraClient TUN contract.

use std::sync::Arc;

use crate::runtime::{
    post_process::{OwnedFields, Ownership, OwnershipEntries},
    value::ConfigValue,
};

use super::{GuardInputs, TunFlavor, TunParams};

pub(super) fn ownership(guard: &GuardInputs<'_>, tun: &TunParams) -> OwnershipEntries {
    let mut entries = guard_ownership(guard);
    entries.extend(tun_ownership(tun));
    entries
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
