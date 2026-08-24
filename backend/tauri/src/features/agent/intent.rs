use std::{future::Future, net::IpAddr};

use super::model::{
    AgentClarificationChoice, AgentClarificationCode, AgentExecuteReadOnlyIntentRequest,
    AgentExecuteReadOnlyIntentResult, AgentIntent, AgentIntentRequest, AgentIntentResolution,
    AgentNetworkSnapshot, AgentRoutingMode, AgentServiceOperation, AgentUnsupportedIntentReason,
};

const MAX_INTENT_TEXT_LENGTH: usize = 160;
const MAX_INTENT_TEXT_BYTES: usize = MAX_INTENT_TEXT_LENGTH * 4;

pub(crate) fn resolve_intent(request: AgentIntentRequest) -> AgentIntentResolution {
    if request.text.len() > MAX_INTENT_TEXT_BYTES
        || request.text.chars().count() > MAX_INTENT_TEXT_LENGTH
    {
        return AgentIntentResolution::Unsupported {
            reason: AgentUnsupportedIntentReason::InputTooLong,
        };
    }

    if contains_probe_target(&request.text) {
        return AgentIntentResolution::Unsupported {
            reason: AgentUnsupportedIntentReason::NoMatchingIntent,
        };
    }

    let text = normalize(&request.text);
    if text.is_empty() {
        return AgentIntentResolution::Unsupported {
            reason: AgentUnsupportedIntentReason::EmptyInput,
        };
    }
    if text.chars().count() > MAX_INTENT_TEXT_LENGTH {
        return AgentIntentResolution::Unsupported {
            reason: AgentUnsupportedIntentReason::InputTooLong,
        };
    }

    if matches_exact(
        &text,
        &[
            "检查上网状态",
            "检查当前主机上网状态",
            "检查网络连接状态",
            "检测上网状态",
            "查看上网状态",
            "checkinternetstatus",
            "checkhostconnectivity",
            "checknetworkconnectivity",
            "檢查上網狀態",
            "檢查目前主機上網狀態",
            "檢查網路連線狀態",
            "проверитьсостояниесети",
            "естьлиинтернет",
        ],
    ) {
        return AgentIntentResolution::Resolved {
            intent: AgentIntent::HostConnectivity,
        };
    }

    if matches_any(
        &text,
        &["开启代理", "打开代理", "enableproxy", "turnonproxy"],
    ) {
        return AgentIntentResolution::NeedsClarification {
            choices: vec![
                choice(
                    AgentClarificationCode::EnableTun,
                    AgentIntent::SetTunEnabled { enabled: true },
                ),
                choice(
                    AgentClarificationCode::UseGlobalRouting,
                    AgentIntent::SetRoutingMode {
                        mode: AgentRoutingMode::Global,
                    },
                ),
                choice(
                    AgentClarificationCode::DiagnoseNetwork,
                    AgentIntent::Diagnose,
                ),
            ],
        };
    }

    let intent = resolve_closed_intent(&text);
    intent.map_or(
        AgentIntentResolution::Unsupported {
            reason: AgentUnsupportedIntentReason::NoMatchingIntent,
        },
        |intent| AgentIntentResolution::Resolved { intent },
    )
}

pub(crate) async fn execute_read_only_intent<F>(
    request: AgentExecuteReadOnlyIntentRequest,
    snapshot: F,
) -> AgentExecuteReadOnlyIntentResult
where
    F: Future<Output = AgentNetworkSnapshot>,
{
    match resolve_intent(AgentIntentRequest { text: request.text }) {
        AgentIntentResolution::Resolved {
            intent: AgentIntent::Diagnose,
        } => AgentExecuteReadOnlyIntentResult::Diagnosed {
            snapshot: Box::new(snapshot.await),
        },
        AgentIntentResolution::Resolved {
            intent: AgentIntent::HostConnectivity,
        } => AgentExecuteReadOnlyIntentResult::HostConnectivity {
            connectivity: snapshot.await.connectivity,
        },
        AgentIntentResolution::Resolved { intent } => {
            AgentExecuteReadOnlyIntentResult::ProposalRequired { intent }
        }
        AgentIntentResolution::NeedsClarification { choices } => {
            AgentExecuteReadOnlyIntentResult::NeedsClarification { choices }
        }
        AgentIntentResolution::Unsupported { reason } => {
            AgentExecuteReadOnlyIntentResult::Unsupported { reason }
        }
    }
}

