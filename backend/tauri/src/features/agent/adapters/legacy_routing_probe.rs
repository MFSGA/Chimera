use std::time::Duration;

use futures_util::StreamExt;
use reqwest::redirect::Policy;
use serde::Deserialize;
use url::Url;

use crate::config::core::Config;

use super::super::{
    core_probe::loopback_controller_url,
    model::AgentRoutingMode,
    ports::{CoreRoutingProbePort, CoreRuntimeObservation},
};

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CORE_CONFIG_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
struct CoreConfigResponse {
    mode: Option<String>,
    tun: Option<CoreTunResponse>,
}

#[derive(Deserialize)]
struct CoreTunResponse {
    enable: Option<bool>,
}

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: controller connection settings are still exposed through Config globals.
// Remove when: ConfigClient is injected through NyanpasuClient.
pub(crate) struct LegacyCoreRoutingProbe;

impl LegacyCoreRoutingProbe {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl CoreRoutingProbePort for LegacyCoreRoutingProbe {
    async fn observed_configuration(&self) -> Result<CoreRuntimeObservation, ()> {
        let info = Config::clash().latest().get_client_info();
        let url = loopback_controller_url(&info.server)?;
        fetch_observed_configuration(url, info.secret.filter(|secret| !secret.is_empty())).await
    }
}

async fn fetch_observed_configuration(
    url: Url,
    secret: Option<String>,
) -> Result<CoreRuntimeObservation, ()> {
    let client = reqwest::ClientBuilder::new()
        .no_proxy()
        .redirect(Policy::none())
        .timeout(CORE_PROBE_TIMEOUT)
        .build()
        .map_err(|_| ())?;
    let mut request = client.get(url);
    if let Some(secret) = secret {
        request = request.bearer_auth(secret);
    }
    let response = request.send().await.map_err(|_| ())?;
    let response = response.error_for_status().map_err(|_| ())?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CORE_CONFIG_RESPONSE_BYTES as u64)
    {
        return Err(());
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(MAX_CORE_CONFIG_RESPONSE_BYTES),
    );
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if body.len().saturating_add(chunk.len()) > MAX_CORE_CONFIG_RESPONSE_BYTES {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }

    let config = serde_json::from_slice::<CoreConfigResponse>(&body).map_err(|_| ())?;
    let routing_mode = config
        .mode
        .as_deref()
        .and_then(AgentRoutingMode::parse)
        .ok_or(())?;
    Ok(CoreRuntimeObservation {
        routing_mode,
        tun_enabled: config.tun.and_then(|tun| tun.enable),
    })
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, sync::Arc};

    use axum::{
        Router,
        body::{Body, Bytes},
        response::Response,
        routing::get,
    };
    use tokio::net::TcpListener;
    use url::Url;

    use super::{MAX_CORE_CONFIG_RESPONSE_BYTES, fetch_observed_configuration};
    use crate::features::agent::{model::AgentRoutingMode, ports::CoreRuntimeObservation};

    async fn serve(body: Vec<u8>, chunked: bool) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind routing probe server");
        let address = listener.local_addr().expect("routing probe address");
        let body = Arc::new(body);
        let task = tokio::spawn(async move {
            let router = Router::new().route(
                "/configs",
                get(move || {
                    let body = body.clone();
                    async move {
                        let response_body = if chunked {
                            Body::from_stream(futures_util::stream::once(async move {
                                Ok::<_, Infallible>(Bytes::copy_from_slice(body.as_slice()))
                            }))
                        } else {
                            Body::from(body.as_ref().clone())
                        };
                        Response::builder()
                            .status(200)
                            .body(response_body)
                            .expect("valid routing probe response")
                    }
                }),
            );
            axum::serve(listener, router)
                .await
                .expect("serve routing probe response");
        });
        (
            Url::parse(&format!("http://{address}/configs")).expect("routing probe URL"),
            task,
        )
    }

    #[tokio::test]
    async fn routing_probe_accepts_bounded_valid_json_with_tun_state() {
        let (url, task) = serve(
            r#"{"mode":"rule","tun":{"enable":false},"ignored":true}"#
                .as_bytes()
                .to_vec(),
            false,
        )
        .await;

        assert_eq!(
            fetch_observed_configuration(url, None).await,
            Ok(CoreRuntimeObservation {
                routing_mode: AgentRoutingMode::Rule,
                tun_enabled: Some(false),
            })
        );
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn routing_probe_keeps_routing_state_when_tun_status_is_missing() {
        let (url, task) = serve(r#"{"mode":"global"}"#.as_bytes().to_vec(), false).await;

        assert_eq!(
            fetch_observed_configuration(url, None).await,
            Ok(CoreRuntimeObservation {
                routing_mode: AgentRoutingMode::Global,
                tun_enabled: None,
            })
        );
        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn routing_probe_rejects_oversized_chunked_json() {
        let oversized = format!(
            r#"{{"mode":"rule","padding":"{}"}}"#,
            "x".repeat(MAX_CORE_CONFIG_RESPONSE_BYTES)
        );
        let (url, task) = serve(oversized.into_bytes(), true).await;

        assert!(fetch_observed_configuration(url, None).await.is_err());
        task.abort();
        let _ = task.await;
    }
}
