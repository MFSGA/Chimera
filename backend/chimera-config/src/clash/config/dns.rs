//! Typed DNS and hosts configuration kept independently from the selected
//! profile. The control selectors start at Profile, so upgrading existing
//! installations does not change their generated runtime configuration.
//! The field names and defaults follow ref's DNS settings contract.

use std::net::IpAddr;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use specta::Type;
use strum::{AsRefStr, EnumString, IntoStaticStr, VariantArray};

use crate::{clash::features::DnsFeatures, runtime::value::ConfigValue};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ControlSource {
    #[default]
    Profile,
    #[serde(alias = "nyanpasu")]
    Chimera,
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Type,
    AsRefStr,
    EnumString,
    VariantArray,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum EnhancedMode {
    Normal,
    #[default]
    FakeIp,
    RedirHost,
}

#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Type,
    AsRefStr,
    EnumString,
    VariantArray,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
pub enum FakeIpFilterMode {
    #[default]
    Blacklist,
    Whitelist,
    Rule,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(untagged)]
pub enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct FallbackFilter {
    pub geoip: bool,
    pub geoip_code: String,
    pub ipcidr: Vec<String>,
    pub domain: Vec<String>,
    pub geosite: Vec<String>,
}

impl Default for FallbackFilter {
    fn default() -> Self {
        Self {
            geoip: true,
            geoip_code: "CN".into(),
            ipcidr: strings(&["240.0.0.0/4", "0.0.0.0/32"]),
            domain: strings(&["+.google.com", "+.facebook.com", "+.youtube.com"]),
            geosite: Vec::new(),
        }
    }
}

/// Saved DNS values: the user keeps editing these even while the selected
/// profile owns the runtime DNS table.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct DnsSettings {
    pub enable: bool,
    pub listen: Option<String>,
    pub ipv6: bool,
    pub enhanced_mode: EnhancedMode,
    pub fake_ip_range: Option<String>,
    pub fake_ip_range6: Option<String>,
    pub fake_ip_filter_mode: FakeIpFilterMode,
    pub fake_ip_filter: Vec<String>,
    pub use_hosts: bool,
    pub use_system_hosts: bool,
    pub respect_rules: bool,
    pub prefer_h3: bool,
    pub default_nameserver: Vec<String>,
    pub nameserver: Vec<String>,
    pub fallback: Vec<String>,
    pub proxy_server_nameserver: Vec<String>,
    pub direct_nameserver: Vec<String>,
    pub direct_nameserver_follow_policy: bool,
    pub nameserver_policy: IndexMap<String, OneOrMany>,
    pub fallback_filter: FallbackFilter,
    /// Untyped additional core DNS keys are preserved for future support.
    #[specta(type = IndexMap<String, specta_typescript::Any>)]
    pub extra: IndexMap<String, serde_json::Value>,
}

impl Default for DnsSettings {
    fn default() -> Self {
        let doh = strings(&[
            "https://doh.pub/dns-query",
            "https://dns.alidns.com/dns-query",
        ]);
        Self {
            enable: true,
            listen: None,
            ipv6: false,
            enhanced_mode: EnhancedMode::FakeIp,
            fake_ip_range: Some("198.18.0.1/16".into()),
            fake_ip_range6: None,
            fake_ip_filter_mode: FakeIpFilterMode::Blacklist,
            fake_ip_filter: strings(&[
                "*",
                "+.lan",
                "+.local",
                "*.arpa",
                "time.*.com",
                "ntp.*.com",
                "+.market.xiaomi.com",
                "localhost.ptlogin2.qq.com",
                "*.msftncsi.com",
                "www.msftconnecttest.com",
            ]),
            use_hosts: false,
            use_system_hosts: false,
            respect_rules: false,
            prefer_h3: false,
            default_nameserver: strings(&["tls://223.5.5.5"]),
            nameserver: doh.clone(),
            fallback: Vec::new(),
            proxy_server_nameserver: doh,
            direct_nameserver: Vec::new(),
            direct_nameserver_follow_policy: false,
            nameserver_policy: IndexMap::new(),
            fallback_filter: FallbackFilter::default(),
            extra: IndexMap::new(),
        }
    }
}

/// The typed keys of [`DnsSettings`], spelled as in the runtime config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type, IntoStaticStr, VariantArray)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum DnsKey {
    Enable,
    Listen,
    Ipv6,
    EnhancedMode,
    FakeIpRange,
    FakeIpRange6,
    FakeIpFilterMode,
    FakeIpFilter,
    UseHosts,
    UseSystemHosts,
    RespectRules,
    PreferH3,
    DefaultNameserver,
    Nameserver,
    Fallback,
    ProxyServerNameserver,
    DirectNameserver,
    DirectNameserverFollowPolicy,
    NameserverPolicy,
    FallbackFilter,
}