fn resolve_closed_intent(text: &str) -> Option<AgentIntent> {
    let rules: &[(&[&str], AgentIntent)] = &[
        (
            &[
                "开启tun",
                "打开tun",
                "启用tun",
                "打开虚拟网卡",
                "enabletun",
                "turnontun",
            ],
            AgentIntent::SetTunEnabled { enabled: true },
        ),
        (
            &[
                "关闭tun",
                "停用tun",
                "关掉虚拟网卡",
                "关闭虚拟网卡",
                "disabletun",
                "turnofftun",
            ],
            AgentIntent::SetTunEnabled { enabled: false },
        ),
        (
            &["开启系统代理", "打开系统代理", "enablesystemproxy"],
            AgentIntent::SetSystemProxyEnabled { enabled: true },
        ),
        (
            &["关闭系统代理", "停用系统代理", "disablesystemproxy"],
            AgentIntent::SetSystemProxyEnabled { enabled: false },
        ),
        (
            &[
                "启用服务模式",
                "开启服务模式",
                "打开服务模式",
                "enableservicemode",
                "turnonservicemode",
            ],
            AgentIntent::SetServiceMode { enabled: true },
        ),
        (
            &[
                "停用服务模式",
                "关闭服务模式",
                "退出服务模式",
                "disableservicemode",
                "turnoffservicemode",
            ],
            AgentIntent::SetServiceMode { enabled: false },
        ),
        (
            &[
                "全局模式",
                "切换全局",
                "全局路由",
                "globalmode",
                "useglobal",
            ],
            AgentIntent::SetRoutingMode {
                mode: AgentRoutingMode::Global,
            },
        ),
        (
            &["规则模式", "切换规则", "规则路由", "rulemode", "userule"],
            AgentIntent::SetRoutingMode {
                mode: AgentRoutingMode::Rule,
            },
        ),
        (
            &["直连模式", "切换直连", "directmode", "usedirect"],
            AgentIntent::SetRoutingMode {
                mode: AgentRoutingMode::Direct,
            },
        ),
        (
            &["启动核心", "启动内核", "startcore"],
            AgentIntent::StartCore,
        ),
        (
            &["重启核心", "重启内核", "restartcore"],
            AgentIntent::RestartCore,
        ),
        (
            &["重连遥测", "重新连接遥测", "reconnecttelemetry"],
            AgentIntent::ReconnectTelemetry,
        ),
        (
            &["启动服务", "startservice"],
            AgentIntent::ControlService {
                operation: AgentServiceOperation::Start,
            },
        ),
        (
            &["停止服务", "stopservice"],
            AgentIntent::ControlService {
                operation: AgentServiceOperation::Stop,
            },
        ),
        (
            &["重启服务", "restartservice"],
            AgentIntent::ControlService {
                operation: AgentServiceOperation::Restart,
            },
        ),
        (
            &["修复系统代理", "修复代理端点", "repairproxyendpoint"],
            AgentIntent::RepairSystemProxyEndpoint,
        ),
        (
            &["关闭残留代理", "清理残留代理", "disablestaleproxy"],
            AgentIntent::DisableStaleSystemProxy,
        ),
        (
            &["诊断", "检查网络", "刷新诊断", "diagnose", "checknetwork"],
            AgentIntent::Diagnose,
        ),
    ];

    rules
        .iter()
        .find_map(|(phrases, intent)| matches_any(text, phrases).then(|| intent.clone()))
}

fn choice(code: AgentClarificationCode, intent: AgentIntent) -> AgentClarificationChoice {
    AgentClarificationChoice { code, intent }
}

fn normalize(text: &str) -> String {
    text.chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn contains_probe_target(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(character, ',' | ';' | '(' | ')' | '[' | ']' | '"' | '\'')
        });
        if token.contains("://") || token.parse::<IpAddr>().is_ok() {
            return true;
        }

        let host = token.trim_end_matches(['.', '?', '!', ':']);
        host.is_ascii()
            && host.contains('.')
            && host.split('.').all(|label| {
                !label.is_empty()
                    && label
                        .chars()
                        .all(|character| character.is_ascii_alphanumeric() || character == '-')
            })
    })
}

fn matches_any(text: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| text.contains(phrase))
}

fn matches_exact(text: &str, phrases: &[&str]) -> bool {
    phrases.contains(&text)
}

#[cfg(test)]
mod tests {
    use std::{future::pending, time::Duration};

    use tokio::time::timeout;

