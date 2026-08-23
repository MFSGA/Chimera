use std::{future::Future, pin::Pin};

use axum::body::Bytes;
use serde_json::Value;

use super::registry::AgentToolError;
use super::{
    bridge::{AgentBridgeStartResult, AgentBridgeStatus},
    history::AgentHistoryDocument,
    model::{
        AgentActionRequest, AgentCommandError, AgentCoreState, AgentHostConnectivitySnapshot,
        AgentNetworkProbeRequest, AgentNetworkProbeResult, AgentNetworkSnapshot,
        AgentProcessPrivilegeStatus, AgentProfileSnapshot, AgentProposal, AgentResult,
        AgentRoutingMode, AgentRunType, AgentSelectedCore, AgentServiceState,
        AgentTelemetrySnapshot,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct AgentConfigurationSnapshot {
    pub(crate) expected_mixed_port: u16,
    pub(crate) selected_core: AgentSelectedCore,
    pub(crate) runtime_config_present: bool,
    pub(crate) routing_mode: Option<AgentRoutingMode>,
    pub(crate) generated_tun_enabled: Option<bool>,
    pub(crate) secret_is_weak: bool,
    pub(crate) desired_service_mode: bool,
    pub(crate) desired_system_proxy: bool,
    pub(crate) desired_tun: bool,
    pub(crate) profiles: AgentProfileSnapshot,
}

pub(crate) trait AgentConfigurationPort: Send + Sync + 'static {
    fn snapshot(&self) -> AgentConfigurationSnapshot;
}

#[async_trait::async_trait]
pub(crate) trait HostConnectivityPort: Send + Sync + 'static {
    async fn snapshot(&self) -> AgentHostConnectivitySnapshot;
}

#[async_trait::async_trait]
pub(crate) trait PlatformReadinessPort: Send + Sync + 'static {
    async fn process_privilege(&self) -> AgentProcessPrivilegeStatus;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CoreRuntimeObservation {
    pub(crate) routing_mode: AgentRoutingMode,
    pub(crate) tun_enabled: Option<bool>,
}

#[async_trait::async_trait]
pub(crate) trait CoreRoutingProbePort: Send + Sync + 'static {
    async fn observed_configuration(&self) -> Result<CoreRuntimeObservation, ()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CoreLifecycleStatus {
    pub(crate) state: AgentCoreState,
    pub(crate) run_type: AgentRunType,
    pub(crate) state_changed_at: i64,
}

#[async_trait::async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync + 'static {
    async fn status(&self) -> CoreLifecycleStatus;

    async fn ensure_running(&self) -> anyhow::Result<()>;

    async fn restart(&self) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ServiceLifecycleStatus {
    pub(crate) state: AgentServiceState,
    pub(crate) runtime_compatible: Option<bool>,
}

#[async_trait::async_trait]
pub(crate) trait ServiceControlPort: Send + Sync + 'static {
    async fn status(&self) -> anyhow::Result<ServiceLifecycleStatus>;

    fn ipc_connected(&self) -> bool;

    async fn start(&self) -> anyhow::Result<()>;

    async fn stop(&self) -> anyhow::Result<()>;

    async fn restart(&self) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub(crate) trait AgentTelemetryPort: Send + Sync + 'static {
    fn snapshot(&self) -> Option<AgentTelemetrySnapshot>;

    async fn reconnect(&self) -> anyhow::Result<()>;
}

#[async_trait::async_trait]
pub(crate) trait AgentMutationPort: Send + Sync + 'static {
    async fn set_tun_enabled(&self, enabled: bool) -> anyhow::Result<()>;

    async fn set_system_proxy_enabled(&self, enabled: bool) -> anyhow::Result<()>;

    async fn persist_system_proxy_desired(&self, enabled: bool) -> anyhow::Result<()>;

    async fn set_service_mode(&self, enabled: bool) -> anyhow::Result<()>;

    async fn restore_service_mode(&self, enabled: bool) -> anyhow::Result<()>;

    async fn set_routing_mode(&self, mode: AgentRoutingMode) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub(crate) struct SystemProxyConfiguration {
    pub(crate) enabled: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) bypass: String,
}

#[async_trait::async_trait]
pub(crate) trait SystemProxyPort: Send + Sync + 'static {
    async fn probe(&self) -> Option<SystemProxyConfiguration>;

    async fn read(&self) -> AgentResult<SystemProxyConfiguration>;

    async fn write(&self, configuration: SystemProxyConfiguration) -> AgentResult<()>;

    async fn apply_desired(&self, enabled: bool) -> AgentResult<()>;
}

#[async_trait::async_trait]
pub(crate) trait AgentRuntimePort: Send + Sync + 'static {
    async fn snapshot(&self) -> AgentNetworkSnapshot;

    async fn set_tun_enabled(&self, before: bool, target: bool) -> AgentResult<()>;

    async fn set_system_proxy_enabled(&self, before: bool, target: bool) -> AgentResult<()>;

    async fn set_service_mode(&self, before: bool, target: bool) -> AgentResult<()>;

    async fn ensure_core_running(&self) -> AgentResult<()>;

    async fn restart_core(&self) -> AgentResult<()>;

    async fn reconnect_telemetry(&self) -> AgentResult<()>;

    async fn control_service(&self, action: &AgentActionRequest) -> AgentResult<()>;

    async fn set_routing_mode(
        &self,
        before: AgentRoutingMode,
        target: AgentRoutingMode,
    ) -> AgentResult<()>;

    async fn repair_system_proxy_endpoint(
        &self,
        snapshot: &AgentNetworkSnapshot,
        expected_port: u16,
        desired_before: bool,
    ) -> AgentResult<()>;

    async fn disable_stale_system_proxy(
        &self,
        snapshot: &AgentNetworkSnapshot,
        expected_port: u16,
        desired_before: bool,
    ) -> AgentResult<()>;
}

#[async_trait::async_trait]
pub(crate) trait AgentConfirmationPort: Send + Sync + 'static {
    async fn confirm(&self, owner_label: &str, proposal: &AgentProposal) -> AgentResult<bool>;

    async fn confirm_history_clear(&self, owner_label: &str) -> AgentResult<bool>;
}

#[async_trait::async_trait]
pub(crate) trait AgentBridgePort: Send + 'static {
    async fn start(&mut self) -> Result<AgentBridgeStartResult, AgentCommandError>;

    async fn status(&mut self) -> AgentBridgeStatus;

    async fn stop(&mut self) -> AgentBridgeStatus;
}

#[async_trait::async_trait]
pub(crate) trait AgentBridgeHealthPort: Send + Sync + 'static {
    async fn is_healthy(&self, health_url: &str, schema_version: u16) -> bool;
}

#[async_trait::async_trait]
pub(crate) trait NetworkProbePort: Send + Sync + 'static {
    async fn execute(
        &self,
        request: AgentNetworkProbeRequest,
    ) -> Result<AgentNetworkProbeResult, AgentToolError>;
}

pub(crate) type AgentToolExecutionFuture =
    Pin<Box<dyn Future<Output = Result<Value, AgentToolError>> + Send + 'static>>;

pub(crate) trait AgentToolExecutorPort: Send + Sync + 'static {
    fn execute(&self, tool_name: String, body: Bytes) -> AgentToolExecutionFuture;
}

#[async_trait::async_trait]
pub(crate) trait AgentHistoryPersistencePort: Send + Sync + 'static {
    async fn load(&self) -> anyhow::Result<AgentHistoryDocument>;

    async fn save(&self, document: &AgentHistoryDocument) -> anyhow::Result<()>;
}
