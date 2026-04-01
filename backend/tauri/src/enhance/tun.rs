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

#[tracing_attributes::instrument(skip(config))]
pub fn use_tun(mut config: Mapping, enable: bool) -> Mapping {
    let tun_key = Value::from("tun");
    let tun_val = config.get(&tun_key);
    tracing::debug!("tun_val: {:?}", tun_val);
    if !enable && tun_val.is_none() {
        return config;
    }

    let mut tun_val = tun_val.map_or(Mapping::new(), |val| {
        val.as_mapping().cloned().unwrap_or(Mapping::new())
    });

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
            append!(tun_val, "gateway", "198.18.0.1/16");
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
        config
    }
}

fn use_dns_for_tun(mut config: Mapping) -> Mapping {
    let dns_key = Value::from("dns");
    let dns_val = config.get(&dns_key);

    let mut dns_val = dns_val.map_or(Mapping::new(), |value| {
        value.as_mapping().cloned().unwrap_or(Mapping::new())
    });

    revise!(dns_val, "enable", true);
    append!(dns_val, "enhanced-mode", "fake-ip");
    append!(dns_val, "fake-ip-range", "198.18.0.1/16");
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
    append!(dns_val, "fallback", vec![] as Vec<&str>);

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
    config
}