    use super::{execute_read_only_intent, resolve_intent};
    use crate::features::agent::{
        host_connectivity::unavailable_host_connectivity,
        model::{
            AgentAppliedState, AgentClarificationCode, AgentConnectorState, AgentCoreSnapshot,
            AgentCoreState, AgentExecuteReadOnlyIntentRequest, AgentExecuteReadOnlyIntentResult,
            AgentHealth, AgentHostScope, AgentIntent, AgentIntentRequest, AgentIntentResolution,
            AgentNetworkSnapshot, AgentOsFamily, AgentPlatformReadinessSnapshot,
            AgentPrivacyBoundary, AgentProcessPrivilegeStatus, AgentProfileSnapshot,
            AgentRoutingMode, AgentRunType, AgentSelectedCore, AgentServiceSnapshot,
            AgentServiceState, AgentSystemDnsVerificationStatus, AgentSystemProxySnapshot,
            AgentTelemetrySnapshot, AgentTunPermissionReadiness, AgentTunSnapshot,
            AgentTunVerificationStatus, AgentUnsupportedIntentReason,
            NETWORK_SNAPSHOT_SCHEMA_VERSION,
        },
    };

    fn resolve(text: &str) -> AgentIntentResolution {
        resolve_intent(AgentIntentRequest { text: text.into() })
    }

    #[test]
    fn resolves_closed_tun_routing_and_diagnostic_intents() {
        assert_eq!(
            resolve("帮我开启 TUN"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetTunEnabled { enabled: true },
            }
        );
        assert_eq!(
            resolve("打开系统代理"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetSystemProxyEnabled { enabled: true },
            }
        );
        assert_eq!(
            resolve("启用服务模式"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetServiceMode { enabled: true },
            }
        );
        assert_eq!(
            resolve("disable service mode"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetServiceMode { enabled: false },
            }
        );
        assert_eq!(
            resolve("启动核心"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::StartCore,
            }
        );
        assert_eq!(
            resolve("切换到规则模式"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetRoutingMode {
                    mode: AgentRoutingMode::Rule,
                },
            }
        );
        assert_eq!(
            resolve("检查网络"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::Diagnose,
            }
        );
    }

    #[test]
    fn resolves_only_fixed_host_connectivity_phrases() {
        for text in [
            "检查上网状态",
            "检查当前主机上网状态",
            "check internet status",
            "check host connectivity",
            "проверить состояние сети",
            "檢查上網狀態",
        ] {
            assert_eq!(
                resolve(text),
                AgentIntentResolution::Resolved {
                    intent: AgentIntent::HostConnectivity,
                },
                "{text}"
            );
        }

        for text in [
            "检查 example.com 上网状态",
            "check internet status https://example.com",
            "check host connectivity 8.8.8.8",
            "检查某个目标的网络连接状态",
        ] {
            assert_eq!(
                resolve(text),
                AgentIntentResolution::Unsupported {
                    reason: AgentUnsupportedIntentReason::NoMatchingIntent,
                },
                "{text}"
            );
        }
    }

