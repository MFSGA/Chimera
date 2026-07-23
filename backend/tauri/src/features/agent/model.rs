use serde::{Deserialize, Serialize};
use specta::Type;

pub(crate) const NETWORK_SNAPSHOT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentHealth {
    Healthy,
    Warning,
    Critical,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentCoreState {
    Running,
    Stopped,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunType {
    Normal,
    Service,
    Elevated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentServiceState {
    NotInstalled,
    Stopped,
    Running,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentHostScope {
    Loopback,
    NonLoopback,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentConnectorState {
    Disconnected,
    Connecting,
    Connected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentAppliedState {
    Consistent,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentRoutingMode {
    Rule,
    Global,
    Direct,
}

impl AgentRoutingMode {
    pub(crate) fn as_core_value(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "rule" => Some(Self::Rule),
            "global" => Some(Self::Global),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentCoreSnapshot {
    pub state: AgentCoreState,
    pub run_type: AgentRunType,
    pub selected_core: String,
    pub state_changed_at: i64,
    pub runtime_config_present: bool,
    pub routing_mode: Option<AgentRoutingMode>,
    pub observed_routing_mode: Option<AgentRoutingMode>,
    pub applied_consistency: AgentAppliedState,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentServiceSnapshot {
    pub desired_enabled: bool,
    pub state: AgentServiceState,
    pub ipc_connected: bool,
    pub runtime_compatible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentSystemProxySnapshot {
    pub desired_enabled: bool,
    pub observed_enabled: Option<bool>,
    pub observed_host_scope: AgentHostScope,
    pub observed_port: Option<u16>,
    pub expected_mixed_port: u16,
    pub matches_expected_endpoint: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentTunSnapshot {
    pub desired_enabled: bool,
    pub generated_runtime_enabled: Option<bool>,
    pub observed_active: AgentAppliedState,
    pub applied_consistency: AgentAppliedState,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentProfileSnapshot {
    pub total_count: u32,
    pub active_count: u32,
    pub remote_count: u32,
    pub local_count: u32,
    pub active_references_valid: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentTelemetrySnapshot {
    pub state: AgentConnectorState,
    pub active_connection_count: Option<u32>,
    pub upload_speed: Option<u64>,
    pub download_speed: Option<u64>,
    pub upload_total: Option<String>,
    pub download_total: Option<String>,
    pub recent_error_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentProbeCode {
    CoreConfigUnavailable,
    SystemProxyUnavailable,
    ServiceStatusUnavailable,
    ServiceStatusTimeout,
    TelemetryUnavailable,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentProbeFailure {
    pub code: AgentProbeCode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentFindingSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentFindingCode {
    WeakControllerSecret,
    SystemProxyWithoutRunningCore,
    SystemProxyEndpointMismatch,
    RuntimeConfigMissing,
    ActiveProfileMissing,
    ServiceModeInconsistent,
    ClashConnectorDisconnected,
    TunRuntimeMismatch,
    RecentCoreErrors,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentFinding {
    pub code: AgentFindingCode,
    pub severity: AgentFindingSeverity,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentPrivacyBoundary {
    pub contains_raw_logs: bool,
    pub contains_profile_names: bool,
    pub contains_profile_urls: bool,
    pub contains_connection_targets: bool,
    pub contains_controller_secret: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentNetworkSnapshot {
    pub schema_version: u16,
    pub revision: String,
    pub captured_at: i64,
    pub app_version: String,
    pub os_family: String,
    pub health: AgentHealth,
    pub core: AgentCoreSnapshot,
    pub service: AgentServiceSnapshot,
    pub system_proxy: AgentSystemProxySnapshot,
    pub tun: AgentTunSnapshot,
    pub profiles: AgentProfileSnapshot,
    pub telemetry: AgentTelemetrySnapshot,
    pub findings: Vec<AgentFinding>,
    pub probe_failures: Vec<AgentProbeFailure>,
    pub privacy: AgentPrivacyBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionKind {
    SetRoutingMode,
    DisableStaleSystemProxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentActionRequest {
    SetRoutingMode { mode: AgentRoutingMode },
    DisableStaleSystemProxy,
}

impl AgentActionRequest {
    pub(crate) fn kind(&self) -> AgentActionKind {
        match self {
            Self::SetRoutingMode { .. } => AgentActionKind::SetRoutingMode,
            Self::DisableStaleSystemProxy => AgentActionKind::DisableStaleSystemProxy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionRisk {
    TrafficChange,
    HostNetworkChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentImpact {
    ExistingConnectionsMayChange,
    TrafficMayBypassProxy,
    AllTrafficUsesProxy,
    RestoreRuleRouting,
    HostSystemProxyDisabled,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentStateChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentProposal {
    pub id: String,
    pub digest: String,
    pub action: AgentActionRequest,
    pub risk: AgentActionRisk,
    pub impacts: Vec<AgentImpact>,
    pub changes: Vec<AgentStateChange>,
    pub snapshot_revision: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub requires_confirmation: bool,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentActionResult {
    pub proposal_id: String,
    pub action: AgentActionKind,
    pub verified: bool,
    pub snapshot: AgentNetworkSnapshot,
}

#[derive(Debug, thiserror::Error)]
pub enum AgentCommandError {
    #[error("agent_action_not_available")]
    ActionNotAvailable,
    #[error("agent_proposal_not_found")]
    ProposalNotFound,
    #[error("agent_proposal_expired")]
    ProposalExpired,
    #[error("agent_proposal_digest_mismatch")]
    ProposalDigestMismatch,
    #[error("agent_network_state_changed")]
    NetworkStateChanged,
    #[error("agent_proposal_rate_limited")]
    ProposalRateLimited,
    #[error("agent_proposal_limit_reached")]
    ProposalLimitReached,
    #[error("agent_confirmation_declined")]
    ConfirmationDeclined,
    #[error("agent_action_failed")]
    ActionFailed,
    #[error("agent_action_partially_applied")]
    PartialApply,
    #[error("agent_action_verification_failed")]
    VerificationFailed,
}

impl Serialize for AgentCommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl Type for AgentCommandError {
    fn definition(_: &mut specta::Types) -> specta::datatype::DataType {
        specta::datatype::DataType::Primitive(specta::datatype::Primitive::str)
    }
}

pub(crate) type AgentResult<T> = Result<T, AgentCommandError>;
