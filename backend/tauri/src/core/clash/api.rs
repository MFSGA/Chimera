use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result};
use indexmap::IndexMap;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use tauri::http::HeaderMap;
use tracing::instrument;

/// TUN runtime projection returned by the running core's `GET /configs` endpoint.
///
/// Keep the field names aligned with upstream `clash_api::RuntimeTun`. This is
/// intentionally limited to the fields Chimera currently consumes; serde
/// ignores additional runtime fields until their shared callers need them.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
#[serde(default, rename_all = "kebab-case")]
pub struct RuntimeTun {
    pub enable: bool,
    pub device: String,
    pub auto_route: bool,
    pub auto_detect_interface: bool,
    pub strict_route: bool,
    pub route_address: Vec<String>,
    pub route_exclude_address: Vec<String>,
    pub inet4_route_address: Vec<String>,
    pub inet6_route_address: Vec<String>,
}

/// Runtime state returned by the running core's `GET /configs` endpoint.
#[derive(Debug, Clone, Default, Deserialize, Serialize, Type)]
pub struct ClashRuntimeConfig {
    pub port: Option<u16>,
    pub mode: Option<String>,
    pub ipv6: Option<bool>,
    #[serde(rename = "socket-port")]
    pub socket_port: Option<u16>,
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<u16>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<u16>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<u16>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<u16>,
    #[serde(rename = "external-controller")]
    pub external_controller: Option<String>,
    pub secret: Option<String>,
    #[specta(skip)]
    pub tun: Option<RuntimeTun>,
}

/// A newtype wrapper for query parameters
struct Query<T>(T);
/// A newtype wrapper for request body
struct Data<T>(T);

impl From<(reqwest::Method, &str)> for PerformRequest<(), ()> {
    fn from((method, path): (reqwest::Method, &str)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Data<T>)> for PerformRequest<T, ()>
where
    T: Serialize,
{
    fn from((method, path, Data(data)): (reqwest::Method, &str, Data<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: Some(data),
            query: None,
        }
    }
}

impl<T> From<(reqwest::Method, &str, Query<T>)> for PerformRequest<(), T>
where
    T: Serialize,
{
    fn from((method, path, Query(query)): (reqwest::Method, &str, Query<T>)) -> Self {
        Self {
            method,
            path: path.to_string(),
            data: None,
            query: Some(query),
        }
    }
}

/// The Request Parameters
struct PerformRequest<D = (), Q = ()> {
    method: reqwest::Method,
    path: String,
    query: Option<Q>,
    data: Option<D>,
}

#[derive(Clone)]
pub(crate) struct ApiClient {
    base_url: url::Url,
    headers: HeaderMap,
}

impl ApiClient {
    pub(crate) fn new(info: crate::config::clash::ClashInfo) -> Result<Self> {
        let base_url =
            url::Url::parse(&format!("http://{}", info.server)).context("failed to parse host")?;
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/json".parse()?);
        if let Some(secret) = info.secret {
            headers.insert("Authorization", format!("Bearer {secret}").parse()?);
        }
        Ok(Self { base_url, headers })
    }

