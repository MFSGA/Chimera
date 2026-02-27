use serde_yaml::{Mapping, Value};

pub const HANDLE_FIELDS: [&str; 12] = [
    "mode",
    "port",
    "socks-port",
    "mixed-port",
    "allow-lan",
    "log-level",
    "log_level",
    "ipv6",
    "secret",
    "external-controller",
    "bind-address",
    "bind_address",
];

pub const DEFAULT_FIELDS: [&str; 9] = [
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rules",
    "rule-providers",
    "profile",
    "log_level",
    "bind_address",
    "bind-address",
];

pub const OTHERS_FIELDS: [&str; 33] = [
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
    "log_level",
    "bind-address",
    // todo
    "bind_address",
    "authentication",
    "tls",                       // meta
    "sniffer",                   // meta
    "geox-url",                  // meta
    "listeners",                 // meta
    "sub-rules",                 // meta
    "geodata-mode",              // meta
    "unified-delay",             // meta
    "tcp-concurrent",            // meta
    "enable-process",            // meta
    "find-process-mode",         // meta
    "skip-auth-prefixes",        // meta
    "external-controller-tls",   // meta
    "global-client-fingerprint", // meta
];

pub fn use_valid_fields(valid: &[String]) -> Vec<String> {
    let others = Vec::from(OTHERS_FIELDS);

    valid
        .iter()
        .cloned()
        .map(|s| s.to_ascii_lowercase())
        .filter(|s| others.contains(&s.as_str()))
        .chain(DEFAULT_FIELDS.iter().map(|s| s.to_string()))
        .collect()
}

pub fn use_keys(config: &Mapping) -> Vec<String> {
    config
        .iter()
        .filter_map(|(key, _)| key.as_str())
        .map(|s| {
            let mut s = s.to_string();
            s.make_ascii_lowercase();
            s
        })
        .collect()
}

/// 使用白名单过滤配置字段
pub fn use_whitelist_fields_filter(config: Mapping, filter: &[String], enable: bool) -> Mapping {
    if !enable {
        return config;
    }

    let mut ret = Mapping::new();

    for (key, value) in config.into_iter() {
        if let Some(key) = key.as_str()
            && filter.contains(&key.to_string())
        {
            ret.insert(Value::from(key), value);
        }
    }
    ret
}
