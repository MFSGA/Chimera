use anyhow::Result;
use backon::Retryable;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::sync::{Arc, OnceLock};
use tokio::{sync::broadcast, try_join};
use tracing::instrument;

use crate::core::clash::{CLASH_API_DEFAULT_BACKOFF_STRATEGY, api};

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

/// todo: fetch proxies and ProvidersProxiesRes
async fn fetch_proxies(api: &api::ApiClient) -> Result<api::ProxiesRes> {
    api.get_proxies().await
}

impl Proxies {
    #[instrument(skip(api))]
    pub async fn fetch(api: &api::ApiClient) -> Result<Self> {
        let inner_proxies = (|| fetch_proxies(api))
            .retry(*CLASH_API_DEFAULT_BACKOFF_STRATEGY)
            .await?;
        let inner_proxies = inner_proxies.proxies;

        let generate_item = |name: &str| {
            if let Some(r) = inner_proxies.get(name) {
                r.clone()
            } else {
                api::ProxyItem {
                    name: name.to_string(),
                    r#type: "Unknown".to_string(),
                    udp: false,
                    history: vec![],
                    ..Default::default()
                }
            }
        };

        let global = inner_proxies.get("GLOBAL");
        let direct = inner_proxies
            .get("DIRECT")
            .ok_or(anyhow::anyhow!("DIRECT is missing in /proxies"))?
            .clone(); // It should be always exists
        let reject = inner_proxies
            .get("REJECT")
            .ok_or(anyhow::anyhow!("REJECT is missing in /proxies"))?
            .clone(); // It should be always exists

        // 3. generate the proxies groups
        let groups: Vec<ProxyGroupItem> = match global {
            Some(api::ProxyItem { all: Some(all), .. }) => {
                tracing::debug!("all: {all:#?}");
                let all = all.clone();
                all.into_iter()
                    .filter(|name| {
                        matches!(
                            inner_proxies.get(name),
                            Some(api::ProxyItem { all: Some(_), .. })
                        )
                    })
                    .map(|name| {
                        let item = inner_proxies
                            .get(&name)
                            .unwrap_or(&api::ProxyItem::default())
                            .clone();
                        let item_all = item.all.clone().unwrap_or_default();
                        let mut item: ProxyGroupItem = item.into();
                        item.all = item_all
                            .into_iter()
                            .map(|name| generate_item(&name))
                            .collect();
                        item
                    })
                    .collect()
            }
            _ => {
                let mut groups: Vec<ProxyGroupItem> = inner_proxies
                    .clone()
                    .into_values()
                    .filter(|v| v.name == "GLOBAL" && v.all.is_some())
                    .map(|v| {
                        let all = v.all.clone().unwrap_or_default();
                        let mut item: ProxyGroupItem = v.clone().into();
                        item.all = all.into_iter().map(|name| generate_item(&name)).collect();
                        item
                    })
                    .collect();
                groups.sort_by(|a, b| b.name.to_lowercase().cmp(&a.name.to_lowercase()));
                groups
            }
        };

        // 4. generate the proxies
        let mut proxies: Vec<api::ProxyItem> = vec![direct.clone(), reject];
        proxies.extend(inner_proxies.clone().into_values().filter(|v| {
            matches!(v.name.as_str(), "DIRECT" | "REJECT")
                && (v.all.is_none() || v.all.as_ref().unwrap().is_empty())
        }));

        // 5. generate the global
        let global: Option<ProxyGroupItem> = global.map(|v| {
            let all = v.all.clone().unwrap_or_default();
            let mut item: ProxyGroupItem = v.clone().into();
            item.all = all.into_iter().map(|name| generate_item(&name)).collect();
            item
        });

        Ok(Proxies {
            global: global.unwrap_or_default(),
            direct,
            groups,
            // records: inner_proxies,
        })
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
        let api = api::ApiClient::new(crate::config::clash::ClashInfo {
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
    async fn update_if_generation(&self, api: &api::ApiClient, generation: u64) -> Result<bool>;
    async fn select_proxy(
        &self,
        api: &api::ApiClient,
        generation: u64,
        group: &str,
        name: &str,
    ) -> Result<()>;
}

impl ProxiesGuardExt for &Arc<RwLock<ProxiesGuard>> {
    async fn update_if_generation(&self, api: &api::ApiClient, generation: u64) -> Result<bool> {
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
        api: &api::ApiClient,
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
