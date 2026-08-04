use std::time::Duration;

use reqwest::redirect::Policy;
use serde::Deserialize;
use tokio::time::timeout;

use super::super::ports::AgentBridgeHealthPort;

const BRIDGE_HEALTH_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_HEALTH_RESPONSE_BYTES: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HealthCheckResponse {
    status: String,
    schema_version: u16,
}

pub(crate) struct HttpBridgeHealth;

impl HttpBridgeHealth {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl AgentBridgeHealthPort for HttpBridgeHealth {
    async fn is_healthy(&self, health_url: &str, schema_version: u16) -> bool {
        let client = match reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .build()
        {
            Ok(client) => client,
            Err(_) => return false,
        };
        matches!(
            timeout(
                BRIDGE_HEALTH_TIMEOUT,
                verify_health_response(client, health_url, schema_version),
            )
            .await,
            Ok(true)
        )
    }
}

async fn verify_health_response(
    client: reqwest::Client,
    health_url: &str,
    schema_version: u16,
) -> bool {
    let Ok(mut response) = client.get(health_url).send().await else {
        return false;
    };
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_HEALTH_RESPONSE_BYTES as u64)
    {
        return false;
    }

    let mut body = Vec::with_capacity(MAX_HEALTH_RESPONSE_BYTES);
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(_) => return false,
        };
        if body.len() + chunk.len() > MAX_HEALTH_RESPONSE_BYTES {
            return false;
        }
        body.extend_from_slice(&chunk);
    }
    matches!(
        serde_json::from_slice::<HealthCheckResponse>(&body),
        Ok(payload) if payload.status == "ok" && payload.schema_version == schema_version
    )
}
