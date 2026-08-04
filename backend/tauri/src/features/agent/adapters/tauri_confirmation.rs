use std::sync::Arc;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};

use super::super::{
    model::{AgentCommandError, AgentProposal, AgentResult},
    ports::AgentConfirmationPort,
};

pub(crate) struct TauriAgentConfirmation {
    app: AppHandle,
    dialog_gate: Arc<tokio::sync::Semaphore>,
}

impl TauriAgentConfirmation {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self {
            app,
            dialog_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    async fn confirm_warning(&self, owner_label: &str, message: String) -> AgentResult<bool> {
        let window = self
            .app
            .get_webview_window(owner_label)
            .ok_or(AgentCommandError::ActionFailed)?;
        let permit = self
            .dialog_gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| AgentCommandError::ActionFailed)?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            window
                .dialog()
                .message(message)
                .title("Chimera")
                .kind(MessageDialogKind::Warning)
                .buttons(MessageDialogButtons::YesNo)
                .parent(&window)
                .blocking_show()
        })
        .await
        .map_err(|_| AgentCommandError::ActionFailed)
    }
}

#[async_trait::async_trait]
impl AgentConfirmationPort for TauriAgentConfirmation {
    async fn confirm(&self, owner_label: &str, proposal: &AgentProposal) -> AgentResult<bool> {
        self.confirm_warning(owner_label, proposal_confirmation_message(proposal))
            .await
    }

    async fn confirm_history_clear(&self, owner_label: &str) -> AgentResult<bool> {
        self.confirm_warning(
            owner_label,
            "Clear Chimera Agent history? This cannot be undone.".to_owned(),
        )
        .await
    }
}

fn proposal_confirmation_message(proposal: &AgentProposal) -> String {
    let mut lines = vec![
        "Confirm Chimera network change".to_owned(),
        format!("risk: {}", proposal.risk.as_str()),
        "changes:".to_owned(),
    ];
    for change in &proposal.changes {
        lines.push(format!(
            "- {}: {} -> {}",
            change.field.as_str(),
            change.before.as_str(),
            change.after.as_str()
        ));
    }
    if !proposal.impacts.is_empty() {
        lines.push("impacts:".to_owned());
        lines.extend(
            proposal
                .impacts
                .iter()
                .map(|impact| format!("- {}", impact.as_str())),
        );
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::proposal_confirmation_message;
    use crate::features::agent::model::{
        AgentActionRequest, AgentActionRisk, AgentImpact, AgentProposal, AgentRoutingMode,
        AgentStateChange, AgentStateField, AgentStateValue,
    };

    #[test]
    fn native_confirmation_discloses_all_closed_risks_changes_and_impacts() {
        let proposal = AgentProposal {
            id: "a".repeat(32),
            digest: "b".repeat(64),
            action: AgentActionRequest::SetRoutingMode {
                mode: AgentRoutingMode::Global,
            },
            risk: AgentActionRisk::TrafficChange,
            impacts: vec![
                AgentImpact::ExistingConnectionsMayChange,
                AgentImpact::TrafficMayBypassProxy,
            ],
            changes: vec![
                AgentStateChange {
                    field: AgentStateField::RoutingMode,
                    before: AgentStateValue::Rule,
                    after: AgentStateValue::Global,
                },
                AgentStateChange {
                    field: AgentStateField::TelemetryConnector,
                    before: AgentStateValue::Connected,
                    after: AgentStateValue::Disconnected,
                },
            ],
            snapshot_revision: "c".repeat(64),
            created_at: 1,
            expires_at: 2,
            requires_confirmation: true,
        };

        let message = proposal_confirmation_message(&proposal);
        for expected in [
            "risk: traffic_change",
            "- routing_mode: rule -> global",
            "- telemetry_connector: connected -> disconnected",
            "- existing_connections_may_change",
            "- traffic_may_bypass_proxy",
        ] {
            assert!(message.contains(expected), "missing {expected}: {message}");
        }
    }
}
