use std::time::Duration;

use tauri::AppHandle;

use super::{
    collect_network_snapshot,
    model::{
        AgentDiagnosticSummary, AgentManifest, AgentNetworkSnapshot, AgentToolError,
        AgentToolManifest, AgentToolName, AgentToolResult, AgentToolRisk,
    },
};

const AGENT_MANIFEST_SCHEMA_VERSION: u16 = 1;
const AGENT_TOOL_VERSION: u16 = 1;
const AGENT_TOOL_OUTPUT_SCHEMA_VERSION: u16 = 1;
const AGENT_TOOL_TIMEOUT_MS: u32 = 15_000;

#[derive(Debug, Clone, Copy)]
struct AgentToolDefinition {
    name: AgentToolName,
    description: &'static str,
}

const AGENT_TOOLS: [AgentToolDefinition; 7] = [
    AgentToolDefinition {
        name: AgentToolName::SystemSnapshot,
        description: "Collect the complete privacy-safe Chimera network snapshot",
    },
    AgentToolDefinition {
        name: AgentToolName::NetworkDiagnose,
        description: "Collect health, findings, and probe failures from a fresh network snapshot",
    },
    AgentToolDefinition {
        name: AgentToolName::CoreStatus,
        description: "Collect the current core process, runtime, and routing summary",
    },
    AgentToolDefinition {
        name: AgentToolName::ProxyStatus,
        description: "Collect the desired and observed host system proxy summary",
    },
    AgentToolDefinition {
        name: AgentToolName::TunStatus,
        description: "Collect the desired and generated TUN state summary",
    },
    AgentToolDefinition {
        name: AgentToolName::ProfileSummary,
        description: "Collect profile counts and active-reference validity without names or URLs",
    },
    AgentToolDefinition {
        name: AgentToolName::ServiceStatus,
        description: "Collect the desired and observed service-mode summary",
    },
];

pub(crate) fn agent_manifest() -> AgentManifest {
    AgentManifest {
        schema_version: AGENT_MANIFEST_SCHEMA_VERSION,
        tools: AGENT_TOOLS
            .iter()
            .map(|definition| AgentToolManifest {
                name: definition.name,
                version: AGENT_TOOL_VERSION,
                description: definition.description.to_owned(),
                risk: AgentToolRisk::ReadOnly,
                read_only: true,
                timeout_ms: AGENT_TOOL_TIMEOUT_MS,
                output_schema_version: AGENT_TOOL_OUTPUT_SCHEMA_VERSION,
            })
            .collect(),
    }
}

pub(crate) async fn execute_readonly_tool(
    app: &AppHandle,
    tool: AgentToolName,
) -> Result<AgentToolResult, AgentToolError> {
    let snapshot = tokio::time::timeout(
        Duration::from_millis(u64::from(AGENT_TOOL_TIMEOUT_MS)),
        collect_network_snapshot(app),
    )
    .await
    .map_err(|_| AgentToolError::TimedOut)?;

    Ok(project_tool(snapshot, tool))
}

