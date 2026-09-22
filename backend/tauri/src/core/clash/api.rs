use std::collections::HashMap;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use tracing::instrument;

pub use clash_api::RuntimeConfig as ClashRuntimeConfig;
#[cfg(test)]
pub use clash_api::RuntimeTun;

#[derive(Clone)]
pub(crate) struct ApiClient {
    client: clash_api::Client,
}

impl ApiClient {
    pub(crate) fn new(info: crate::config::clash::ClashInfo) -> Result<Self> {
        let host = clash_api::Host::http(&info.server).context("failed to parse Clash API host")?;
        Ok(Self {
            client: clash_api::Client::with_secret(host, info.secret.unwrap_or_default()),
        })
    }

    pub(crate) fn from_connection(
        connection: chimera_ipc::api::core::v2::CoreApiConnection,
    ) -> Result<Self> {
        use chimera_ipc::api::core::v2::CoreControllerInfo;

        let host = match connection.controller {
            CoreControllerInfo::Http(url) => {
                clash_api::Host::url(&url).context("failed to parse bound Clash API URL")?
            }
            CoreControllerInfo::UnixSocket(path) => clash_api::Host::unix_socket(path),
            CoreControllerInfo::NamedPipe(path) => clash_api::Host::named_pipe(path),
        };
        Ok(Self {
            client: clash_api::Client::with_secret(host, connection.secret.unwrap_or_default()),
        })
    }

    pub(crate) async fn get_configs(&self) -> Result<ClashRuntimeConfig> {
        Ok(self.client.configs().await?)
    }

    pub(crate) async fn get_proxies(&self) -> Result<ProxiesRes> {
        Ok(self.client.get_json("/proxies").await?)
    }

    pub(crate) async fn get_connections(&self) -> Result<ConnectionsRes> {
        Ok(self.client.get_json("/connections").await?)
    }

    pub(crate) async fn delete_connections(&self, id: Option<&str>) -> Result<()> {
        let path = match id {
            Some(id) => format!("/connections/{id}"),
            None => "/connections".to_string(),
        };
        Ok(self.client.delete(&path).await?)
    }

    pub(crate) async fn update_proxy(&self, group: &str, name: &str) -> Result<()> {
        let path = format!("/proxies/{group}");
        let mut data = HashMap::new();
        data.insert("name", name);
        Ok(self.client.put_json(&path, &data).await?)
    }

    pub(crate) async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        Ok(self.client.patch_json("/configs", config).await?)
    }

    pub(crate) async fn get_proxy_delay(
        &self,
        name: String,
        test_url: Option<String>,
    ) -> Result<DelayRes> {
        let path = format!("/proxies/{name}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = test_url
            .map(|s| if s.is_empty() { default_url.into() } else { s })
            .unwrap_or(default_url.into());
        let query = [("timeout", "10000"), ("url", test_url.as_str())];
        Ok(self.client.get_json_query(&path, &query).await?)
    }

    pub(crate) async fn get_group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<HashMap<String, u32>> {
        let path = format!("/group/{group}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = url
            .map(|s| if s.is_empty() { default_url.into() } else { s })
            .unwrap_or(default_url.into());
        let query = [("timeout", "10000"), ("url", test_url.as_str())];
        Ok(self.client.get_json_query(&path, &query).await?)
    }
}

/// 缩短clash的日志
#[instrument]
pub fn parse_log(log: String) -> String {
    if log.starts_with("time=") && log.len() > 33 {
        return log[33..].to_owned();
    }
    if log.len() > 9 {
        return log[9..].to_owned();
    }
    log
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItemHistory {
    pub time: String,
    pub delay: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxyItem {
    pub name: String,
    pub history: Vec<ProxyItemHistory>,
    pub all: Option<Vec<String>>,
    #[serde(default)]
    pub hidden: bool, // Mihomo Only
    pub now: Option<String>, // 当前选中的代理
    pub r#type: String,      // TODO: 考虑改成枚举
    pub udp: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>, // Mihomo Only
}

#[derive(Debug, Clone, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProxiesRes {
    #[serde(default)]
    pub proxies: IndexMap<String, ProxyItem>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionItem {
    pub id: String,
    #[serde(default)]
    pub chains: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionsRes {
    #[serde(default)]
    pub connections: Vec<ConnectionItem>,
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}

#[cfg(test)]
mod tests {
    use super::{ApiClient, ClashRuntimeConfig};

    #[test]
    fn instance_bound_connections_build_transport_aware_api_client() {
        use chimera_ipc::api::core::v2::{CoreApiConnection, CoreControllerInfo};

        let http = ApiClient::from_connection(CoreApiConnection {
            instance_id: "service-http".to_string(),
            controller: CoreControllerInfo::Http("http://127.0.0.1:9090".to_string()),
            secret: Some("token".to_string()),
        })
        .unwrap();
        assert!(matches!(http.client.host(), clash_api::Host::Http(_)));

        let unix = ApiClient::from_connection(CoreApiConnection {
            instance_id: "service-unix".to_string(),
            controller: CoreControllerInfo::UnixSocket("/tmp/chimera.sock".to_string()),
            secret: None,
        })
        .unwrap();
        assert!(matches!(
            unix.client.host(),
            clash_api::Host::UnixSocket(path) if path == std::path::Path::new("/tmp/chimera.sock")
        ));

        let pipe = ApiClient::from_connection(CoreApiConnection {
            instance_id: "service-pipe".to_string(),
            controller: CoreControllerInfo::NamedPipe("chimera-pipe".to_string()),
            secret: None,
        })
        .unwrap();
        assert!(matches!(
            pipe.client.host(),
            clash_api::Host::NamedPipe(path) if path == std::path::Path::new("chimera-pipe")
        ));
    }

    #[test]
    fn runtime_tun_deserializes_mihomo_kebab_case_fields() {
        let config: ClashRuntimeConfig = serde_json::from_value(serde_json::json!({
            "mode": "rule",
            "tun": {
                "enable": true,
                "device": "Meta",
                "auto-route": true,
                "auto-detect-interface": true,
                "strict-route": false,
                "route-address": ["0.0.0.0/1"],
                "route-exclude-address": ["192.168.0.0/16"],
                "inet4-route-address": ["128.0.0.0/1"],
                "inet6-route-address": ["::/1", "8000::/1"]
            }
        }))
        .expect("runtime config should deserialize");

        let tun = config.tun.expect("TUN runtime projection");
        assert!(tun.enable);
        assert_eq!(tun.device, "Meta");
        assert!(tun.auto_route);
        assert!(tun.auto_detect_interface);
        assert_eq!(tun.route_address, vec!["0.0.0.0/1"]);
        assert_eq!(tun.inet4_route_address, vec!["128.0.0.0/1"]);
        assert_eq!(tun.inet6_route_address, vec!["::/1", "8000::/1"]);
    }
}
