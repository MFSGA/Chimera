mod execution;
mod manifest;
mod output;
pub(crate) mod probe;
mod request;

use serde::Serialize;

pub(crate) use execution::execute_tool;
pub(crate) use manifest::{agent_manifest, tool_timeout, validate_tool_request};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentToolErrorCode {
    UnknownTool,
    InvalidRequest,
    InvalidTarget,
    TargetBlocked,
    ResolutionFailed,
    TimedOut,
    ExecutionFailed,
}

impl AgentToolErrorCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidTarget => "invalid_target",
            Self::TargetBlocked => "target_blocked",
            Self::ResolutionFailed => "resolution_failed",
            Self::TimedOut => "timed_out",
            Self::ExecutionFailed => "execution_failed",
        }
    }
}

#[derive(Debug)]
pub(crate) struct AgentToolError {
    pub code: AgentToolErrorCode,
    pub message: &'static str,
}

impl AgentToolError {
    pub(super) fn new(code: AgentToolErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

fn unknown_tool() -> AgentToolError {
    AgentToolError::new(AgentToolErrorCode::UnknownTool, "unknown agent tool")
}

#[cfg(test)]
use manifest::{
    AGENT_TOOL_INPUT_SCHEMA_VERSION, AGENT_TOOL_VERSION, AGENT_TOOLS, AgentToolInput,
    tool_definition,
};
#[cfg(test)]
use output::serialize_tool_result;
#[cfg(test)]
use probe::{
    MAX_NETWORK_PROBE_URL_BYTES, MAX_RESOLVED_ADDRESSES, NETWORK_PROBE_REQUEST_TIMEOUT_MS,
    collect_safe_addresses, is_blocked_hostname, is_blocked_ip, validate_probe_request,
};
#[cfg(test)]
use request::parse_empty_request;

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
