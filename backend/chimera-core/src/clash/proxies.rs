//! Canonical Clash proxy model and pure assembly algorithms.
//! No Tauri dependency: the UI/IPC compatibility projection lives in the app
//! crate until legacy consumers have migrated to this normalized graph.

use anyhow::Result;
use clash_api::{
    IndexMap, ProviderName, ProviderType, Proxy, ProxyName, ProxyProvider, SubscriptionInfo,
    VehicleType,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;

/// Provider metadata retained after assembling the application proxy view.
/// This lives beside `Proxies`, matching ref's `clash/proxies.rs` ownership.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyProviderSummary {
    pub name: ProviderName,
    #[serde(rename = "type")]
    pub provider_type: ProviderType,
    pub vehicle_type: VehicleType,
    pub updated_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub subscription_info: Option<SubscriptionInfo>,
    pub proxy_count: usize,
}

impl From<ProxyProvider> for ProxyProviderSummary {
    fn from(provider: ProxyProvider) -> Self {
        Self {
            name: provider.name,
            provider_type: provider.provider_type,
            vehicle_type: provider.vehicle_type,
            updated_at: provider.updated_at,
            subscription_info: provider.subscription_info,
            proxy_count: provider.proxies.len(),
        }
    }
}

/// Native group kinds and capabilities follow ref; legacy UI serialization
/// remains a projection until the normalized nodes/groups IPC is migrated.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
pub enum ProxyGroupKind {
    Selector,
    #[serde(rename = "URLTest")]
    UrlTest,
    Fallback,
    LoadBalance,
    Relay,
    Smart,
    #[serde(untagged)]
    Unknown(String),
}

