use super::model::{
    AgentClarificationChoice, AgentClarificationCode, AgentIntent, AgentIntentRequest,
    AgentIntentResolution, AgentRoutingMode, AgentServiceOperation, AgentUnsupportedIntentReason,
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

fn matches_any(text: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|phrase| text.contains(phrase))
}

#[cfg(test)]
mod tests {
    use super::resolve_intent;
    use crate::features::agent::model::{
        AgentClarificationCode, AgentIntent, AgentIntentRequest, AgentIntentResolution,
        AgentRoutingMode, AgentUnsupportedIntentReason,
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
}
