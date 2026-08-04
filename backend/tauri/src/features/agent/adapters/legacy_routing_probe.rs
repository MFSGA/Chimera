use std::time::Duration;

use reqwest::redirect::Policy;
use serde::Deserialize;

use crate::config::core::Config;

use super::super::{
    core_probe::loopback_controller_url, model::AgentRoutingMode, ports::CoreRoutingProbePort,
};

const CORE_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
struct CoreConfigResponse {
    mode: Option<String>,
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
    async fn observed_mode(&self) -> Result<AgentRoutingMode, ()> {
        let info = Config::clash().latest().get_client_info();
        let url = loopback_controller_url(&info.server)?;
        let client = reqwest::ClientBuilder::new()
            .no_proxy()
            .redirect(Policy::none())
            .timeout(CORE_PROBE_TIMEOUT)
            .build()
            .map_err(|_| ())?;
        let mut request = client.get(url);
        if let Some(secret) = info.secret.filter(|secret| !secret.is_empty()) {
            request = request.bearer_auth(secret);
        }
        let response = request.send().await.map_err(|_| ())?;
        let config = response
            .error_for_status()
            .map_err(|_| ())?
            .json::<CoreConfigResponse>()
            .await
            .map_err(|_| ())?;
        config
            .mode
            .as_deref()
            .and_then(AgentRoutingMode::parse)
            .ok_or(())
    }
}