impl ProxyGroupKind {
    fn parse(value: &str) -> Self {
        match value {
            "Selector" => Self::Selector,
            "URLTest" => Self::UrlTest,
            "Fallback" => Self::Fallback,
            "LoadBalance" => Self::LoadBalance,
            "Relay" => Self::Relay,
            "Smart" => Self::Smart,
            other => Self::Unknown(other.to_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroupCapabilities {
    pub select: bool,
    pub clear_fixed: bool,
}

impl ProxyGroupCapabilities {
    fn infer(kind: &ProxyGroupKind, record: &Proxy) -> Self {
        match kind {
            ProxyGroupKind::Selector => Self {
                select: true,
                clear_fixed: false,
            },
            ProxyGroupKind::UrlTest | ProxyGroupKind::Fallback => {
                let pinnable = record.fixed.is_some();
                Self {
                    select: pinnable,
                    clear_fixed: pinnable,
                }
            }
            _ => Self::default(),
        }
    }
}

/// Normalized native group metadata, projected to the old UI shape at the
/// boundary. This intentionally matches ref's group record fields.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyGroup {
    pub name: ProxyName,
    #[serde(rename = "type")]
    pub kind: ProxyGroupKind,
    pub all: Vec<ProxyName>,
    pub now: Option<ProxyName>,
    pub fixed: Option<ProxyName>,
    pub test_url: Option<String>,
    pub expected_status: Option<String>,
    pub hidden: bool,
    pub icon: Option<String>,
    pub capabilities: ProxyGroupCapabilities,
}

impl ProxyGroup {
    fn from_record(record: &Proxy) -> Self {
        let kind = ProxyGroupKind::parse(&record.proxy_type);
        let capabilities = ProxyGroupCapabilities::infer(&kind, record);
        Self {
            name: record.name.clone(),
            kind,
            all: record.all.clone().unwrap_or_default(),
            now: record.now.clone(),
            fixed: record
                .fixed
                .clone()
                .filter(|name| !name.as_str().is_empty()),
            test_url: record.test_url.clone().filter(|url| !url.is_empty()),
            expected_status: record
                .expected_status
                .clone()
                .filter(|status| !status.is_empty()),
            hidden: record.hidden.unwrap_or(false),
            icon: record.icon.clone(),
            capabilities,
        }
    }
}

/// Internal normalized proxy graph. This follows ref's `Proxies` layout;
/// the existing public `Proxies` remains a compatibility projection until
/// main and legacy consumers can switch together.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NativeProxies {
    pub global: Option<ProxyGroup>,
    pub groups: Vec<ProxyGroup>,
    pub nodes: IndexMap<ProxyName, Proxy>,
}

/// Normalize both controller spellings of the provider field before
/// building a native node index, as ref's `with_provider` does.
fn with_provider(mut proxy: Proxy) -> Proxy {
    proxy.provider = proxy
        .provider
        .take()
        .filter(|name| !name.is_empty())
        .or_else(|| proxy.provider_name.clone().filter(|name| !name.is_empty()));
    proxy
}

/// Every proxy of an HTTP, File, or Inline provider by name, with its
/// provider. Mihomo 1.19.28 no longer includes these nodes in /proxies, so
/// their metadata must come from /providers/proxies. The index borrows, so
/// only the nodes a group references are ever copied.
fn provider_proxy_index(
    providers: &IndexMap<ProviderName, ProxyProvider>,
) -> IndexMap<&ProxyName, (&ProviderName, &Proxy)> {
    let mut proxies = IndexMap::new();
    for (provider, record) in providers {
        if !matches!(
            record.vehicle_type,
            VehicleType::Http | VehicleType::File | VehicleType::Inline
        ) {
            continue;
        }
        for proxy in &record.proxies {
            proxies.insert(&proxy.name, (provider, proxy));
        }
    }
    proxies
}

/// A group member found neither in /proxies nor in any provider.
fn unknown_proxy(name: &ProxyName) -> Proxy {
    Proxy {
        name: name.clone(),
        proxy_type: "Unknown".to_owned(),
        history: Vec::new(),
        extra: None,
        alive: None,
        udp: false,
        uot: None,
        xudp: None,
        tfo: None,
        mptcp: None,
        smux: None,
        interface: None,
        routing_mark: None,
        provider_name: None,
        dialer_proxy: None,
        id: None,
        now: None,
        all: None,
        test_url: None,
        expected_status: None,
        fixed: None,
        hidden: None,
        icon: None,
        empty_fallback: None,
        provider: None,
    }
}

impl NativeProxies {
    /// Read one controller snapshot and assemble its normalized graph in the
    /// independent core crate, matching ref's core-owned proxy fetch path.
    /// Retry policy is supplied by the caller to preserve app lifecycle rules.
    #[tracing::instrument(skip(api))]
    pub async fn fetch(api: &crate::actor_v2::api::ApiClient) -> Result<Self> {
        let crate::actor_v2::api::ProxySnapshot {
            proxies,
            providers,
            groups,
        } = api.proxy_snapshot().await?;
        Self::from_responses(proxies, &providers, groups)
    }

    /// `group_list` is the core's own group list; `None` infers groups from
    /// every `/proxies` record with members.
    pub fn from_responses(
        proxies: IndexMap<ProxyName, Proxy>,
        providers: &IndexMap<ProviderName, ProxyProvider>,
        group_list: Option<IndexMap<ProxyName, Proxy>>,
    ) -> Result<Self> {
        let mut nodes: IndexMap<ProxyName, Proxy> = proxies
            .into_iter()
            .map(|(name, proxy)| (name, with_provider(proxy)))
            .collect();
        // A listed group replaces its /proxies record, so one group never
        // mixes the members of one read with the selection of the other.
        let mut group_records: IndexMap<ProxyName, Proxy> = match group_list {
            Some(groups) => {
                let groups: IndexMap<ProxyName, Proxy> = groups
                    .into_iter()
                    .map(|(name, group)| (name, with_provider(group)))
                    .collect();
                for (name, group) in &groups {
                    nodes.insert(name.clone(), group.clone());
                }
                groups
            }
            None => nodes
                .iter()
                .filter(|(_, proxy)| proxy.all.is_some())
                .map(|(name, proxy)| (name.clone(), proxy.clone()))
                .collect(),
        };
        let global = group_records.swap_remove(&ProxyName::from("GLOBAL"));

        for required in ["DIRECT", "REJECT"] {
            anyhow::ensure!(
                nodes.contains_key(&ProxyName::from(required)),
                "{required} is missing in /proxies"
            );
        }

        // GLOBAL only orders the groups it lists, never decides which
        // groups exist; the rest follow by name.
        let mut ordered = Vec::with_capacity(group_records.len());
        if let Some(names) = global.as_ref().and_then(|group| group.all.as_ref()) {
            for name in names {
                if let Some(group) = group_records.swap_remove(name) {
                    ordered.push(group);
                }
            }
        }
        let mut remaining: Vec<_> = group_records.into_values().collect();
        remaining.sort_by(|a, b| a.name.as_str().cmp(b.name.as_str()));
        ordered.extend(remaining);

        // Group members missing from /proxies (provider-owned nodes) are
        // added once; a node shared by several groups keeps a single entry.
        let provider_proxies = provider_proxy_index(providers);
        let mut convert = |record: Proxy| {
            for name in record.all.iter().flatten() {
                if !nodes.contains_key(name) {
                    let node = match provider_proxies.get(name) {
                        Some((provider, proxy)) => {
                            let mut node = Proxy::clone(proxy);
                            node.provider = Some(provider.as_str().to_owned());
                            node
                        }
                        None => unknown_proxy(name),
                    };
                    nodes.insert(name.clone(), node);
                }
            }
            ProxyGroup::from_record(&record)
        };
        let groups = ordered.into_iter().map(&mut convert).collect();
        let global = global.map(&mut convert);

        Ok(NativeProxies {
            global,
            groups,
            nodes,
        })
    }

    /// Keep only per-URL latency histories that the normalized proxy view
    /// can display. This mirrors ref's native-node filtering rather than
    /// filtering repeated legacy group member projections.
    pub fn retain_extra(&mut self, keep: &HashSet<&str>) {
        for node in self.nodes.values_mut() {
            if let Some(extra) = node.extra.as_mut() {
                extra.retain(|url, _| keep.contains(url.as_str()));
            }
        }
    }

    /// Preserve both raw and URL-parser-normalized spellings because core
    /// implementations may use either spelling for the latency cache key.
    pub fn trim_for_frontend(mut self, default_url: &str) -> Self {
        let mut urls = Vec::new();
        for url in self
            .global
            .iter()
            .chain(&self.groups)
            .filter_map(|group| group.test_url.as_deref())
            .chain([default_url])
        {
            urls.extend(url::Url::parse(url).map(|parsed| parsed.as_str().to_owned()));
            urls.push(url.to_owned());
        }
        let keep: HashSet<&str> = urls.iter().map(String::as_str).collect();
        self.retain_extra(&keep);
        self
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn native(name: &str, kind: &str, all: Option<Vec<&str>>) -> Proxy {
        serde_json::from_value(json!({
            "name": name,
            "type": kind,
            "udp": false,
            "history": [],
            "all": all,
        }))
        .expect("native Clash proxy fixture")
    }

    struct Fixture {
        proxies: IndexMap<ProxyName, Proxy>,
        providers: IndexMap<ProviderName, ProxyProvider>,
        groups: Option<IndexMap<ProxyName, Proxy>>,
    }

    fn snapshot(items: Vec<Proxy>) -> Fixture {
        let mut proxies = IndexMap::new();
        for fixed in [
            native("DIRECT", "Direct", None),
            native("REJECT", "Reject", None),
        ] {
            proxies.insert(fixed.name.clone(), fixed);
        }
        for item in items {
            proxies.insert(item.name.clone(), item);
        }
        Fixture {
            proxies,
            providers: IndexMap::new(),
            groups: None,
        }
    }

    /// Given a native core group record, infer the same capabilities as ref,
    /// independent of the legacy UI's embedded-node projection.
    #[test]
    fn native_group_metadata_preserves_pin_and_delay_semantics() {
        let selector = native("manual", "Selector", Some(vec!["DIRECT"]));
        let selector = ProxyGroup::from_record(&selector);
        assert!(selector.capabilities.select);
        assert!(!selector.capabilities.clear_fixed);
        assert_eq!(selector.all, vec![ProxyName::from("DIRECT")]);

        let automatic = native("auto", "URLTest", Some(vec!["DIRECT"]));
        let automatic = ProxyGroup::from_record(&automatic);
        assert!(!automatic.capabilities.select);
        assert!(!automatic.capabilities.clear_fixed);

        let mut pinnable = native("auto", "URLTest", Some(vec!["DIRECT"]));
        pinnable.fixed = Some(ProxyName::from(""));
        pinnable.test_url = Some("https://example.test/generate_204".into());
        pinnable.expected_status = Some("204".into());
        let pinnable = ProxyGroup::from_record(&pinnable);
        assert!(pinnable.capabilities.select);
        assert!(pinnable.capabilities.clear_fixed);
        assert!(pinnable.fixed.is_none());
        assert_eq!(pinnable.expected_status.as_deref(), Some("204"));
        assert_eq!(
            pinnable.test_url.as_deref(),
            Some("https://example.test/generate_204")
        );

        let unknown = ProxyGroup::from_record(&native("future", "NewCoreKind", None));
        assert_eq!(unknown.kind, ProxyGroupKind::Unknown("NewCoreKind".into()));
        assert!(!unknown.capabilities.select);
        assert!(!unknown.capabilities.clear_fixed);
    }

    #[test]
    fn native_proxy_provider_alias_is_normalized_without_overriding_primary() {
        let mut alias = native("alias", "Vless", None);
        alias.provider_name = Some("legacy-provider".into());
        assert_eq!(
            with_provider(alias).provider.as_deref(),
            Some("legacy-provider")
        );

        let mut primary = native("primary", "Vless", None);
        primary.provider = Some("canonical-provider".into());
        primary.provider_name = Some("legacy-provider".into());
        assert_eq!(
            with_provider(primary).provider.as_deref(),
            Some("canonical-provider")
        );

        let mut empty = native("empty", "Vless", None);
        empty.provider = Some(String::new());
        empty.provider_name = Some("fallback-provider".into());
        assert_eq!(
            with_provider(empty).provider.as_deref(),
            Some("fallback-provider")
        );
    }

    #[test]
    fn native_trim_preserves_relevant_raw_and_normalized_latency_urls() {
        let mut global = native("GLOBAL", "Selector", Some(vec!["G"]));
        global.test_url = Some("https://global".into());
        let mut group = native("G", "URLTest", Some(vec!["node"]));
        group.test_url = Some("https://group".into());
        group.hidden = Some(true);
        let mut node = native("node", "Vless", None);
        node.extra = Some(
            [
                "https://global",
                "https://global/",
                "https://group/",
                "https://default/",
                "https://unused/",
            ]
            .into_iter()
            .map(|url| {
                (
                    url.to_owned(),
                    clash_api::ProxyExtra {
                        alive: true,
                        history: Vec::new(),
                    },
                )
            })
            .collect(),
        );
        let data = snapshot(vec![global, group, node]);
        let native = NativeProxies::from_responses(data.proxies, &data.providers, data.groups)
            .unwrap()
            .trim_for_frontend("https://default");
        let retained: Vec<_> = native.nodes[&ProxyName::from("node")]
            .extra
            .as_ref()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            retained,
            [
                "https://global",
                "https://global/",
                "https://group/",
                "https://default/"
            ]
        );
        assert!(native.nodes[&ProxyName::from("DIRECT")].extra.is_none());
    }

    #[test]
    fn native_trim_keeps_empty_latency_map_and_normal_history() {
        let mut node = native("node", "Vless", None);
        node.extra = Some(IndexMap::new());
        node.history = vec![
            serde_json::from_value(json!({
                "time": "2026-10-06T12:34:56+08:00", "delay": 7
            }))
            .unwrap(),
        ];
        let before = serde_json::to_value(&node.history).unwrap();
        let data = snapshot(vec![node]);
        let native = NativeProxies::from_responses(data.proxies, &data.providers, data.groups)
            .unwrap()
            .trim_for_frontend("https://default/");
        let after = &native.nodes[&ProxyName::from("node")];
        assert_eq!(after.extra, Some(IndexMap::new()));
        assert_eq!(serde_json::to_value(&after.history).unwrap(), before);
    }
}
