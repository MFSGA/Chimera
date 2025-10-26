use std::collections::HashMap;

use anyhow::{Context, Result};
use reqwest::{Method, StatusCode};
use serde::Serialize;
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