    /// Wait until the applied core's HTTP controller is actually accepting
    /// authenticated requests. Process liveness alone is not API readiness.
    pub(crate) async fn wait_until_ready(&self, timeout: Duration) -> Result<()> {
        let http_client = reqwest::ClientBuilder::new()
            .no_proxy()
            .timeout(Duration::from_secs(1))
            .build()?;
        let url = url::Url::options()
            .base_url(Some(&self.base_url))
            .parse("/configs")
            .context("failed to create controller readiness URL")?;
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            match http_client
                .get(url.clone())
                .headers(self.headers.clone())
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
            {
                Ok(_) => return Ok(()),
                Err(error) if tokio::time::Instant::now() >= deadline => {
                    return Err(error).context(format!(
                        "Clash controller {} did not become ready within {timeout:?}",
                        self.base_url
                    ));
                }
                Err(_) => {}
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[instrument(skip_all, fields(
        method = tracing::field::Empty,
        url = tracing::field::Empty,
        query = tracing::field::Empty,
        data = tracing::field::Empty,
    ))]
    async fn perform_request<D, Q>(
        &self,
        param: impl Into<PerformRequest<D, Q>>,
    ) -> Result<reqwest::Response>
    where
        Q: Serialize + core::fmt::Debug,
        D: Serialize + core::fmt::Debug,
    {
        let PerformRequest {
            method,
            path,
            data,
            query,
        } = param.into();
        let opts = url::Url::options().base_url(Some(&self.base_url));
        let url = opts.parse(&path).context("failed to parse path")?;

        let span = tracing::Span::current();
        span.record("method", tracing::field::display(&method));
        span.record("url", tracing::field::display(&url));
        span.record("query", tracing::field::debug(&query));
        span.record("data", tracing::field::debug(&data));

        async {
            let http_client = reqwest::ClientBuilder::new().no_proxy().build()?;
            let mut builder = http_client
                .request(method.clone(), url.clone())
                .headers(self.headers.clone());

            if let Some(query) = &query {
                builder = builder.query(query);
            }
            if let Some(data) = &data {
                builder = builder.json(data);
            }

            let response = builder.send().await?;
            if let Err(error) = response.error_for_status_ref() {
                match error.status() {
                    Some(StatusCode::BAD_REQUEST) => {
                        let Ok(bytes) = response.bytes().await else {
                            return Err(error.into());
                        };
                        let message: serde_json::Value = match serde_json::from_slice(&bytes) {
                            Ok(value) => value,
                            Err(_) => {
                                serde_json::Value::String(String::from_utf8_lossy(&bytes).into())
                            }
                        };
                        return Err(error).context(format!("message: {message}"));
                    }
                    _ => return Err(error).context("clash api error"),
                }
            }
            Ok(response)
        }
        .await
        .inspect_err(|error| tracing::error!(method = %method, url = %url, query = ?query, data = ?data, "failed to perform request: {:?}", error))
    }

    pub(crate) async fn get_configs(&self) -> Result<ClashRuntimeConfig> {
        Ok(self
            .perform_request((Method::GET, "/configs"))
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn get_proxies(&self) -> Result<ProxiesRes> {
        Ok(self
            .perform_request((Method::GET, "/proxies"))
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn get_connections(&self) -> Result<ConnectionsRes> {
        Ok(self
            .perform_request((Method::GET, "/connections"))
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn delete_connections(&self, id: Option<&str>) -> Result<()> {
        let path = match id {
            Some(id) => format!("/connections/{id}"),
            None => "/connections".to_string(),
        };
        self.perform_request((Method::DELETE, path.as_str()))
            .await?;
        Ok(())
    }

    pub(crate) async fn update_proxy(&self, group: &str, name: &str) -> Result<()> {
        let path = format!("/proxies/{group}");
        let mut data = HashMap::new();
        data.insert("name", name);
        self.perform_request((Method::PUT, path.as_str(), Data(data)))
            .await?;
        Ok(())
    }

    pub(crate) async fn patch_configs(&self, config: &Mapping) -> Result<()> {
        self.perform_request((Method::PATCH, "/configs", Data(config)))
            .await?;
        Ok(())
    }

    pub(crate) async fn get_proxy_delay(
        &self,
        name: String,
        test_url: Option<String>,
    ) -> Result<DelayRes> {
        let path = format!("/proxies/{name}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = test_url
            .map(|value| {
                if value.is_empty() {
                    default_url.into()
                } else {
                    value
                }
            })
            .unwrap_or(default_url.into());
        let query = Query([("timeout", "10000"), ("url", &test_url)]);
        Ok(self
            .perform_request((Method::GET, path.as_str(), query))
            .await?
            .json()
            .await?)
    }

    pub(crate) async fn get_group_delay(
        &self,
        group: String,
        url: Option<String>,
    ) -> Result<HashMap<String, u32>> {
        let path = format!("/group/{group}/delay");
        let default_url = "http://www.gstatic.com/generate_204";
        let test_url = url
            .map(|value| {
                if value.is_empty() {
                    default_url.into()
                } else {
                    value
                }
            })
            .unwrap_or(default_url.into());
        let query = Query([("timeout", "10000"), ("url", &test_url)]);
        Ok(self
            .perform_request((Method::GET, path.as_str(), query))
            .await?
            .json()
            .await?)
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
    use std::time::Duration;

    use super::{ApiClient, ClashRuntimeConfig};

    #[tokio::test]
    async fn readiness_requires_an_responding_controller() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let address = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("probe should connect");
            let mut request = [0_u8; 1024];
            let read = stream
                .read(&mut request)
                .await
                .expect("request should read");
            assert!(read > 0, "readiness probe must send an HTTP request");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}")
                .await
                .expect("response should write");
            stream.shutdown().await.expect("response should close");
        });
        let api = ApiClient::new(crate::config::clash::ClashInfo {
            port: address.port(),
            server: address.to_string(),
            secret: None,
        })
        .expect("API client should build");

        api.wait_until_ready(Duration::from_secs(1))
            .await
            .expect("controller response should satisfy readiness");
        server.await.expect("server task should finish");
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