impl DnsKey {
    /// The runtime config mapping that holds these keys.
    pub const TABLE: &'static str = "dns";

    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

/// A table rendered for the selected core, with what had to be left out.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedTable {
    pub table: ConfigValue,
    /// One line per setting the core cannot take as saved.
    pub warnings: Vec<String>,
}

impl DnsSettings {
    /// The `dns` table for a core with `features`, or `None` when the core
    /// has no `dns` table Chimera writes. Keys the core does not read are
    /// left out; a saved value that differs from the default gets a warning.
    pub fn render(&self, features: &DnsFeatures) -> Option<RenderedTable> {
        if features.keys.is_ignored() {
            return None;
        }
        let defaults = Self::default();
        let mut warnings = Vec::new();
        let mut table = serde_json::Map::new();

        // A filter mode the core does not apply turns a rule or allow list
        // into a deny list, so the list is dropped with it.
        let drop_filter = self.fake_ip_filter_mode != FakeIpFilterMode::Blacklist
            && !features
                .fake_ip_filter_mode
                .supports(&self.fake_ip_filter_mode);
        if drop_filter {
            warnings.push(format!(
                "the selected core does not support `dns.fake-ip-filter-mode: {}`; \
                 `fake-ip-filter-mode` and `fake-ip-filter` are not written",
                self.fake_ip_filter_mode.as_ref(),
            ));
        }

        for &key in DnsKey::VARIANTS {
            if drop_filter && matches!(key, DnsKey::FakeIpFilterMode | DnsKey::FakeIpFilter) {
                continue;
            }
            if !features.keys.supports(&key) {
                let mut unread = Vec::new();
                if self.value(key, features, &mut unread)
                    != defaults.value(key, features, &mut unread)
                {
                    warnings.push(format!(
                        "the selected core does not read `dns.{}`; it is not written",
                        key.as_str(),
                    ));
                }
                continue;
            }
            if key == DnsKey::EnhancedMode && !features.enhanced_mode.supports(&self.enhanced_mode)
            {
                // `normal` is every core's default, so leaving it out keeps it.
                if self.enhanced_mode != EnhancedMode::Normal {
                    warnings.push(format!(
                        "the selected core does not support `dns.enhanced-mode: {}`; \
                         it is not written",
                        self.enhanced_mode.as_ref(),
                    ));
                }
                continue;
            }
            if let Some(value) = self.value(key, features, &mut warnings) {
                table.insert(key.as_str().to_owned(), value);
            }
        }
        for (key, value) in &self.extra {
            if DnsKey::VARIANTS.iter().any(|typed| typed.as_str() == key) {
                continue;
            }
            table.insert(key.clone(), value.clone());
        }

        Some(RenderedTable {
            table: ConfigValue::from_json_value(serde_json::Value::Object(table)),
            warnings,
        })
    }

    /// The value written at `key`, or `None` when nothing is written. Values
    /// the core reads only in part are reduced here, with a warning.
    fn value(
        &self,
        key: DnsKey,
        features: &DnsFeatures,
        warnings: &mut Vec<String>,
    ) -> Option<serde_json::Value> {
        use serde_json::json;
        let value = match key {
            DnsKey::Enable => json!(self.enable),
            DnsKey::Listen => json!(self.listen.as_ref()?),
            DnsKey::Ipv6 => json!(self.ipv6),
            DnsKey::EnhancedMode => json!(self.enhanced_mode),
            DnsKey::FakeIpRange => json!(self.fake_ip_range.as_ref()?),
            DnsKey::FakeIpRange6 => json!(self.fake_ip_range6.as_ref()?),
            DnsKey::FakeIpFilterMode => json!(self.fake_ip_filter_mode),
            DnsKey::FakeIpFilter => json!(self.fake_ip_filter),
            DnsKey::UseHosts => json!(self.use_hosts),
            DnsKey::UseSystemHosts => json!(self.use_system_hosts),
            DnsKey::RespectRules => json!(self.respect_rules),
            DnsKey::PreferH3 => json!(self.prefer_h3),
            DnsKey::DefaultNameserver => json!(self.default_nameserver),
            DnsKey::Nameserver => json!(self.nameserver),
            DnsKey::Fallback => json!(self.fallback),
            DnsKey::ProxyServerNameserver => json!(self.proxy_server_nameserver),
            DnsKey::DirectNameserver => json!(self.direct_nameserver),
            DnsKey::DirectNameserverFollowPolicy => json!(self.direct_nameserver_follow_policy),
            DnsKey::NameserverPolicy => {
                if self.nameserver_policy.is_empty() {
                    return None;
                }
                let policy: serde_json::Map<String, serde_json::Value> = self
                    .nameserver_policy
                    .iter()
                    .map(|(domain, servers)| {
                        let value = single_or_list(
                            servers,
                            features.list_values,
                            || format!("dns.nameserver-policy.{domain}"),
                            warnings,
                        );
                        (domain.clone(), value)
                    })
                    .collect();
                serde_json::Value::Object(policy)
            }
            DnsKey::FallbackFilter => {
                let mut filter = json!({
                    "geoip": self.fallback_filter.geoip,
                    "geoip-code": self.fallback_filter.geoip_code,
                    "ipcidr": self.fallback_filter.ipcidr,
                    "domain": self.fallback_filter.domain,
                });
                if !self.fallback_filter.geosite.is_empty() {
                    if features.fallback_filter_geosite {
                        filter["geosite"] = json!(self.fallback_filter.geosite);
                    } else {
                        warnings.push(
                            "the selected core does not read `dns.fallback-filter.geosite`; \
                             it is not written"
                                .to_owned(),
                        );
                    }
                }
                filter
            }
        };
        Some(value)
    }
}

