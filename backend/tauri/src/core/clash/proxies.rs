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
async fn fetch_proxies() -> Result<api::ProxiesRes> {
    api::get_proxies().await
}

impl Proxies {
    #[instrument]
    pub async fn fetch() -> Result<Self> {
        let inner_proxies = fetch_proxies
            .retry(*CLASH_API_DEFAULT_BACKOFF_STRATEGY)
            .await?;
        let inner_proxies = inner_proxies.proxies;

        let generate_item = |name: &str| {
            if let Some(r) = inner_proxies.get(name) {
                r.clone()
            } else {
                api::ProxyItem {
                    name: name.to_string(),
                    // r#type: "Unknown".to_string(),
                    // udp: false,
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

pub trait ProxiesGuardExt {
    async fn update(&self) -> Result<()>;
    async fn select_proxy(&self, group: &str, name: &str) -> Result<()>;
}

type ProxiesGuardSingleton = &'static Arc<RwLock<ProxiesGuard>>;
impl ProxiesGuardExt for ProxiesGuardSingleton {
    async fn update(&self) -> Result<()> {
        let proxies = Proxies::fetch().await?;
        let buf = serde_json::to_string(&proxies)?;
        let checksum = adler::adler32(buf.as_bytes())?;
        {
            let reader = self.read();
            if reader.checksum == Some(checksum) {
                return Ok(());
            }
        }
        let mut writer = self.write();
        writer.replace(proxies, checksum);
        Ok(())
    }

    async fn select_proxy(&self, group: &str, name: &str) -> Result<()> {
        api::update_proxy(group, name).await?;
        self.update().await?;
        Ok(())
    }
}
