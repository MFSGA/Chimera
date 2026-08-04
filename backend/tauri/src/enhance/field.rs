use std::collections::HashSet;

use serde_yaml::{Mapping, Value};

pub const HANDLE_FIELDS: [&str; 10] = [
    "mode",
    "port",
    "socks-port",
    "mixed-port",
    "allow-lan",
    "log-level",
    "ipv6",
    "secret",
    "external-controller",
    "bind-address",
];

pub const DEFAULT_FIELDS: [&str; 5] = [
    "proxies",
    "proxy-groups",
    "proxy-providers",
    "rules",
    "rule-providers",
];

pub const OTHERS_FIELDS: [&str; 30] = [
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
    // "bind-address",
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
    let allowed = OTHERS_FIELDS.into_iter().collect::<HashSet<_>>();
    let mut seen = HashSet::new();

    valid
        .iter()
        .map(|field| field.trim().to_ascii_lowercase())
        .filter(|field| allowed.contains(field.as_str()))
        .chain(DEFAULT_FIELDS.iter().map(|field| field.to_string()))
        .filter(|field| seen.insert(field.clone()))
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

    let filter = filter.iter().map(String::as_str).collect::<HashSet<_>>();
    for (key, value) in config {
        if let Some(key) = key.as_str() {
            let normalized = key.to_ascii_lowercase();
            if filter.contains(normalized.as_str()) {
                ret.insert(Value::from(normalized), value);
            }
        }
    }
    ret
}

#[cfg(test)]
mod tests {
    use serde_yaml::{Mapping, Value};

    use super::{DEFAULT_FIELDS, use_keys, use_valid_fields, use_whitelist_fields_filter};

    fn mapping(yaml: &str) -> Mapping {
        serde_yaml::from_str(yaml).expect("valid field filtering fixture")
    }

    #[test]
    fn valid_fields_are_trimmed_lowercased_and_deduplicated() {
        let fields =
            use_valid_fields(&[" DNS ".into(), "dns".into(), "TUN".into(), "unknown".into()]);

        assert_eq!(fields.iter().filter(|field| *field == "dns").count(), 1);
        assert_eq!(fields.iter().filter(|field| *field == "tun").count(), 1);
        assert!(!fields.iter().any(|field| field == "unknown"));
        for default in DEFAULT_FIELDS {
            assert_eq!(fields.iter().filter(|field| *field == default).count(), 1);
        }
    }

    #[test]
    fn whitelist_filter_canonicalizes_accepted_ascii_key_case() {
        let config = mapping("DNS:\n  enable: true\nProxies: []\nUnknown: true\n");
        let filter = vec!["dns".into(), "proxies".into()];

        let filtered = use_whitelist_fields_filter(config, &filter, true);

        assert!(filtered.contains_key("dns"));
        assert!(filtered.contains_key("proxies"));
        assert!(!filtered.contains_key("DNS"));
        assert!(!filtered.contains_key("Unknown"));
    }

    #[test]
    fn disabled_whitelist_filter_preserves_original_key_spelling() {
        let config = mapping("DNS:\n  enable: true\n");
        let expected = config.clone();

        assert_eq!(use_whitelist_fields_filter(config, &[], false), expected);
    }

    #[test]
    fn use_keys_reports_normalized_string_keys_only() {
        let mut config = mapping("DNS: {}\n");
        config.insert(Value::Number(1.into()), Value::Bool(true));

        assert_eq!(use_keys(&config), vec!["dns"]);
    }
}
