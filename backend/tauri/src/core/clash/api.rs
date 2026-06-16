use std::collections::HashMap;

use anyhow::{Context, Result};
use indexmap::IndexMap;
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use specta::Type;
use tauri::http::HeaderMap;
use tracing::instrument;

use crate::config::core::Config;

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

/// PUT /configs
/// path 是绝对路径
#[instrument]
pub async fn put_configs(config_path: &str) -> Result<()> {
    let path = "/configs";

    let mut data = HashMap::new();
    data.insert("path", config_path);

    let _ = perform_request((Method::PUT, path, Data(data))).await?;

    Ok(())
}

#[instrument(skip_all, fields(
    method = tracing::field::Empty,
    url = tracing::field::Empty,
    query = tracing::field::Empty,
    data = tracing::field::Empty,
))]
async fn perform_request<D, Q>(param: impl Into<PerformRequest<D, Q>>) -> Result<reqwest::Response>
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
    let (host, headers) = clash_client_info().context("failed to get clash client info")?;
    let base_url = url::Url::parse(&host).context("failed to parse host")?;
    let opts = url::Url::options().base_url(Some(&base_url));
    let url = opts.parse(&path).context("failed to parse path")?;

    let span = tracing::Span::current();
    span.record("method", tracing::field::display(&method));
    span.record("url", tracing::field::display(&url));
    span.record("query", tracing::field::debug(&query));
    span.record("data", tracing::field::debug(&data));

    async {
        let client = reqwest::ClientBuilder::new().no_proxy().build()?;
        let mut builder = client.request(method.clone(), url.clone()).headers(headers);

        if let Some(query) = &query {
            builder = builder.query(query);
        }
        if let Some(data) = &data {
            builder = builder.json(data);
        }

        let resp = builder.send().await?;

        if let Err(err) = resp.error_for_status_ref() {
            match err.status() {
                // Try To parse error message
                Some(StatusCode::BAD_REQUEST) => {
                    let Ok(bytes) = resp.bytes().await else {
                        return Err(err.into());
                    };

                    let message: serde_json::Value = match serde_json::from_slice(&bytes) {
                        Ok(v) => v,
                        Err(_) => {
                            let s = String::from_utf8_lossy(&bytes);
                            serde_json::Value::String(s.to_string())
                        }
                    };

                    return Err(err).context(format!("message: {message}"));
                }
                _ => return Err(err).context("clash api error"),
            }
        }
        Ok(resp)
    }
    .await
    .inspect_err(|e| tracing::error!(method = %method, url = %url, query = ?query, data = ?data, "failed to perform request: {:?}", e))
}

/// 根据clash info获取clash服务地址和请求头
#[instrument]
fn clash_client_info() -> Result<(String, HeaderMap)> {
    let client = { Config::clash().data().get_client_info() };

    let server = format!("http://{}", client.server);

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", "application/json".parse()?);

    if let Some(secret) = client.secret {
        let secret = format!("Bearer {secret}").parse()?;
        headers.insert("Authorization", secret);
    }

    Ok((server, headers))
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

/// GET /proxies
/// 获取代理列表
#[instrument]
pub async fn get_proxies() -> Result<ProxiesRes> {
    let path = "/proxies";
    let resp: ProxiesRes = perform_request((Method::GET, path)).await?.json().await?;
    Ok(resp)
}

/// DELETE /connections
/// Close all connections or a specific connection by ID
#[instrument]
pub async fn delete_connections(id: Option<&str>) -> Result<()> {
    let path = match id {
        Some(id) => format!("/connections/{}", id),
        None => "/connections".to_string(),
    };

    let _ = perform_request((Method::DELETE, path.as_str())).await?;
    Ok(())
}

/// PUT /proxies/{group}
/// 选择代理
/// group: 代理分组名称
/// name: 代理名称
#[instrument]
pub async fn update_proxy(group: &str, name: &str) -> Result<()> {
    let path = format!("/proxies/{group}");

    let mut data = HashMap::new();
    data.insert("name", name);

    let _ = perform_request((Method::PUT, path.as_str(), Data(data))).await?;
    Ok(())
}

/// PATCH /configs
#[instrument]
pub async fn patch_configs(config: &Mapping) -> Result<()> {
    let path = "/configs";
    let _ = perform_request((Method::PATCH, path, Data(config))).await?;
    Ok(())
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, Type)]
pub struct DelayRes {
    delay: u64,
}

/// GET /proxies/{name}/delay
/// 获取代理延迟
#[instrument]
pub async fn get_proxy_delay(name: String, test_url: Option<String>) -> Result<DelayRes> {
    let path = format!("/proxies/{name}/delay");
    let default_url = "http://www.gstatic.com/generate_204";
    let test_url = test_url
        .map(|s| if s.is_empty() { default_url.into() } else { s })
        .unwrap_or(default_url.into());

    let query = Query([("timeout", "10000"), ("url", &test_url)]);
    let resp: DelayRes = perform_request((Method::GET, path.as_str(), query))
        .await?
        .json()
        .await?;
    Ok(resp)
}

/// GET /group/:name/delay
#[instrument]
pub async fn get_group_delay(group: String, url: Option<String>) -> Result<HashMap<String, u32>> {
    let path = format!("/group/{group}/delay");
    let default_url = "http://www.gstatic.com/generate_204";
    let test_url = url
        .map(|s| if s.is_empty() { default_url.into() } else { s })
        .unwrap_or(default_url.into());

    let query = Query([("timeout", "10000"), ("url", &test_url)]);
    let resp: HashMap<String, u32> = perform_request((Method::GET, path.as_str(), query))
        .await?
        .json()
        .await?;
    Ok(resp)
}
