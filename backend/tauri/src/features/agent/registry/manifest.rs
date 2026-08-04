use std::time::Duration;

use super::{AgentToolError, request, unknown_tool};
use crate::features::agent::{
    AgentManifest, AgentNetworkProbeRequest, AgentToolManifest, AgentToolName, AgentToolRisk,
    model::{AGENT_MANIFEST_SCHEMA_VERSION, NETWORK_SNAPSHOT_SCHEMA_VERSION},
};

pub(super) const AGENT_TOOL_VERSION: u16 = 1;
pub(super) const AGENT_TOOL_INPUT_SCHEMA_VERSION: u16 = 1;
const NETWORK_PROBE_OUTPUT_SCHEMA_VERSION: u16 = 1;
const SNAPSHOT_TOOL_TIMEOUT_MS: u32 = 15_000;
pub(super) const NETWORK_PROBE_TOOL_TIMEOUT_MS: u32 = 12_000;

const SYSTEM_SNAPSHOT_OUTPUT_FIELDS: &[&str] = &[
    "schema_version",
    "captured_at",
    "app_version",
    "os_family",
    "health",
    "core_state",
    "run_type",
    "selected_core",
    "privacy",
];
const NETWORK_DIAGNOSE_OUTPUT_FIELDS: &[&str] = &[
    "schema_version",
    "revision",
    "captured_at",
    "app_version",
    "os_family",
    "health",
    "core",
    "service",
    "system_proxy",
    "tun",
    "profiles",
    "telemetry",
    "findings",
    "probe_failures",
    "privacy",
];
const NETWORK_PROBE_OUTPUT_FIELDS: &[&str] = &[
    "status",
    "expected_status",
    "matches_expected_status",
    "latency_ms",
];
const CORE_STATUS_OUTPUT_FIELDS: &[&str] = &[
    "state",
    "run_type",
    "selected_core",
    "state_changed_at",
    "runtime_config_present",
    "routing_mode",
    "observed_routing_mode",
    "applied_consistency",
];
const PROXY_STATUS_OUTPUT_FIELDS: &[&str] = &[
    "desired_enabled",
    "observed_enabled",
    "observed_host_scope",
    "observed_port",
    "expected_mixed_port",
    "matches_expected_endpoint",
];
const TUN_STATUS_OUTPUT_FIELDS: &[&str] = &[
    "desired_enabled",
    "generated_runtime_enabled",
    "observed_active",
    "applied_consistency",
];
const PROFILE_SUMMARY_OUTPUT_FIELDS: &[&str] = &[
    "total_count",
    "active_count",
    "remote_count",
    "local_count",
    "active_references_valid",
];
const SERVICE_STATUS_OUTPUT_FIELDS: &[&str] = &[
    "desired_enabled",
    "state",
    "ipc_connected",
    "runtime_compatible",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentToolKind {
    SystemSnapshot,
    NetworkDiagnose,
    NetworkProbe,
    CoreStatus,
    ProxyStatus,
    TunStatus,
    ProfileSummary,
    ServiceStatus,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AgentToolInput {
    Empty,
    NetworkProbe,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct AgentToolDefinition {
    pub(super) kind: AgentToolKind,
    pub(super) name: AgentToolName,
    pub(super) description: &'static str,
    pub(super) output_schema_version: u16,
    pub(super) timeout_ms: u32,
    pub(super) input: AgentToolInput,
    pub(super) output_fields: &'static [&'static str],
}

pub(super) const AGENT_TOOLS: [AgentToolDefinition; 8] = [
    AgentToolDefinition {
        kind: AgentToolKind::SystemSnapshot,
        name: AgentToolName::SystemSnapshot,
        description: "Collect a privacy-safe application and system summary",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: SYSTEM_SNAPSHOT_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::NetworkDiagnose,
        name: AgentToolName::NetworkDiagnose,
        description: "Collect a privacy-safe network diagnostic snapshot",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: NETWORK_DIAGNOSE_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::NetworkProbe,
        name: AgentToolName::NetworkProbe,
        description: "Probe one public HTTP or HTTPS endpoint without following redirects",
        output_schema_version: NETWORK_PROBE_OUTPUT_SCHEMA_VERSION,
        timeout_ms: NETWORK_PROBE_TOOL_TIMEOUT_MS,
        input: AgentToolInput::NetworkProbe,
        output_fields: NETWORK_PROBE_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::CoreStatus,
        name: AgentToolName::CoreStatus,
        description: "Collect the current core process, runtime, and routing summary",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: CORE_STATUS_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::ProxyStatus,
        name: AgentToolName::ProxyStatus,
        description: "Collect the desired and observed host system proxy summary",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: PROXY_STATUS_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::TunStatus,
        name: AgentToolName::TunStatus,
        description: "Collect the desired, generated, and observed TUN summary",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: TUN_STATUS_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::ProfileSummary,
        name: AgentToolName::ProfileSummary,
        description: "Collect privacy-safe profile counts and reference validity",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: PROFILE_SUMMARY_OUTPUT_FIELDS,
    },
    AgentToolDefinition {
        kind: AgentToolKind::ServiceStatus,
        name: AgentToolName::ServiceStatus,
        description: "Collect the desired and observed background service summary",
        output_schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
        timeout_ms: SNAPSHOT_TOOL_TIMEOUT_MS,
        input: AgentToolInput::Empty,
        output_fields: SERVICE_STATUS_OUTPUT_FIELDS,
    },
];

impl AgentToolDefinition {
    fn manifest(self) -> AgentToolManifest {
        AgentToolManifest {
            name: self.name,
            version: AGENT_TOOL_VERSION,
            description: self.description.into(),
            input_schema_version: AGENT_TOOL_INPUT_SCHEMA_VERSION,
            output_schema_version: self.output_schema_version,
            read_only: true,
            risk: AgentToolRisk::ReadOnly,
            requires_authentication: true,
            timeout_ms: self.timeout_ms,
        }
    }

    fn validate(self, body: &[u8]) -> Result<(), AgentToolError> {
        match self.input {
            AgentToolInput::Empty => request::parse_empty_request(body),
            AgentToolInput::NetworkProbe => {
                request::parse_body::<request::RequiredToolEnvelope<AgentNetworkProbeRequest>>(body)
                    .map(|_| ())
            }
        }
    }
}

pub(super) fn tool_definition(name: &str) -> Result<AgentToolDefinition, AgentToolError> {
    AGENT_TOOLS
        .iter()
        .copied()
        .find(|tool| tool.name.as_str() == name)
        .ok_or_else(unknown_tool)
}

pub(crate) fn agent_manifest() -> AgentManifest {
    AgentManifest {
        schema_version: AGENT_MANIFEST_SCHEMA_VERSION,
        tools: AGENT_TOOLS
            .into_iter()
            .map(AgentToolDefinition::manifest)
            .collect(),
    }
}

pub(crate) fn tool_timeout(name: &str) -> Option<Duration> {
    AGENT_TOOLS
        .iter()
        .find(|tool| tool.name.as_str() == name)
        .map(|tool| Duration::from_millis(u64::from(tool.timeout_ms)))
}

pub(crate) fn validate_tool_request(name: &str, body: &[u8]) -> Result<(), AgentToolError> {
    tool_definition(name)?.validate(body)
}