/// The `hosts` table for a core with `features`, or `None` when the core has
/// no `hosts` table Chimera writes. An empty result means Chimera removes
/// the table.
pub fn render_hosts(
    hosts: &IndexMap<String, OneOrMany>,
    features: &DnsFeatures,
) -> Option<RenderedTable> {
    if features.keys.is_ignored() {
        return None;
    }
    let mut warnings = Vec::new();
    let mut table = serde_json::Map::new();
    for (domain, targets) in hosts {
        if !features.list_values {
            // Without list values the core reads only the first IP address.
            // Ignore later aliases along with the other unselected values.
            if let Some(alias) = targets
                .as_slice()
                .first()
                .filter(|target| target.parse::<IpAddr>().is_err())
            {
                warnings.push(format!(
                    "the selected core does not read the alias `{alias}` in `hosts.{domain}`; \
                     the entry is not written"
                ));
                continue;
            }
        }
        let value = single_or_list(
            targets,
            features.list_values,
            || format!("hosts.{domain}"),
            &mut warnings,
        );
        table.insert(domain.clone(), value);
    }
    Some(RenderedTable {
        table: ConfigValue::from_json_value(serde_json::Value::Object(table)),
        warnings,
    })
}

/// `values` as written, or its first entry for a core that reads a single
/// one.
fn single_or_list(
    values: &OneOrMany,
    list_values: bool,
    path: impl FnOnce() -> String,
    warnings: &mut Vec<String>,
) -> serde_json::Value {
    match values {
        OneOrMany::One(value) => serde_json::Value::String(value.clone()),
        OneOrMany::Many(list) if list_values => serde_json::json!(list),
        OneOrMany::Many(list) => {
            if list.len() > 1 {
                warnings.push(format!(
                    "the selected core reads one entry at `{}`; only the first is written",
                    path(),
                ));
            }
            serde_json::Value::String(list.first().cloned().unwrap_or_default())
        }
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).into()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_control_is_profile_and_dns_is_kept_as_a_draft() {
        assert_eq!(ControlSource::default(), ControlSource::Profile);
        let default = DnsSettings::default();
        assert!(default.enable);
        assert_eq!(default.enhanced_mode, EnhancedMode::FakeIp);
        assert!(!default.nameserver.is_empty());
    }

    #[test]
    fn control_source_uses_chimera_brand_and_reads_legacy_values() {
        assert_eq!(
            serde_json::from_str::<ControlSource>("\"nyanpasu\"").unwrap(),
            ControlSource::Chimera
        );
        assert_eq!(
            serde_json::to_string(&ControlSource::Chimera).unwrap(),
            "\"chimera\""
        );
    }

    #[test]
    fn upgrading_old_clash_config_never_implicitly_takes_over_dns_or_hosts() {
        use crate::clash::config::ClashConfig;

        let mut old = serde_json::to_value(ClashConfig::default()).unwrap();
        let table = old.as_object_mut().unwrap();
        for key in ["dns_control", "dns", "hosts_control", "hosts"] {
            table.remove(key);
        }
        let upgraded: ClashConfig = serde_json::from_value(old).unwrap();
        assert!(upgraded.managed_dns().is_none());
        assert!(upgraded.managed_hosts().is_none());
        assert_eq!(upgraded.dns_control, ControlSource::Profile);
        assert_eq!(upgraded.hosts_control, ControlSource::Profile);
        assert_eq!(upgraded.dns, DnsSettings::default());
        assert!(upgraded.hosts.is_empty());
    }

    #[test]
    fn rendering_flattens_extensions_without_overriding_typed_dns_fields() {
        let mut dns = DnsSettings::default();
        dns.extra
            .insert("cache-algorithm".into(), serde_json::json!("arc"));
        dns.extra.insert("enable".into(), serde_json::json!(false));
        let rendered = dns
            .render(&crate::application::ClashCore::Mihomo.dns_features())
            .expect("Mihomo supports DNS")
            .table
            .to_json();
        assert_eq!(rendered["enable"], true);
        assert_eq!(rendered["cache-algorithm"], "arc");
        assert!(rendered.get("extra").is_none());
        assert!(rendered.get("listen").is_none());
    }

    #[test]
    fn clash_rs_drops_unsupported_dns_settings_and_warns() {
        use crate::application::ClashCore;

        let mut dns = DnsSettings::default();
        dns.prefer_h3 = true;
        dns.fake_ip_filter_mode = FakeIpFilterMode::Rule;
        dns.fake_ip_filter = vec!["DOMAIN-SUFFIX,lan,real-ip".into()];
        dns.nameserver_policy.insert(
            "geosite:cn".into(),
            OneOrMany::Many(vec!["223.5.5.5".into(), "119.29.29.29".into()]),
        );

        let rendered = dns.render(&ClashCore::ClashRs.dns_features()).unwrap();
        let table = rendered.table.to_json();
        assert!(table.get("prefer-h3").is_none());
        assert!(table.get("fake-ip-filter-mode").is_none());
        assert!(table.get("fake-ip-filter").is_none());
        assert_eq!(table["nameserver-policy"]["geosite:cn"], "223.5.5.5");
        assert!(
            rendered
                .warnings
                .iter()
                .any(|entry| entry.contains("prefer-h3"))
        );
        assert!(
            rendered
                .warnings
                .iter()
                .any(|entry| entry.contains("fake-ip-filter-mode"))
        );
        assert!(
            rendered
                .warnings
                .iter()
                .any(|entry| entry.contains("only the first"))
        );
    }

    #[test]
    fn hosts_values_are_filtered_per_core_without_losing_saved_draft() {
        use crate::application::ClashCore;

        let hosts = IndexMap::from([
            (
                "one.test".into(),
                OneOrMany::Many(vec!["1.1.1.1".into(), "8.8.8.8".into()]),
            ),
            (
                "mixed.test".into(),
                OneOrMany::Many(vec!["9.9.9.9".into(), "fallback.example".into()]),
            ),
            ("alias.test".into(), OneOrMany::One("target.example".into())),
        ]);
        let mihomo = render_hosts(&hosts, &ClashCore::Mihomo.dns_features()).unwrap();
        assert_eq!(
            mihomo.table.to_json()["one.test"],
            serde_json::json!(["1.1.1.1", "8.8.8.8"])
        );
        assert_eq!(mihomo.table.to_json()["alias.test"], "target.example");
        assert!(mihomo.warnings.is_empty());

        let rs = render_hosts(&hosts, &ClashCore::ClashRs.dns_features()).unwrap();
        assert_eq!(rs.table.to_json()["one.test"], "1.1.1.1");
        assert_eq!(rs.table.to_json()["mixed.test"], "9.9.9.9");
        assert!(rs.table.to_json().get("alias.test").is_none());
        assert_eq!(rs.warnings.len(), 3);
        assert!(rs.warnings.iter().any(|warning| {
            warning.contains("hosts.mixed.test") && warning.contains("only the first")
        }));
    }

    #[test]
    fn settings_roundtrip_keeps_order_and_single_or_multiple_policy_values() {
        let document = r#"
enable: false
enhanced-mode: redir-host
nameserver-policy:
  geosite:cn: [223.5.5.5, 119.29.29.29]
  +.lan: 192.168.1.1
"#;
        let original: DnsSettings = serde_yaml_ng::from_str(document).unwrap();
        assert!(!original.enable);
        assert_eq!(
            original
                .nameserver_policy
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["geosite:cn", "+.lan"]
        );
        let encoded = serde_yaml_ng::to_string(&original).unwrap();
        let decoded: DnsSettings = serde_yaml_ng::from_str(&encoded).unwrap();
        assert_eq!(decoded, original);
    }
}
