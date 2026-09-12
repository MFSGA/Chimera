use super::model::{
    AgentIntent, AgentIntentRequest, AgentIntentResolution, AgentRoutingMode,
    AgentUnsupportedIntentReason,
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

    resolve_closed_intent(&text).map_or(
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
            &["开启tun", "打开tun", "启用tun", "enabletun", "turnontun"],
            AgentIntent::SetTunEnabled { enabled: true },
        ),
        (
            &["关闭tun", "禁用tun", "停用tun", "disabletun", "turnofftun"],
            AgentIntent::SetTunEnabled { enabled: false },
        ),
        (
            &[
                "开启系统代理",
                "打开系统代理",
                "启用系统代理",
                "enablesystemproxy",
                "turnonsystemproxy",
            ],
            AgentIntent::SetSystemProxyEnabled { enabled: true },
        ),
        (
            &[
                "关闭系统代理",
                "禁用系统代理",
                "停用系统代理",
                "disablesystemproxy",
                "turnoffsystemproxy",
            ],
            AgentIntent::SetSystemProxyEnabled { enabled: false },
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
        AgentIntent, AgentIntentRequest, AgentIntentResolution, AgentRoutingMode,
        AgentUnsupportedIntentReason,
    };

    fn resolve(text: &str) -> AgentIntentResolution {
        resolve_intent(AgentIntentRequest { text: text.into() })
    }

    #[test]
    fn resolves_only_current_closed_capabilities() {
        assert_eq!(
            resolve("帮我切换到规则模式"),
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
        assert_eq!(
            resolve("清理残留代理"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::DisableStaleSystemProxy,
            }
        );
        assert_eq!(
            resolve("帮我开启 TUN"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetTunEnabled { enabled: true },
            }
        );
        assert_eq!(
            resolve("关闭系统代理"),
            AgentIntentResolution::Resolved {
                intent: AgentIntent::SetSystemProxyEnabled { enabled: false },
            }
        );
    }

    #[test]
    fn unsupported_or_future_capabilities_fail_closed() {
        for text in ["", "启动核心", "重启服务", "修改配置文件"] {
            let expected = if text.is_empty() {
                AgentUnsupportedIntentReason::EmptyInput
            } else {
                AgentUnsupportedIntentReason::NoMatchingIntent
            };
            assert_eq!(
                resolve(text),
                AgentIntentResolution::Unsupported { reason: expected }
            );
        }
    }

    #[test]
    fn oversized_input_is_rejected_before_normalization() {
        assert_eq!(
            resolve(&"!".repeat(161)),
            AgentIntentResolution::Unsupported {
                reason: AgentUnsupportedIntentReason::InputTooLong,
            }
        );
        assert_eq!(
            resolve(&"🦀".repeat(161)),
            AgentIntentResolution::Unsupported {
                reason: AgentUnsupportedIntentReason::InputTooLong,
            }
        );
    }
}
