use anyhow::{Result, bail};
use serde_yaml::{Mapping, Value};

use crate::config::{
    chimera::{ClashCore, TunStack},
    core::Config,
};

macro_rules! revise {
    ($map: expr, $key: expr, $val: expr) => {
        let ret_key = Value::String($key.into());
        $map.insert(ret_key, Value::from($val));
    };
}

macro_rules! append {
    ($map: expr, $key: expr, $val: expr) => {
        let ret_key = Value::String($key.into());
        if !$map.contains_key(&ret_key) {
            $map.insert(ret_key, Value::from($val));
        }
    };
}

fn mapping_field_or_empty(config: &Mapping, key: &str) -> Result<Mapping> {
    match config.get(key) {
        None => Ok(Mapping::new()),
        Some(Value::Mapping(mapping)) => Ok(mapping.clone()),
        Some(_) => bail!("`{key}` must be a mapping"),
    }
}

#[tracing_attributes::instrument(skip(config))]
pub fn use_tun(mut config: Mapping, enable: bool) -> Result<Mapping> {
    let tun_key = Value::from("tun");
    let tun_val = config.get(&tun_key);
    tracing::debug!("tun_val: {:?}", tun_val);
    if !enable && tun_val.is_none() {
        return Ok(config);
    }

    let mut tun_val = mapping_field_or_empty(&config, "tun")?;

    revise!(tun_val, "enable", enable);
    if enable {
        let core = {
            *Config::verge()
                .latest()
                .clash_core
                .as_ref()
                .unwrap_or(&ClashCore::default())
        };
        if matches!(
            core,
            // todo: solve the remote problem.
            ClashCore::ClashRs | ClashCore::ChimeraClient | ClashCore::ClashRsAlpha
        ) {
            append!(tun_val, "device-id", "dev://utun1989");
            append!(tun_val, "route-all", true);
            append!(tun_val, "dns-hijack", true);
            // mainly used for linux
            append!(tun_val, "so-mark", 7777);
        } else {
            let mut tun_stack = {
                *Config::verge()
                    .latest()
                    .tun_stack
                    .as_ref()
                    .unwrap_or(&TunStack::default())
            };
            if core == ClashCore::ClashPremium && tun_stack == TunStack::Mixed {
                tun_stack = TunStack::Gvisor;
            }
            append!(tun_val, "stack", AsRef::<str>::as_ref(&tun_stack));
            append!(tun_val, "dns-hijack", vec!["any:53"]);
            append!(tun_val, "auto-route", true);
            append!(tun_val, "auto-detect-interface", true);
        }
    }

    revise!(config, "tun", tun_val);

    if enable {
        use_dns_for_tun(config)
    } else {
        Ok(config)
    }
}

fn use_dns_for_tun(mut config: Mapping) -> Result<Mapping> {
    let mut dns_val = mapping_field_or_empty(&config, "dns")?;

    revise!(dns_val, "enable", true);
    append!(dns_val, "enhanced-mode", "fake-ip");
    // append!(dns_val, "fake-ip-range", "198.18.0.1/16");
    append!(
        dns_val,
        "nameserver",
        vec![
            "https://dns.alidns.com/dns-query",
            "114.114.114.114",
            "223.5.5.5",
            "8.8.8.8"
        ]
    );
    append!(
        dns_val,
        "default-nameserver",
        vec!["114.114.114.114", "1.1.1.1", "8.8.8.8"]
    );
    // append!(dns_val, "fallback", vec![] as Vec<&str>);

    #[cfg(target_os = "windows")]
    append!(
        dns_val,
        "fake-ip-filter",
        vec![
            "dns.msftncsi.com",
            "www.msftncsi.com",
            "www.msftconnecttest.com",
        ]
    );

    revise!(config, "dns", dns_val);
    Ok(config)
}

#[cfg(test)]
mod tests {
    use serde_yaml::{Mapping, Value};

    use super::{use_dns_for_tun, use_tun};

    fn mapping(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).expect("valid TUN mapping fixture")
    }

    #[test]
    fn disabling_absent_tun_preserves_the_complete_config() {
        let config = mapping("mixed-port: 7890\n");
        let expected = config.clone();

        let result = use_tun(config, false).expect("absent disabled TUN must be a no-op");

        assert_eq!(result, expected);
    }

    #[test]
    fn disabling_non_mapping_tun_is_rejected() {
        for yaml in ["tun: invalid\n", "tun: []\n", "tun: null\n"] {
            let error = use_tun(mapping(yaml), false)
                .expect_err("non-mapping TUN field must not be silently replaced");
            assert!(error.to_string().contains("`tun` must be a mapping"));
        }
    }

    #[test]
    fn disabling_existing_tun_preserves_custom_fields() {
        let result = use_tun(mapping("tun:\n  stack: system\n  custom: true\n"), false)
            .expect("valid TUN mapping must be updated");
        let tun = result
            .get("tun")
            .and_then(Value::as_mapping)
            .expect("updated TUN must remain a mapping");

        assert_eq!(tun.get("enable"), Some(&Value::Bool(false)));
        assert_eq!(tun.get("stack"), Some(&Value::String("system".into())));
        assert_eq!(tun.get("custom"), Some(&Value::Bool(true)));
    }

    #[test]
    fn dns_for_tun_rejects_non_mapping_dns() {
        for yaml in ["dns: invalid\n", "dns: []\n", "dns: null\n"] {
            let error = use_dns_for_tun(mapping(yaml))
                .expect_err("non-mapping DNS field must not be silently replaced");
            assert!(error.to_string().contains("`dns` must be a mapping"));
        }
    }

    #[test]
    fn dns_for_tun_preserves_existing_values_and_adds_defaults() {
        let result = use_dns_for_tun(mapping(
            "dns:\n  nameserver:\n    - https://example.com/dns-query\n  custom: true\n",
        ))
        .expect("valid DNS mapping must be enhanced");
        let dns = result
            .get("dns")
            .and_then(Value::as_mapping)
            .expect("enhanced DNS must remain a mapping");

        assert_eq!(dns.get("enable"), Some(&Value::Bool(true)));
        assert_eq!(dns.get("custom"), Some(&Value::Bool(true)));
        assert_eq!(
            dns.get("nameserver")
                .and_then(Value::as_sequence)
                .and_then(|nameservers| nameservers.first())
                .and_then(Value::as_str),
            Some("https://example.com/dns-query")
        );
        assert!(dns.contains_key("default-nameserver"));
    }
}
