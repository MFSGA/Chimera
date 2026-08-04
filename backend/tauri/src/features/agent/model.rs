use serde::{Deserialize, Deserializer, Serialize};
use specta::Type;

pub(crate) use super::error::{AgentCommandError, AgentResult};

pub(crate) const AGENT_MANIFEST_SCHEMA_VERSION: u16 = 1;
pub(crate) const NETWORK_SNAPSHOT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolRisk {
    ReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
pub enum AgentToolName {
    #[serde(rename = "system.snapshot")]
    SystemSnapshot,
    #[serde(rename = "network.diagnose")]
    NetworkDiagnose,
    #[serde(rename = "network.probe")]
    NetworkProbe,
    #[serde(rename = "core.status")]
    CoreStatus,
    #[serde(rename = "proxy.status")]
    ProxyStatus,
    #[serde(rename = "tun.status")]
    TunStatus,
    #[serde(rename = "profile.summary")]
    ProfileSummary,
    #[serde(rename = "service.status")]
    ServiceStatus,
}

impl AgentToolName {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SystemSnapshot => "system.snapshot",
            Self::NetworkDiagnose => "network.diagnose",
            Self::NetworkProbe => "network.probe",
            Self::CoreStatus => "core.status",
            Self::ProxyStatus => "proxy.status",
            Self::TunStatus => "tun.status",
            Self::ProfileSummary => "profile.summary",
            Self::ServiceStatus => "service.status",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentToolManifest {
    pub name: AgentToolName,
    pub version: u16,
    pub description: String,
    pub input_schema_version: u16,
    pub output_schema_version: u16,
    pub read_only: bool,
    pub risk: AgentToolRisk,
    pub requires_authentication: bool,
    pub timeout_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentManifest {
    pub schema_version: u16,
    pub tools: Vec<AgentToolManifest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(deny_unknown_fields)]
pub struct AgentNetworkProbeRequest {
    pub url: String,
    pub expected_status: Option<u16>,
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentNetworkProbeResult {
    pub status: u16,
    pub expected_status: Option<u16>,
    pub matches_expected_status: Option<bool>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentSystemSnapshot {
    pub schema_version: u16,
    pub captured_at: i64,
    pub app_version: String,
    pub os_family: AgentOsFamily,
    pub health: AgentHealth,
    pub core_state: AgentCoreState,
    pub run_type: AgentRunType,
    pub selected_core: AgentSelectedCore,
    pub privacy: AgentPrivacyBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentOsFamily {
    Windows,
    Macos,
    Ios,
    Linux,
    Android,
    Freebsd,
    Dragonfly,
    Openbsd,
    Netbsd,
    Unknown,
}

impl AgentOsFamily {
    pub(crate) fn current() -> Self {
        Self::from_name(std::env::consts::OS)
    }

    fn from_name(name: &str) -> Self {
        match name {
            "windows" => Self::Windows,
            "macos" => Self::Macos,
            "ios" => Self::Ios,
            "linux" => Self::Linux,
            "android" => Self::Android,
            "freebsd" => Self::Freebsd,
            "dragonfly" => Self::Dragonfly,
            "openbsd" => Self::Openbsd,
            "netbsd" => Self::Netbsd,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentHealth {
    Healthy,
    Warning,
    Critical,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
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
#[serde(rename_all = "kebab-case")]
pub enum AgentSelectedCore {
    Clash,
    ClashRs,
    Mihomo,
    ChimeraClient,
    MihomoAlpha,
    ClashRsAlpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
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
    pub selected_core: AgentSelectedCore,
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
    pub upload_total: Option<u64>,
    pub download_total: Option<u64>,
    pub recent_error_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentProbeCode {
    CoreStatusTimeout,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
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

impl AgentPrivacyBoundary {
    pub(crate) const fn privacy_safe() -> Self {
        Self {
            contains_raw_logs: false,
            contains_profile_names: false,
            contains_profile_urls: false,
            contains_connection_targets: false,
            contains_controller_secret: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentNetworkSnapshot {
    pub schema_version: u16,
    pub revision: String,
    pub captured_at: i64,
    pub app_version: String,
    pub os_family: AgentOsFamily,
    pub health: AgentHealth,
    pub core: AgentCoreSnapshot,
    pub service: AgentServiceSnapshot,
    pub system_proxy: AgentSystemProxySnapshot,
    pub tun: AgentTunSnapshot,
    pub profiles: AgentProfileSnapshot,
    pub telemetry: AgentTelemetrySnapshot,
    pub findings: Vec<AgentFinding>,
    pub probe_failures: Vec<AgentProbeFailure>,
    pub recommendations: Vec<AgentRecommendation>,
    pub privacy: AgentPrivacyBoundary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionKind {
    SetRoutingMode,
    SetTunEnabled,
    SetSystemProxyEnabled,
    SetServiceMode,
    StartCore,
    RestartCore,
    ReconnectTelemetry,
    StartService,
    StopService,
    RestartService,
    RepairSystemProxyEndpoint,
    DisableStaleSystemProxy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AgentActionRequest {
    SetRoutingMode { mode: AgentRoutingMode },
    SetTunEnabled { enabled: bool },
    SetSystemProxyEnabled { enabled: bool },
    SetServiceMode { enabled: bool },
    StartCore,
    RestartCore,
    ReconnectTelemetry,
    StartService,
    StopService,
    RestartService,
    RepairSystemProxyEndpoint,
    DisableStaleSystemProxy,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum StrictAgentActionRequest {
    SetRoutingMode { mode: AgentRoutingMode },
    SetTunEnabled { enabled: bool },
    SetSystemProxyEnabled { enabled: bool },
    SetServiceMode { enabled: bool },
    StartCore {},
    RestartCore {},
    ReconnectTelemetry {},
    StartService {},
    StopService {},
    RestartService {},
    RepairSystemProxyEndpoint {},
    DisableStaleSystemProxy {},
}

impl<'de> Deserialize<'de> for AgentActionRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match StrictAgentActionRequest::deserialize(deserializer)? {
            StrictAgentActionRequest::SetRoutingMode { mode } => Self::SetRoutingMode { mode },
            StrictAgentActionRequest::SetTunEnabled { enabled } => Self::SetTunEnabled { enabled },
            StrictAgentActionRequest::SetSystemProxyEnabled { enabled } => {
                Self::SetSystemProxyEnabled { enabled }
            }
            StrictAgentActionRequest::SetServiceMode { enabled } => {
                Self::SetServiceMode { enabled }
            }
            StrictAgentActionRequest::StartCore {} => Self::StartCore,
            StrictAgentActionRequest::RestartCore {} => Self::RestartCore,
            StrictAgentActionRequest::ReconnectTelemetry {} => Self::ReconnectTelemetry,
            StrictAgentActionRequest::StartService {} => Self::StartService,
            StrictAgentActionRequest::StopService {} => Self::StopService,
            StrictAgentActionRequest::RestartService {} => Self::RestartService,
            StrictAgentActionRequest::RepairSystemProxyEndpoint {} => {
                Self::RepairSystemProxyEndpoint
            }
            StrictAgentActionRequest::DisableStaleSystemProxy {} => Self::DisableStaleSystemProxy,
        })
    }
}

impl AgentActionRequest {
    pub(crate) fn kind(&self) -> AgentActionKind {
        match self {
            Self::SetRoutingMode { .. } => AgentActionKind::SetRoutingMode,
            Self::SetTunEnabled { .. } => AgentActionKind::SetTunEnabled,
            Self::SetSystemProxyEnabled { .. } => AgentActionKind::SetSystemProxyEnabled,
            Self::SetServiceMode { .. } => AgentActionKind::SetServiceMode,
            Self::StartCore => AgentActionKind::StartCore,
            Self::RestartCore => AgentActionKind::RestartCore,
            Self::ReconnectTelemetry => AgentActionKind::ReconnectTelemetry,
            Self::StartService => AgentActionKind::StartService,
            Self::StopService => AgentActionKind::StopService,
            Self::RestartService => AgentActionKind::RestartService,
            Self::RepairSystemProxyEndpoint => AgentActionKind::RepairSystemProxyEndpoint,
            Self::DisableStaleSystemProxy => AgentActionKind::DisableStaleSystemProxy,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Type)]
#[serde(deny_unknown_fields)]
pub struct AgentIntentRequest {
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentServiceOperation {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum AgentIntent {
    Diagnose,
    SetTunEnabled { enabled: bool },
    SetSystemProxyEnabled { enabled: bool },
    SetServiceMode { enabled: bool },
    SetRoutingMode { mode: AgentRoutingMode },
    StartCore,
    RestartCore,
    ReconnectTelemetry,
    ControlService { operation: AgentServiceOperation },
    RepairSystemProxyEndpoint,
    DisableStaleSystemProxy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentClarificationCode {
    EnableTun,
    UseGlobalRouting,
    DiagnoseNetwork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct AgentClarificationChoice {
    pub code: AgentClarificationCode,
    pub intent: AgentIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentUnsupportedIntentReason {
    EmptyInput,
    InputTooLong,
    NoMatchingIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AgentIntentResolution {
    Resolved {
        intent: AgentIntent,
    },
    NeedsClarification {
        choices: Vec<AgentClarificationChoice>,
    },
    Unsupported {
        reason: AgentUnsupportedIntentReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentActionRisk {
    TrafficChange,
    HostNetworkChange,
    ServiceControl,
    TelemetryRecovery,
}

impl AgentActionRisk {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::TrafficChange => "traffic_change",
            Self::HostNetworkChange => "host_network_change",
            Self::ServiceControl => "service_control",
            Self::TelemetryRecovery => "telemetry_recovery",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentRecommendationUnavailableReason {
    CurrentStateNotSupported,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentRecommendation {
    pub action: AgentActionRequest,
    pub available: bool,
    pub unavailable_reason: Option<AgentRecommendationUnavailableReason>,
    pub risk: Option<AgentActionRisk>,
    pub impacts: Vec<AgentImpact>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentImpact {
    ExistingConnectionsMayChange,
    CoreMayRestart,
    HostDnsMayChange,
    AdminPermissionMayBeRequired,
    TrafficMayBypassProxy,
    AllTrafficUsesProxy,
    RestoreRuleRouting,
    HostSystemProxyEnabled,
    HostSystemProxyDisabled,
    HostSystemProxyEndpointChanged,
    ServiceAvailabilityMayChange,
    TelemetryMayBeUnavailable,
}

impl AgentImpact {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ExistingConnectionsMayChange => "existing_connections_may_change",
            Self::CoreMayRestart => "core_may_restart",
            Self::HostDnsMayChange => "host_dns_may_change",
            Self::AdminPermissionMayBeRequired => "admin_permission_may_be_required",
            Self::TrafficMayBypassProxy => "traffic_may_bypass_proxy",
            Self::AllTrafficUsesProxy => "all_traffic_uses_proxy",
            Self::RestoreRuleRouting => "restore_rule_routing",
            Self::HostSystemProxyEnabled => "host_system_proxy_enabled",
            Self::HostSystemProxyDisabled => "host_system_proxy_disabled",
            Self::HostSystemProxyEndpointChanged => "host_system_proxy_endpoint_changed",
            Self::ServiceAvailabilityMayChange => "service_availability_may_change",
            Self::TelemetryMayBeUnavailable => "telemetry_may_be_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentStateField {
    RoutingMode,
    Tun,
    CoreProcess,
    TelemetryConnector,
    Service,
    ServiceMode,
    SystemProxyEndpoint,
    SystemProxy,
}

impl AgentStateField {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::RoutingMode => "routing_mode",
            Self::Tun => "tun",
            Self::CoreProcess => "core_process",
            Self::TelemetryConnector => "telemetry_connector",
            Self::Service => "service",
            Self::ServiceMode => "service_mode",
            Self::SystemProxyEndpoint => "system_proxy_endpoint",
            Self::SystemProxy => "system_proxy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentStateValue {
    Rule,
    Global,
    Direct,
    Running,
    Stopped,
    Restarted,
    Connected,
    Disconnected,
    Unexpected,
    ExpectedLoopbackEndpoint,
    Enabled,
    Disabled,
}

impl AgentStateValue {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Restarted => "restarted",
            Self::Connected => "connected",
            Self::Disconnected => "disconnected",
            Self::Unexpected => "unexpected",
            Self::ExpectedLoopbackEndpoint => "expected_loopback_endpoint",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AgentStateChange {
    pub field: AgentStateField,
    pub before: AgentStateValue,
    pub after: AgentStateValue,
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

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
