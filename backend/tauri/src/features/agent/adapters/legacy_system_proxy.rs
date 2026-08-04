use std::{sync::Arc, time::Duration};

use sysproxy::Sysproxy;

use super::super::{
    model::{AgentCommandError, AgentResult},
    ports::{AgentMutationPort, SystemProxyConfiguration, SystemProxyPort},
};

const SYSTEM_PROXY_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const SYSTEM_PROXY_ACTION_TIMEOUT: Duration = Duration::from_secs(15);

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: host system proxy desired-state mutations still use the legacy feat service.
// Remove when: SystemProxyClient is injected through NyanpasuClient.
pub(crate) struct LegacySystemProxy {
    gate: Arc<tokio::sync::Semaphore>,
    mutation: Arc<dyn AgentMutationPort>,
}

impl LegacySystemProxy {
    pub(crate) fn new(mutation: Arc<dyn AgentMutationPort>) -> Self {
        Self {
            gate: Arc::new(tokio::sync::Semaphore::new(1)),
            mutation,
        }
    }
}

#[async_trait::async_trait]
impl SystemProxyPort for LegacySystemProxy {
    async fn probe(&self) -> Option<SystemProxyConfiguration> {
        let permit = self.gate.clone().try_acquire_owned().ok()?;
        tokio::time::timeout(
            SYSTEM_PROXY_PROBE_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                Sysproxy::get_system_proxy()
            }),
        )
        .await
        .ok()?
        .ok()?
        .ok()
        .map(SystemProxyConfiguration::from)
    }

    async fn read(&self) -> AgentResult<SystemProxyConfiguration> {
        let permit = tokio::time::timeout(
            SYSTEM_PROXY_ACTION_TIMEOUT,
            self.gate.clone().acquire_owned(),
        )
        .await
        .map_err(|_| AgentCommandError::ActionFailed)?
        .map_err(|_| AgentCommandError::ActionFailed)?;
        tokio::time::timeout(
            SYSTEM_PROXY_ACTION_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                Sysproxy::get_system_proxy()
            }),
        )
        .await
        .map_err(|_| AgentCommandError::ActionFailed)?
        .map_err(|_| AgentCommandError::ActionFailed)?
        .map(SystemProxyConfiguration::from)
        .map_err(|_| AgentCommandError::ActionFailed)
    }

    async fn write(&self, configuration: SystemProxyConfiguration) -> AgentResult<()> {
        let permit = tokio::time::timeout(
            SYSTEM_PROXY_ACTION_TIMEOUT,
            self.gate.clone().acquire_owned(),
        )
        .await
        .map_err(|_| AgentCommandError::PartialApply)?
        .map_err(|_| AgentCommandError::PartialApply)?;
        tokio::time::timeout(
            SYSTEM_PROXY_ACTION_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                Sysproxy::from(configuration).set_system_proxy()
            }),
        )
        .await
        .map_err(|_| AgentCommandError::PartialApply)?
        .map_err(|_| AgentCommandError::PartialApply)?
        .map_err(|_| AgentCommandError::PartialApply)
    }

    async fn apply_desired(&self, enabled: bool) -> AgentResult<()> {
        let deadline = tokio::time::Instant::now() + SYSTEM_PROXY_ACTION_TIMEOUT;
        let permit = tokio::time::timeout_at(deadline, self.gate.clone().acquire_owned())
            .await
            .map_err(|_| AgentCommandError::ActionFailed)?
            .map_err(|_| AgentCommandError::ActionFailed)?;
        let mutation = self.mutation.clone();
        let task = tokio::spawn(async move {
            let _permit = permit;
            mutation.set_system_proxy_enabled(enabled).await
        });
        tokio::time::timeout_at(deadline, task)
            .await
            .map_err(|_| AgentCommandError::PartialApply)?
            .map_err(|_| AgentCommandError::PartialApply)?
            .map_err(|_| AgentCommandError::ActionFailed)
    }
}

impl From<Sysproxy> for SystemProxyConfiguration {
    fn from(proxy: Sysproxy) -> Self {
        Self {
            enabled: proxy.enable,
            host: proxy.host,
            port: proxy.port,
            bypass: proxy.bypass,
        }
    }
}

impl From<SystemProxyConfiguration> for Sysproxy {
    fn from(configuration: SystemProxyConfiguration) -> Self {
        Self {
            enable: configuration.enabled,
            host: configuration.host,
            port: configuration.port,
            bypass: configuration.bypass,
        }
    }
}
