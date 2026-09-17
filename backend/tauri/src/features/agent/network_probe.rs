use super::{AgentNetworkProbeRequest, AgentNetworkProbeResult, AgentToolError};

pub(crate) async fn execute_network_probe(
    request: AgentNetworkProbeRequest,
) -> Result<AgentNetworkProbeResult, AgentToolError> {
    let result = crate::network_probe::execute(crate::network_probe::NetworkProbeRequest {
        url: request.url,
        expected_status: request.expected_status,
        timeout_ms: request.timeout_ms,
    })
    .await
    .map_err(map_network_probe_error)?;

    Ok(AgentNetworkProbeResult {
        status: result.status,
        expected_status: result.expected_status,
        matches_expected_status: result.matches_expected_status,
        latency_ms: result.latency_ms,
    })
}

fn map_network_probe_error(error: crate::network_probe::NetworkProbeError) -> AgentToolError {
    match error {
        crate::network_probe::NetworkProbeError::InvalidRequest => AgentToolError::InvalidRequest,
        crate::network_probe::NetworkProbeError::InvalidTarget => AgentToolError::InvalidTarget,
        crate::network_probe::NetworkProbeError::TargetBlocked => AgentToolError::TargetBlocked,
        crate::network_probe::NetworkProbeError::ResolutionFailed => {
            AgentToolError::ResolutionFailed
        }
        crate::network_probe::NetworkProbeError::TimedOut => AgentToolError::TimedOut,
        crate::network_probe::NetworkProbeError::ExecutionFailed => AgentToolError::ExecutionFailed,
    }
}