    #[test]
    fn ambiguous_proxy_request_requires_fixed_clarification() {
        let AgentIntentResolution::NeedsClarification { choices } = resolve("帮我开启代理")
        else {
            panic!("ambiguous proxy request must not resolve directly");
        };
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0].code, AgentClarificationCode::EnableTun);
        assert_eq!(choices[1].code, AgentClarificationCode::UseGlobalRouting);
        assert_eq!(choices[2].code, AgentClarificationCode::DiagnoseNetwork);
    }

    #[tokio::test]
    async fn executes_closed_read_only_intents_and_projects_only_the_requested_result() {
        let snapshot = test_snapshot();
        let diagnosed = execute_read_only_intent(
            AgentExecuteReadOnlyIntentRequest {
                text: "诊断".into(),
            },
            async { snapshot.clone() },
        )
        .await;
        let AgentExecuteReadOnlyIntentResult::Diagnosed {
            snapshot: diagnosed_snapshot,
        } = diagnosed
        else {
            panic!("diagnose must return the full privacy-safe snapshot");
        };
        assert_eq!(diagnosed_snapshot.revision, "intent-test-revision");

        let connectivity = execute_read_only_intent(
            AgentExecuteReadOnlyIntentRequest {
                text: "检查上网状态".into(),
            },
            async { snapshot },
        )
        .await;
        let AgentExecuteReadOnlyIntentResult::HostConnectivity { connectivity } = connectivity
        else {
            panic!("host connectivity must return only its closed snapshot");
        };
        assert_eq!(
            connectivity.status,
            crate::features::agent::model::AgentHostConnectivityStatus::Indeterminate
        );
    }

    #[tokio::test]
    async fn write_and_unresolved_intents_never_poll_the_snapshot_future() {
        let write_result = timeout(
            Duration::from_millis(50),
            execute_read_only_intent(
                AgentExecuteReadOnlyIntentRequest {
                    text: "开启系统代理".into(),
                },
                pending::<AgentNetworkSnapshot>(),
            ),
        )
        .await
        .expect("write intent must return without reading the snapshot");
        assert!(matches!(
            write_result,
            AgentExecuteReadOnlyIntentResult::ProposalRequired {
                intent: AgentIntent::SetSystemProxyEnabled { enabled: true }
            }
        ));

        let clarification = timeout(
            Duration::from_millis(50),
            execute_read_only_intent(
                AgentExecuteReadOnlyIntentRequest {
                    text: "帮我开启代理".into(),
                },
                pending::<AgentNetworkSnapshot>(),
            ),
        )
        .await
        .expect("clarification must return without reading the snapshot");
        assert!(matches!(
            clarification,
            AgentExecuteReadOnlyIntentResult::NeedsClarification { .. }
        ));

        let unsupported = timeout(
            Duration::from_millis(50),
            execute_read_only_intent(
                AgentExecuteReadOnlyIntentRequest {
                    text: "执行未注册动作".into(),
                },
                pending::<AgentNetworkSnapshot>(),
            ),
        )
        .await
        .expect("unsupported intent must return without reading the snapshot");
        assert!(matches!(
            unsupported,
            AgentExecuteReadOnlyIntentResult::Unsupported {
                reason: AgentUnsupportedIntentReason::NoMatchingIntent
            }
        ));
    }

    #[test]
    fn unknown_empty_and_oversized_inputs_are_closed() {
        assert_eq!(
            resolve(""),
            AgentIntentResolution::Unsupported {
                reason: AgentUnsupportedIntentReason::EmptyInput,
            }
        );
        assert_eq!(
            resolve("帮我做一个未注册的动作"),
            AgentIntentResolution::Unsupported {
                reason: AgentUnsupportedIntentReason::NoMatchingIntent,
            }
        );
        for oversized in [
            "a".repeat(161),
            "!".repeat(161),
            format!("开启tun{}", "!".repeat(160)),
            "🦀".repeat(161),
        ] {
            assert_eq!(
                resolve(&oversized),
                AgentIntentResolution::Unsupported {
                    reason: AgentUnsupportedIntentReason::InputTooLong,
                }
            );
        }
    }

    fn test_snapshot() -> AgentNetworkSnapshot {
        AgentNetworkSnapshot {
            schema_version: NETWORK_SNAPSHOT_SCHEMA_VERSION,
            revision: "intent-test-revision".into(),
            captured_at: 0,
            app_version: "test".into(),
            os_family: AgentOsFamily::Windows,
            health: AgentHealth::Healthy,
            core: AgentCoreSnapshot {
                state: AgentCoreState::Running,
                run_type: AgentRunType::Normal,
                selected_core: AgentSelectedCore::Mihomo,
                state_changed_at: 0,
                runtime_config_present: true,
                routing_mode: Some(AgentRoutingMode::Rule),
                observed_routing_mode: Some(AgentRoutingMode::Rule),
                applied_consistency: AgentAppliedState::Consistent,
            },
            service: AgentServiceSnapshot {
                desired_enabled: false,
                state: AgentServiceState::NotInstalled,
                ipc_connected: false,
                runtime_compatible: None,
            },
            system_proxy: AgentSystemProxySnapshot {
                desired_enabled: false,
                observed_enabled: Some(false),
                observed_host_scope: AgentHostScope::Loopback,
                observed_port: Some(7890),
                expected_mixed_port: 7890,
                matches_expected_endpoint: Some(true),
            },
            tun: AgentTunSnapshot {
                desired_enabled: false,
                generated_runtime_enabled: Some(false),
                observed_enabled: Some(false),
                applied_consistency: AgentAppliedState::Consistent,
            },
            profiles: AgentProfileSnapshot {
                total_count: 1,
                active_count: 1,
                remote_count: 0,
                local_count: 1,
                active_references_valid: true,
            },
            telemetry: AgentTelemetrySnapshot {
                state: AgentConnectorState::Connected,
                active_connection_count: Some(0),
                upload_speed: Some(0),
                download_speed: Some(0),
                upload_total: Some(0),
                download_total: Some(0),
                recent_error_count: 0,
            },
            connectivity: unavailable_host_connectivity(),
            platform_readiness: AgentPlatformReadinessSnapshot {
                process_privilege: AgentProcessPrivilegeStatus::Unknown,
                service_mode_available: Some(false),
                tun_permission: AgentTunPermissionReadiness::NotRequired,
                tun_verification: AgentTunVerificationStatus::NotRequested,
                system_dns_verification: AgentSystemDnsVerificationStatus::NotRequired,
                reasons: Vec::new(),
            },
            findings: Vec::new(),
            probe_failures: Vec::new(),
            recommendations: Vec::new(),
            privacy: AgentPrivacyBoundary::privacy_safe(),
        }
    }
}
