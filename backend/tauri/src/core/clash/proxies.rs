use anyhow::{Result, ensure};
use backon::Retryable;
use clash_api::{
    IndexMap, ProviderName, ProviderType, Proxy, ProxyName, ProxyProvider, SubscriptionInfo,
    VehicleType,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};
use tokio::{sync::broadcast, try_join};
use tracing::instrument;

use crate::core::actor_v2::api::{ApiClient, ProxySnapshot};
use crate::core::clash::{CLASH_API_DEFAULT_BACKOFF_STRATEGY, api};

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

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
pub struct ProxyGroupItem {
    /// 1
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
    /// 2
    pub name: String,
    /// 3
    pub all: Vec<api::ProxyItem>,
    /// 4
    pub history: Vec<api::ProxyItemHistory>,
    /// 5
    pub now: Option<String>, // 当前选中的代理
    /// 6
    pub r#type: String, // TODO: 考虑改成枚举
    /// 7
    pub udp: bool,
    /// 8
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
}

impl From<api::ProxyItem> for ProxyGroupItem {
    fn from(item: api::ProxyItem) -> Self {
        let all = vec![];
        ProxyGroupItem {
            name: item.name,
            history: item.history,
            all,

            hidden: item.hidden,
            now: item.now,
            r#type: item.r#type,
            udp: item.udp,
            icon: item.icon,
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

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct Proxies {
    pub groups: Vec<ProxyGroupItem>,
    /// 2
    pub global: ProxyGroupItem,
    /// 3
    pub direct: api::ProxyItem,
}

/// Project a native Clash proxy without leaking its protocol client into the
/// legacy main/legacy UI wire model. The typed proxy view is migrated separately.
fn proxy_item(record: &Proxy) -> api::ProxyItem {
    api::ProxyItem {
        name: record.name.to_string(),
        history: record
            .history
            .iter()
            .map(|sample| api::ProxyItemHistory {
                time: sample.time.to_rfc3339(),
                delay: sample.delay,
            })
            .collect(),
        all: record
            .all
            .as_ref()
            .map(|names| names.iter().map(ToString::to_string).collect()),
        hidden: record.hidden.unwrap_or_default(),
        now: record.now.as_ref().map(ToString::to_string),
        r#type: record.proxy_type.clone(),
        udp: record.udp,
        icon: record.icon.clone(),
    }
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
    /// Read and normalize one instance-bound controller snapshot without
    /// changing the legacy proxy cache or its IPC representation.
    #[instrument(skip(api))]
    pub async fn fetch(api: &ApiClient) -> Result<Self> {
        let ProxySnapshot {
            proxies,
            providers,
            groups,
        } = (|| api.proxy_snapshot())
            .retry(*CLASH_API_DEFAULT_BACKOFF_STRATEGY)
            .await?;
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

impl Proxies {
    #[instrument(skip(api))]
    pub async fn fetch(api: &ApiClient) -> Result<Self> {
        let ProxySnapshot {
            proxies,
            providers,
            groups,
        } = (|| api.proxy_snapshot())
            .retry(*CLASH_API_DEFAULT_BACKOFF_STRATEGY)
            .await?;
        Self::from_responses(proxies, &providers, groups)
    }

    /// Project ref's normalized proxy graph to the existing main/legacy UI
    /// response shape without duplicating the core's assembly algorithm.
    pub fn from_responses(
        proxies: IndexMap<ProxyName, Proxy>,
        providers: &IndexMap<ProviderName, ProxyProvider>,
        group_list: Option<IndexMap<ProxyName, Proxy>>,
    ) -> Result<Self> {
        let native = NativeProxies::from_responses(proxies, providers, group_list)?;
        Ok(Self::from_native(native))
    }

    fn from_native(native: NativeProxies) -> Self {
        let NativeProxies {
            global,
            groups,
            nodes,
        } = native;
        let resolve = |name: &ProxyName| {
            // NativeProxies inserts provider-only and unknown members into its
            // single node index, so every legacy member comes from one source.
            proxy_item(&nodes[name])
        };
        let group_item = |group: ProxyGroup| {
            let record = &nodes[&group.name];
            let members = group.all.iter().map(&resolve).collect();
            let mut item: ProxyGroupItem = proxy_item(record).into();
            item.all = members;
            item
        };
        let groups = groups.into_iter().map(&group_item).collect();
        let global = global.map(&group_item).unwrap_or_default();
        let direct = proxy_item(&nodes[&ProxyName::from("DIRECT")]);
        Self {
            global,
            direct,
            groups,
        }
    }
}

pub struct ProxiesGuard {
    inner: Proxies,
    checksum: Option<u32>,
    updated_at: u64,
    generation: u64,
    sender: broadcast::Sender<()>,
}

impl ProxiesGuard {
    pub fn global() -> &'static Arc<RwLock<ProxiesGuard>> {
        static PROXIES: OnceLock<Arc<RwLock<ProxiesGuard>>> = OnceLock::new();
        PROXIES.get_or_init(|| {
            let (tx, _) = broadcast::channel(5); // 默认提供 5 个消费位置，提供一定的缓冲
            Arc::new(RwLock::new(ProxiesGuard {
                checksum: None,
                sender: tx,
                inner: Proxies::default(),
                updated_at: 0,
                generation: 0,
            }))
        })
    }

    pub fn is_updated(&self) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        now - self.updated_at <= 3
    }

    pub fn inner(&self) -> &Proxies {
        &self.inner
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    fn ensure_generation_matches(&self, generation: u64) -> Result<()> {
        if self.generation != generation {
            anyhow::bail!("proxy selection was invalidated by a profile change")
        }
        Ok(())
    }

    pub(crate) fn invalidate(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.checksum = None;
        self.updated_at = 0;
    }

    fn replace_if_generation(&mut self, generation: u64, proxies: Proxies, checksum: u32) -> bool {
        if self.generation != generation {
            return false;
        }
        self.replace(proxies, checksum);
        true
    }

    pub fn replace(&mut self, proxies: Proxies, checksum: u32) {
        let now = chrono::Utc::now().timestamp() as u64;
        self.inner = proxies;
        self.checksum = Some(checksum);
        self.updated_at = now;

        if let Err(e) = self.sender.send(()) {
            log::warn!(
                target: "clash::proxies",
                "send update signal failed: {e:?}"
            );
        }
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

    fn assemble(snapshot: ProxySnapshot) -> Result<Proxies> {
        let ProxySnapshot {
            proxies,
            providers,
            groups,
        } = snapshot;
        Proxies::from_responses(proxies, &providers, groups)
    }

    fn snapshot(items: Vec<Proxy>) -> ProxySnapshot {
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
        ProxySnapshot {
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

    #[test]
    fn all_groups_are_retained_in_global_order_with_remaining_sorted() {
        let data = snapshot(vec![
            native("GLOBAL", "Selector", Some(vec!["B", "A"])),
            native("C", "Fallback", Some(vec!["DIRECT"])),
            native("A", "Selector", Some(vec!["REJECT"])),
            native("B", "URLTest", Some(vec!["DIRECT"])),
        ]);
        let view = assemble(data).unwrap();
        assert_eq!(view.global.name, "GLOBAL");
        assert_eq!(view.direct.name, "DIRECT");
        assert_eq!(
            view.groups
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            ["B", "A", "C"]
        );
        assert_eq!(view.groups[0].all[0].name, "DIRECT");
        assert_eq!(view.groups[1].all[0].name, "REJECT");
    }

    #[test]
    fn authoritative_group_list_replaces_stale_proxy_group_records() {
        let mut data = snapshot(vec![
            native("GLOBAL", "Selector", Some(vec!["old"])),
            native("G", "Selector", Some(vec!["REJECT"])),
        ]);
        data.groups = Some(
            [
                native("GLOBAL", "Selector", Some(vec!["G"])),
                native("G", "URLTest", Some(vec!["DIRECT"])),
            ]
            .into_iter()
            .map(|item| (item.name.clone(), item))
            .collect(),
        );
        let view = assemble(data).unwrap();
        assert_eq!(view.global.all[0].name, "G");
        assert_eq!(view.groups.len(), 1);
        assert_eq!(view.groups[0].r#type, "URLTest");
        assert_eq!(view.groups[0].all[0].name, "DIRECT");
    }

    #[test]
    fn provider_only_nodes_are_resolved_without_hiding_unknown_members() {
        let mut data = snapshot(vec![
            native("GLOBAL", "Selector", Some(vec!["Group"])),
            native(
                "Group",
                "Selector",
                Some(vec!["from-provider", "unknown", "excluded-provider"]),
            ),
        ]);
        for (provider_name, vehicle, node_name) in [
            ("HTTP subscription", "HTTP", "from-provider"),
            ("Compatible provider", "Compatible", "excluded-provider"),
        ] {
            let provider: ProxyProvider = serde_json::from_value(json!({
                "name": provider_name,
                "type": "Proxy",
                "vehicleType": vehicle,
                "proxies": [native(node_name, "Vless", None)],
            }))
            .unwrap();
            data.providers
                .insert(ProviderName::from(provider_name), provider);
        }
        let view = assemble(data).unwrap();
        let group = &view.groups[0];
        assert_eq!(
            group
                .all
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            ["from-provider", "unknown", "excluded-provider"]
        );
        assert_eq!(group.all[0].r#type, "Vless");
        assert_eq!(group.all[1].r#type, "Unknown");
        assert_eq!(group.all[2].r#type, "Unknown");
    }

    #[tokio::test]
    async fn fetch_exposes_provider_only_nodes_from_the_bound_snapshot() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock controller should bind");
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut paths = Vec::new();
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = [0_u8; 4096];
                let read = socket.read(&mut bytes).await.unwrap();
                let request = String::from_utf8_lossy(&bytes[..read]);
                let path = request
                    .split_whitespace()
                    .nth(1)
                    .expect("request path")
                    .to_owned();
                let (status, body) = if path.starts_with("/providers/proxies") {
                    (
                        "200 OK",
                        r#"{"providers":{"subscription":{"name":"subscription","type":"Proxy","vehicleType":"HTTP","proxies":[{"name":"provider-node","type":"Vless","history":[],"udp":false}]}}}"#,
                    )
                } else if path.starts_with("/proxies") {
                    (
                        "200 OK",
                        r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","history":[],"udp":false},"REJECT":{"name":"REJECT","type":"Reject","history":[],"udp":false},"GLOBAL":{"name":"GLOBAL","type":"Selector","history":[],"udp":false,"all":["Group"]},"Group":{"name":"Group","type":"Selector","history":[],"udp":false,"all":["provider-node"]}}}"#,
                    )
                } else if path.starts_with("/group") {
                    ("404 Not Found", r#"{"error":"unsupported"}"#)
                } else {
                    panic!("unexpected proxy fetch request: {request}");
                };
                paths.push(path);
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
            paths
        });

        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: address.port(),
            server: address.to_string(),
            secret: None,
        })
        .unwrap();
        let view = tokio::time::timeout(std::time::Duration::from_secs(5), Proxies::fetch(&api))
            .await
            .expect("legacy proxy view must finish")
            .expect("legacy proxy view should assemble");
        assert_eq!(view.global.all[0].name, "Group");
        assert_eq!(view.groups[0].name, "Group");
        assert_eq!(view.groups[0].all[0].name, "provider-node");
        assert_eq!(view.groups[0].all[0].r#type, "Vless");

        let paths = tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .expect("mock controller should finish")
            .expect("mock controller should not panic");
        assert_eq!(paths.len(), 3);
    }

    #[test]
    fn native_ipc_view_serializes_normalized_nodes_without_legacy_embedded_members() {
        let data = snapshot(vec![
            native("GLOBAL", "Selector", Some(vec!["Group"])),
            native("Group", "Selector", Some(vec!["DIRECT"])),
        ]);
        let view = NativeProxies::from_responses(data.proxies, &data.providers, data.groups)
            .expect("native graph should assemble");
        let json = serde_json::to_value(&view).expect("native IPC view should serialize");
        assert_eq!(json["global"]["name"], "GLOBAL");
        assert_eq!(json["groups"][0]["all"][0], "DIRECT");
        assert_eq!(json["nodes"]["DIRECT"]["name"], "DIRECT");
        assert!(json["groups"][0]["all"][0].is_string());
        assert_eq!(json["groups"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn native_graph_deduplicates_shared_provider_nodes_and_preserves_provider_identity() {
        let mut data = snapshot(vec![
            native("GLOBAL", "Selector", Some(vec!["First", "Second"])),
            native("First", "Selector", Some(vec!["shared", "missing"])),
            native("Second", "Selector", Some(vec!["shared"])),
        ]);
        let provider: ProxyProvider = serde_json::from_value(json!({
            "name": "subscription",
            "type": "Proxy",
            "vehicleType": "HTTP",
            "proxies": [native("shared", "Vless", None)]
        }))
        .unwrap();
        data.providers
            .insert(ProviderName::from("subscription"), provider);
        let native =
            NativeProxies::from_responses(data.proxies, &data.providers, data.groups).unwrap();

        assert_eq!(native.groups.len(), 2);
        assert_eq!(native.nodes.len(), 7); // DIRECT, REJECT, GLOBAL, two groups, shared + missing
        assert_eq!(
            native.nodes[&ProxyName::from("shared")].provider.as_deref(),
            Some("subscription")
        );
        assert_eq!(
            native.nodes[&ProxyName::from("missing")].proxy_type,
            "Unknown"
        );
        assert_eq!(
            native.groups[0].all[0], native.groups[1].all[0],
            "both groups must reference one canonical node"
        );
        let legacy = Proxies::from_native(native);
        assert_eq!(legacy.groups[0].all[0].name, "shared");
        assert_eq!(legacy.groups[1].all[0].name, "shared");
        assert_eq!(legacy.groups[0].all[1].r#type, "Unknown");
    }

    #[test]
    fn missing_required_direct_or_reject_is_rejected() {
        let mut data = snapshot(vec![]);
        data.proxies.swap_remove(&ProxyName::from("DIRECT"));
        assert!(
            assemble(data)
                .unwrap_err()
                .to_string()
                .contains("DIRECT is missing")
        );
        let mut data = snapshot(vec![]);
        data.proxies.swap_remove(&ProxyName::from("REJECT"));
        assert!(
            assemble(data)
                .unwrap_err()
                .to_string()
                .contains("REJECT is missing")
        );
    }

    #[test]
    fn invalidation_marks_the_cached_snapshot_stale() {
        let (sender, _) = broadcast::channel(1);
        let mut guard = ProxiesGuard {
            inner: Proxies {
                global: ProxyGroupItem {
                    name: "cached profile".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            checksum: Some(1),
            updated_at: chrono::Utc::now().timestamp() as u64,
            generation: 0,
            sender,
        };

        assert!(guard.is_updated());

        guard.invalidate();

        assert!(!guard.is_updated());
        assert_eq!(guard.checksum, None);
        assert_eq!(guard.inner().global.name, "cached profile");
    }

    #[test]
    fn invalidated_refresh_cannot_replace_the_snapshot() {
        let (sender, _) = broadcast::channel(1);
        let mut guard = ProxiesGuard {
            inner: Proxies {
                global: ProxyGroupItem {
                    name: "previous profile".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            checksum: Some(1),
            updated_at: chrono::Utc::now().timestamp() as u64,
            generation: 0,
            sender,
        };
        let refresh_generation = guard.generation();

        guard.invalidate();

        assert!(!guard.replace_if_generation(
            refresh_generation,
            Proxies {
                global: ProxyGroupItem {
                    name: "stale profile response".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            2,
        ));
        assert!(!guard.is_updated());
        assert_eq!(guard.inner().global.name, "previous profile");
    }

    #[test]
    fn invalidated_selection_generation_is_rejected() {
        let (sender, _) = broadcast::channel(1);
        let mut guard = ProxiesGuard {
            inner: Proxies::default(),
            checksum: Some(1),
            updated_at: chrono::Utc::now().timestamp() as u64,
            generation: 0,
            sender,
        };
        let selection_generation = guard.generation();

        guard.invalidate();

        assert!(
            guard
                .ensure_generation_matches(selection_generation)
                .is_err()
        );
    }

    #[tokio::test]
    async fn selection_invalidated_while_request_is_in_flight_returns_error() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("controller listener should bind");
        let address = listener.local_addr().expect("controller address");
        let (request_started_tx, request_started_rx) = tokio::sync::oneshot::channel();
        let (continue_response_tx, continue_response_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = tokio::time::timeout(TEST_TIMEOUT, listener.accept())
                .await
                .expect("selection should connect before the deadline")
                .expect("controller should accept the selection");
            let mut reader = BufReader::new(stream);
            let mut request_line = Vec::new();
            let read =
                tokio::time::timeout(TEST_TIMEOUT, reader.read_until(b'\n', &mut request_line))
                    .await
                    .expect("selection request should arrive before the deadline")
                    .expect("selection request line should read");
            assert!(read > 0, "selection request should not be empty");
            assert!(request_line.starts_with(b"PUT /proxies/GLOBAL"));
            request_started_tx
                .send(())
                .expect("test should still be waiting for request");
            tokio::time::timeout(TEST_TIMEOUT, continue_response_rx)
                .await
                .expect("test should release the selection response before the deadline")
                .expect("test should release the selection response");
            tokio::time::timeout(
                TEST_TIMEOUT,
                reader.get_mut().write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                ),
            )
            .await
            .expect("selection response should write before the deadline")
            .expect("selection response should write");
        });

        let (sender, _) = broadcast::channel(1);
        let guard = Arc::new(RwLock::new(ProxiesGuard {
            inner: Proxies::default(),
            checksum: None,
            updated_at: 0,
            generation: 0,
            sender,
        }));
        let generation = guard.read().generation();
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: address.port(),
            server: address.to_string(),
            secret: None,
        })
        .expect("API client should build");
        let guard_for_selection = guard.clone();
        let mut selection = tokio::spawn(async move {
            (&guard_for_selection)
                .select_proxy(&api, generation, "GLOBAL", "DIRECT")
                .await
        });

        tokio::time::timeout(TEST_TIMEOUT, request_started_rx)
            .await
            .expect("selection request should start before the deadline")
            .expect("selection server should report the request");
        guard.write().invalidate();
        continue_response_tx
            .send(())
            .expect("selection server should still be waiting");

        let selection_result = tokio::time::timeout(TEST_TIMEOUT, &mut selection).await;
        if selection_result.is_err() {
            selection.abort();
            let _ = tokio::time::timeout(TEST_TIMEOUT, &mut selection).await;
        }
        let error = selection_result
            .expect("selection task should finish before the deadline")
            .expect("selection task should not panic")
            .expect_err("an invalidated selection must not report success");
        assert!(
            error
                .to_string()
                .contains("snapshot changed while selection was in flight")
        );
        tokio::time::timeout(TEST_TIMEOUT, server)
            .await
            .expect("controller server should finish before the deadline")
            .expect("controller server should not panic");
    }
}

pub trait ProxiesGuardExt {
    async fn update_if_generation(&self, api: &ApiClient, generation: u64) -> Result<bool>;
    async fn select_proxy(
        &self,
        api: &ApiClient,
        generation: u64,
        group: &str,
        name: &str,
    ) -> Result<()>;
}

impl ProxiesGuardExt for &Arc<RwLock<ProxiesGuard>> {
    async fn update_if_generation(&self, api: &ApiClient, generation: u64) -> Result<bool> {
        if self.read().generation() != generation {
            return Ok(false);
        }

        let proxies = Proxies::fetch(api).await?;
        let buf = serde_json::to_string(&proxies)?;
        let checksum = adler::adler32(buf.as_bytes())?;
        let mut writer = self.write();
        if writer.generation() != generation {
            return Ok(false);
        }
        if writer.checksum == Some(checksum) {
            return Ok(true);
        }
        Ok(writer.replace_if_generation(generation, proxies, checksum))
    }

    async fn select_proxy(
        &self,
        api: &ApiClient,
        generation: u64,
        group: &str,
        name: &str,
    ) -> Result<()> {
        self.read().ensure_generation_matches(generation)?;
        api.update_proxy(group, name).await?;
        if !self.update_if_generation(api, generation).await? {
            anyhow::bail!("proxy snapshot changed while selection was in flight");
        }
        Ok(())
    }
}
