use serde_json::Value;

use super::{
    AgentToolError,
    manifest::{AgentToolKind, tool_definition},
    output::serialize_tool_result,
    request::{RequiredToolEnvelope, parse_body, parse_empty_request},
};
use crate::features::agent::{
    AgentNetworkProbeRequest,
    model::AgentSystemSnapshot,
    ports::{AgentRuntimePort, NetworkProbePort},
};

pub(crate) async fn execute_tool(
    runtime: &dyn AgentRuntimePort,
    network_probe: &dyn NetworkProbePort,
    name: &str,
    body: &[u8],
) -> Result<Value, AgentToolError> {
    let definition = tool_definition(name)?;
    match definition.kind {
        AgentToolKind::SystemSnapshot => {
            parse_empty_request(body)?;
            let snapshot = runtime.snapshot().await;
            serialize_tool_result(
                AgentSystemSnapshot {
                    schema_version: snapshot.schema_version,
                    captured_at: snapshot.captured_at,
                    app_version: snapshot.app_version,
                    os_family: snapshot.os_family,
                    health: snapshot.health,
                    core_state: snapshot.core.state,
                    run_type: snapshot.core.run_type,
                    selected_core: snapshot.core.selected_core,
                    privacy: snapshot.privacy,
                },
                "failed to serialize system snapshot",
                definition.output_fields,
            )
        }
        AgentToolKind::NetworkDiagnose => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await,
                "failed to serialize diagnostic result",
                definition.output_fields,
            )
        }
        AgentToolKind::NetworkProbe => {
            let request: RequiredToolEnvelope<AgentNetworkProbeRequest> = parse_body(body)?;
            serialize_tool_result(
                network_probe.execute(request.arguments).await?,
                "failed to serialize probe result",
                definition.output_fields,
            )
        }
        AgentToolKind::CoreStatus => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await.core,
                "failed to serialize core status",
                definition.output_fields,
            )
        }
        AgentToolKind::ProxyStatus => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await.system_proxy,
                "failed to serialize proxy status",
                definition.output_fields,
            )
        }
        AgentToolKind::TunStatus => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await.tun,
                "failed to serialize TUN status",
                definition.output_fields,
            )
        }
        AgentToolKind::ProfileSummary => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await.profiles,
                "failed to serialize profile summary",
                definition.output_fields,
            )
        }
        AgentToolKind::ServiceStatus => {
            parse_empty_request(body)?;
            serialize_tool_result(
                runtime.snapshot().await.service,
                "failed to serialize service status",
                definition.output_fields,
            )
        }
    }
}
