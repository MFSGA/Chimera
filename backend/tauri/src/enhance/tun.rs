use chimera_config::clash::config::{ClashConfig, tun_stack::TunStack};
use serde_yaml::{Mapping, Value};

use crate::config::chimera::ClashCore;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TunParams {
    pub enable: bool,
    pub flavor: TunFlavor,
    pub windows_fake_ip_filter: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TunFlavor {
    ClashRs,
    /// ChimeraClient intentionally retains its legacy clash-rs-compatible TUN fields.
    ChimeraClient,
    Standard {
        stack: TunStack,
    },
}

pub(super) fn params_for(clash: &ClashConfig, core: ClashCore) -> TunParams {
    TunParams {
        enable: clash.enable_tun_mode,
        flavor: derive_tun_flavor(core, clash.tun_stack),
        windows_fake_ip_filter: cfg!(target_os = "windows"),
    }
}

/// Mirrors the upstream typed runtime builder: only stable ClashRs uses the
/// ClashRs-specific branch; ClashRsAlpha follows the standard path. Premium
/// cannot use the mixed stack, so it falls back to gVisor.
pub(super) fn derive_tun_flavor(core: ClashCore, stack: TunStack) -> TunFlavor {
    match core {
        ClashCore::ClashRs => TunFlavor::ClashRs,
        ClashCore::ChimeraClient => TunFlavor::ChimeraClient,
        _ => TunFlavor::Standard {
            stack: if core == ClashCore::ClashPremium && stack == TunStack::Mixed {
                TunStack::Gvisor
            } else {
                stack
            },
        },
    }
}

fn revise(map: &mut Mapping, key: &str, value: impl Into<Value>) {
    map.insert(Value::String(key.into()), value.into());
}

fn append(map: &mut Mapping, key: &str, value: impl Into<Value>) {
    let key = Value::String(key.into());
    if !map.contains_key(&key) {
        map.insert(key, value.into());
    }
}

#[tracing_attributes::instrument(skip(config))]
pub(super) fn use_tun(mut config: Mapping, params: TunParams) -> Mapping {
    let tun_key = Value::from("tun");
    let tun_value = config.get(&tun_key);
    tracing::debug!("tun_val: {:?}", tun_value);
    if !params.enable && tun_value.is_none() {
        return config;
    }

    let mut tun = tun_value
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();

    revise(&mut tun, "enable", params.enable);
    if params.enable {
        match params.flavor {
            TunFlavor::ClashRs => {
                append(&mut tun, "device-id", "dev://utun1989");
                append(&mut tun, "auto-route", true);
            }
            TunFlavor::ChimeraClient => {
                append(&mut tun, "device-id", "dev://utun1989");
                append(&mut tun, "route-all", true);
                append(&mut tun, "dns-hijack", true);
                // ChimeraClient retains the existing Linux routing mark contract.
                append(&mut tun, "so-mark", 7777);
            }
            TunFlavor::Standard { stack } => {
                append(&mut tun, "stack", AsRef::<str>::as_ref(&stack));
                append(&mut tun, "dns-hijack", vec!["any:53"]);
                append(&mut tun, "auto-route", true);
                append(&mut tun, "auto-detect-interface", true);
            }
        }
    }

    revise(&mut config, "tun", tun);

    if params.enable {
        use_dns_for_tun(config, params.windows_fake_ip_filter)
    } else {
        config
    }
}

fn use_dns_for_tun(mut config: Mapping, windows_fake_ip_filter: bool) -> Mapping {
    let dns_key = Value::from("dns");
    let dns_value = config.get(&dns_key);

    let mut dns = dns_value
        .and_then(Value::as_mapping)
        .cloned()
        .unwrap_or_default();

    revise(&mut dns, "enable", true);
    append(&mut dns, "enhanced-mode", "fake-ip");
    // Retained Chimera difference: do not overwrite/seed fake-ip-range so users can
    // keep a custom range without the runtime builder inventing a competing default.
    append(
        &mut dns,
        "nameserver",
        vec![
            "https://dns.alidns.com/dns-query",
            "114.114.114.114",
            "223.5.5.5",
            "8.8.8.8",
        ],
    );
    append(
        &mut dns,
        "default-nameserver",
        vec!["114.114.114.114", "1.1.1.1", "8.8.8.8"],
    );

    if windows_fake_ip_filter {
        append(
            &mut dns,
            "fake-ip-filter",
            vec![
                "dns.msftncsi.com",
                "www.msftncsi.com",
                "www.msftconnecttest.com",
            ],
        );
    }

    revise(&mut config, "dns", dns);
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(yaml: &str) -> Mapping {
        serde_yaml::from_str::<Value>(yaml)
            .unwrap()
            .as_mapping()
            .cloned()
            .unwrap()
    }

    fn tun_params(enable: bool, flavor: TunFlavor) -> TunParams {
        TunParams {
            enable,
            flavor,
            windows_fake_ip_filter: true,
        }
    }

    fn value_at<'a>(config: &'a Mapping, root: &str, key: &str) -> Option<&'a Value> {
        config
            .get(Value::String(root.into()))
            .and_then(Value::as_mapping)
            .and_then(|mapping| mapping.get(Value::String(key.into())))
    }

    #[test]
    fn disabled_without_existing_tun_is_untouched() {
        let config = mapping("proxies: []\n");
        let result = use_tun(
            config.clone(),
            tun_params(
                false,
                TunFlavor::Standard {
                    stack: TunStack::System,
                },
            ),
        );
        assert_eq!(result, config);
    }

    #[test]
    fn disabled_existing_tun_forces_enable_false_without_dns_changes() {
        let result = use_tun(
            mapping("tun:\n  enable: true\n  stack: system\n"),
            tun_params(
                false,
                TunFlavor::Standard {
                    stack: TunStack::System,
                },
            ),
        );
        assert_eq!(
            value_at(&result, "tun", "enable"),
            Some(&Value::Bool(false))
        );
        assert_eq!(
            value_at(&result, "tun", "stack").and_then(Value::as_str),
            Some("system")
        );
        assert!(!result.contains_key(Value::String("dns".into())));
    }

    #[test]
    fn standard_tun_appends_defaults_but_preserves_user_values() {
        let result = use_tun(
            mapping(
                "tun:\n  stack: system\ndns:\n  nameserver:\n    - 1.1.1.1\n  fake-ip-range: 198.19.0.1/16\n",
            ),
            tun_params(
                true,
                TunFlavor::Standard {
                    stack: TunStack::Gvisor,
                },
            ),
        );

        assert_eq!(value_at(&result, "tun", "enable"), Some(&Value::Bool(true)));
        assert_eq!(
            value_at(&result, "tun", "stack").and_then(Value::as_str),
            Some("system")
        );
        assert_eq!(
            value_at(&result, "tun", "auto-route"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            value_at(&result, "tun", "auto-detect-interface"),
            Some(&Value::Bool(true))
        );
        assert_eq!(value_at(&result, "dns", "enable"), Some(&Value::Bool(true)));
        assert_eq!(
            value_at(&result, "dns", "fake-ip-range").and_then(Value::as_str),
            Some("198.19.0.1/16")
        );
        let nameservers = value_at(&result, "dns", "nameserver")
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(nameservers, &vec![Value::String("1.1.1.1".into())]);
        assert!(value_at(&result, "dns", "fake-ip-filter").is_some());
    }

    #[test]
    fn clash_rs_uses_upstream_device_branch() {
        let result = use_tun(Mapping::new(), tun_params(true, TunFlavor::ClashRs));
        assert_eq!(
            value_at(&result, "tun", "device-id").and_then(Value::as_str),
            Some("dev://utun1989")
        );
        assert_eq!(
            value_at(&result, "tun", "auto-route"),
            Some(&Value::Bool(true))
        );
        assert!(value_at(&result, "tun", "route-all").is_none());
        assert!(value_at(&result, "tun", "so-mark").is_none());
        assert!(value_at(&result, "tun", "dns-hijack").is_none());
    }

    #[test]
    fn chimera_client_retains_legacy_tun_contract() {
        let result = use_tun(Mapping::new(), tun_params(true, TunFlavor::ChimeraClient));
        assert_eq!(
            value_at(&result, "tun", "route-all"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            value_at(&result, "tun", "dns-hijack"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            value_at(&result, "tun", "so-mark").and_then(Value::as_i64),
            Some(7777)
        );
    }

    #[test]
    fn tun_flavor_matches_upstream_for_standard_cores() {
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRs, TunStack::Mixed),
            TunFlavor::ClashRs
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashRsAlpha, TunStack::Mixed),
            TunFlavor::Standard {
                stack: TunStack::Mixed
            }
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::ClashPremium, TunStack::Mixed),
            TunFlavor::Standard {
                stack: TunStack::Gvisor
            }
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::Mihomo, TunStack::System),
            TunFlavor::Standard {
                stack: TunStack::System
            }
        );
        assert_eq!(
            derive_tun_flavor(ClashCore::ChimeraClient, TunStack::System),
            TunFlavor::ChimeraClient
        );
    }

    #[test]
    fn chimera_dns_defaults_remain_intentional_retained_differences() {
        let result = use_tun(
            Mapping::new(),
            tun_params(
                true,
                TunFlavor::Standard {
                    stack: TunStack::System,
                },
            ),
        );
        assert!(value_at(&result, "dns", "fake-ip-range").is_none());
        let nameservers = value_at(&result, "dns", "nameserver")
            .and_then(Value::as_sequence)
            .unwrap();
        assert_eq!(
            nameservers.first().and_then(Value::as_str),
            Some("https://dns.alidns.com/dns-query")
        );
        assert!(value_at(&result, "dns", "default-nameserver").is_some());
    }
}
