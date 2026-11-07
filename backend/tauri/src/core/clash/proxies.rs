//! Tauri-only compatibility projection, lifecycle guard, and IPC fetch adapter.
//! Canonical native graph assembly is owned by `chimera_core::clash::proxies`.
use anyhow::Result;
use backon::Retryable;
use clash_api::{IndexMap, ProviderName, Proxy, ProxyName, ProxyProvider};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, OnceLock};
use tokio::{sync::broadcast, try_join};
use tracing::instrument;

use crate::core::actor_v2::api::ApiClient;
#[cfg(test)]
use crate::core::actor_v2::api::ProxySnapshot;
use crate::core::clash::{CLASH_API_DEFAULT_BACKOFF_STRATEGY, api};

pub use chimera_core::clash::proxies::{
    NativeProxies, ProxyGroup, ProxyGroupCapabilities, ProxyGroupKind, ProxyProviderSummary,
};

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

/// Fetch the native graph through the Tauri-owned instance-bound controller.
/// Assembly and schema live in the independent core crate, not in this adapter.
#[instrument(skip(api))]
pub async fn fetch_native_proxies(api: &ApiClient) -> Result<NativeProxies> {
    (|| NativeProxies::fetch(api))
        .retry(*CLASH_API_DEFAULT_BACKOFF_STRATEGY)
        .await
}

impl Proxies {
    #[instrument(skip(api))]
    pub async fn fetch(api: &ApiClient) -> Result<Self> {
        let native = fetch_native_proxies(api).await?;
        Ok(Self::from_native(native))
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

#[cfg(test)]
#[path = "proxy_pin_tests.rs"]
mod proxy_pin_tests;

pub trait ProxiesGuardExt {
    async fn update_if_generation(&self, api: &ApiClient, generation: u64) -> Result<bool>;
    async fn select_proxy(
        &self,
        api: &ApiClient,
        generation: u64,
        group: &str,
        name: &str,
    ) -> Result<()>;
    async fn clear_proxy_fixed(&self, api: &ApiClient, generation: u64, group: &str) -> Result<()>;
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

    async fn clear_proxy_fixed(&self, api: &ApiClient, generation: u64, group: &str) -> Result<()> {
        self.read().ensure_generation_matches(generation)?;
        let mutation = api.clear_proxy_selection(&ProxyName::new(group)).await;
        // A rejected/timeout DELETE may still have applied. Read the core's
        // true state even on errors, without replacing an invalidated profile.
        let refresh = self.update_if_generation(api, generation).await;
        if let Err(error) = mutation {
            if let Err(refresh_error) = refresh {
                tracing::debug!(%refresh_error, "proxy refresh after unconfirmed unpin failed");
            }
            return Err(error.into());
        }
        if !refresh? {
            anyhow::bail!("proxy snapshot changed while unpinning was in flight");
        }
        Ok(())
    }
}