fn project_tool(snapshot: AgentNetworkSnapshot, tool: AgentToolName) -> AgentToolResult {
    match tool {
        AgentToolName::SystemSnapshot => AgentToolResult::SystemSnapshot {
            output: Box::new(snapshot),
        },
        AgentToolName::NetworkDiagnose => {
            let AgentNetworkSnapshot {
                revision,
                captured_at,
                health,
                findings,
                probe_failures,
                privacy,
                ..
            } = snapshot;
            AgentToolResult::NetworkDiagnose {
                output: AgentDiagnosticSummary {
                    revision,
                    captured_at,
                    health,
                    findings,
                    probe_failures,
                    privacy,
                },
            }
        }
        AgentToolName::CoreStatus => AgentToolResult::CoreStatus {
            output: snapshot.core,
        },
        AgentToolName::ProxyStatus => AgentToolResult::ProxyStatus {
            output: snapshot.system_proxy,
        },
        AgentToolName::TunStatus => AgentToolResult::TunStatus {
            output: snapshot.tun,
        },
        AgentToolName::ProfileSummary => AgentToolResult::ProfileSummary {
            output: snapshot.profiles,
        },
        AgentToolName::ServiceStatus => AgentToolResult::ServiceStatus {
            output: snapshot.service,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{
        AGENT_MANIFEST_SCHEMA_VERSION, AGENT_TOOL_OUTPUT_SCHEMA_VERSION, agent_manifest,
        project_tool,
    };
    use crate::features::agent::model::{
        AgentAppliedState, AgentConnectorState, AgentCoreSnapshot, AgentCoreState, AgentFinding,
        AgentFindingCode, AgentFindingSeverity, AgentHealth, AgentHostScope, AgentNetworkSnapshot,
        AgentPrivacyBoundary, AgentProbeCode, AgentProbeFailure, AgentProfileSnapshot,
        AgentRoutingMode, AgentRunType, AgentServiceSnapshot, AgentServiceState,
        AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentToolName, AgentToolResult,
        AgentToolRisk, AgentTunSnapshot, NETWORK_SNAPSHOT_SCHEMA_VERSION,
    };

    // Assemble the top-level fixture from small deterministic sections used by projection tests.
    fn sample_snapshot() -> AgentNetworkSnapshot {
        AgentNetworkSnapshot {
            schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
            revision: "revision-42".to_owned(),
            captured_at: 42,
            app_version: "0.23.0".to_owned(),
            os_family: "windows".to_owned(),
            health: AgentHealth::Warning,
            core: sample_core(),
            service: sample_service(),
            system_proxy: sample_proxy(),
            tun: sample_tun(),
            profiles: sample_profiles(),
            telemetry: sample_telemetry(),
            findings: vec![AgentFinding {
                code: AgentFindingCode::TunRuntimeMismatch,
                severity: AgentFindingSeverity::Critical,
                recommended_action: None,
            }],
            probe_failures: vec![AgentProbeFailure {
                code: AgentProbeCode::TelemetryUnavailable,
            }],
            privacy: sample_privacy(),
        }
    }

    // Build the core section with distinct routing values so a wrong projection is visible.
    fn sample_core() -> AgentCoreSnapshot {
        AgentCoreSnapshot {
            state: AgentCoreState::Running,
            run_type: AgentRunType::Service,
            selected_core: "sample-core".to_owned(),
            state_changed_at: 41,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Global),
            observed_routing_mode: Some(AgentRoutingMode::Rule),
            applied_consistency: AgentAppliedState::Stale,
        }
    }

    // Build the service section with a deliberately incompatible runtime marker.
    fn sample_service() -> AgentServiceSnapshot {
        AgentServiceSnapshot {
            desired_enabled: true,
            state: AgentServiceState::Running,
            ipc_connected: true,
            runtime_compatible: Some(false),
        }
    }

    // Build the proxy section with different observed and expected ports for assertions.
    fn sample_proxy() -> AgentSystemProxySnapshot {
        AgentSystemProxySnapshot {
            desired_enabled: true,
            observed_enabled: Some(false),
            observed_host_scope: AgentHostScope::Loopback,
            observed_port: Some(7890),
            expected_mixed_port: 7891,
            matches_expected_endpoint: Some(false),
        }
    }

    // Build the TUN section with intentionally inconsistent desired and generated states.
    fn sample_tun() -> AgentTunSnapshot {
        AgentTunSnapshot {
            desired_enabled: false,
            generated_runtime_enabled: Some(true),
            observed_active: AgentAppliedState::Unknown,
            applied_consistency: AgentAppliedState::Stale,
        }
    }

    // Build profile counts with unique values to make field mixups easy to detect.
    fn sample_profiles() -> AgentProfileSnapshot {
        AgentProfileSnapshot {
            total_count: 5,
            active_count: 2,
            remote_count: 3,
            local_count: 2,
            active_references_valid: false,
        }
    }

    // Build telemetry values used only by the full system snapshot assertion.
    fn sample_telemetry() -> AgentTelemetrySnapshot {
        AgentTelemetrySnapshot {
            state: AgentConnectorState::Connected,
            active_connection_count: Some(7),
            upload_speed: Some(8),
            download_speed: Some(9),
            upload_total: Some("10 B".to_owned()),
            download_total: Some("11 B".to_owned()),
            recent_error_count: 12,
        }
    }

    // Build the privacy boundary with every sensitive-content flag disabled.
    fn sample_privacy() -> AgentPrivacyBoundary {
        AgentPrivacyBoundary {
            contains_raw_logs: false,
            contains_profile_names: false,
            contains_profile_urls: false,
            contains_connection_targets: false,
            contains_controller_secret: false,
        }
    }

    #[test]
    fn manifest_is_closed_read_only_and_versioned() {
        let manifest = agent_manifest();
        assert_eq!(manifest.schema_version, AGENT_MANIFEST_SCHEMA_VERSION);
        assert_eq!(manifest.tools.len(), 7);

        let names = manifest
            .tools
            .iter()
            .map(|tool| tool.name)
            .collect::<HashSet<_>>();
        assert_eq!(names.len(), manifest.tools.len());

        for tool in manifest.tools {
            assert!(tool.read_only);
            assert_eq!(tool.risk, AgentToolRisk::ReadOnly);
            assert!(tool.version > 0);
            assert!(tool.timeout_ms > 0);
            assert_eq!(tool.output_schema_version, AGENT_TOOL_OUTPUT_SCHEMA_VERSION);
            assert!(!tool.description.trim().is_empty());
        }
    }

    #[test]
    fn system_snapshot_projection_preserves_the_full_snapshot() {
        let result = project_tool(sample_snapshot(), AgentToolName::SystemSnapshot);
        let AgentToolResult::SystemSnapshot { output } = result else {
            panic!("system.snapshot projected to the wrong result variant");
        };

        assert_eq!(output.revision, "revision-42");
        assert_eq!(output.core.selected_core, "sample-core");
        assert_eq!(output.telemetry.active_connection_count, Some(7));
    }

    #[test]
    fn diagnostic_projection_preserves_diagnostic_fields() {
        let result = project_tool(sample_snapshot(), AgentToolName::NetworkDiagnose);
        let AgentToolResult::NetworkDiagnose { output } = result else {
            panic!("network.diagnose projected to the wrong result variant");
        };

        assert_eq!(output.revision, "revision-42");
        assert_eq!(output.captured_at, 42);
        assert_eq!(output.health, AgentHealth::Warning);
        assert_eq!(
            output.findings[0].code,
            AgentFindingCode::TunRuntimeMismatch
        );
        assert_eq!(
            output.probe_failures[0].code,
            AgentProbeCode::TelemetryUnavailable
        );
        assert!(!output.privacy.contains_raw_logs);
    }

    #[test]
    fn status_tools_project_the_matching_snapshot_sections() {
        let AgentToolResult::CoreStatus { output } =
            project_tool(sample_snapshot(), AgentToolName::CoreStatus)
        else {
            panic!("core.status projected to the wrong result variant");
        };
        assert_eq!(output.selected_core, "sample-core");
        assert_eq!(output.routing_mode, Some(AgentRoutingMode::Global));

        let AgentToolResult::ProxyStatus { output } =
            project_tool(sample_snapshot(), AgentToolName::ProxyStatus)
        else {
            panic!("proxy.status projected to the wrong result variant");
        };
        assert_eq!(output.observed_port, Some(7890));
        assert_eq!(output.expected_mixed_port, 7891);

        let AgentToolResult::TunStatus { output } =
            project_tool(sample_snapshot(), AgentToolName::TunStatus)
        else {
            panic!("tun.status projected to the wrong result variant");
        };
        assert!(!output.desired_enabled);
        assert_eq!(output.generated_runtime_enabled, Some(true));

        let AgentToolResult::ProfileSummary { output } =
            project_tool(sample_snapshot(), AgentToolName::ProfileSummary)
        else {
            panic!("profile.summary projected to the wrong result variant");
        };
        assert_eq!(output.total_count, 5);
        assert_eq!(output.active_count, 2);

        let AgentToolResult::ServiceStatus { output } =
            project_tool(sample_snapshot(), AgentToolName::ServiceStatus)
        else {
            panic!("service.status projected to the wrong result variant");
        };
        assert_eq!(output.state, AgentServiceState::Running);
        assert_eq!(output.runtime_compatible, Some(false));
    }
}
