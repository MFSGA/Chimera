use serde_yaml::Mapping;

pub const DEFAULT_FIELDS: [&str; 5] = [
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rules",
    "rule-providers",
];

pub const OTHERS_FIELDS: [&str; 31] = [
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
