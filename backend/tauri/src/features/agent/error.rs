use serde::Serialize;
use specta::Type;

#[derive(Debug, Serialize, Type, thiserror::Error)]
pub enum AgentCommandError {
    #[error("agent_action_not_available")]
    #[serde(rename = "agent_action_not_available")]
    ActionNotAvailable,
    #[error("agent_proposal_not_found")]
    #[serde(rename = "agent_proposal_not_found")]
    ProposalNotFound,
    #[error("agent_proposal_expired")]
    #[serde(rename = "agent_proposal_expired")]
    ProposalExpired,
    #[error("agent_proposal_digest_mismatch")]
    #[serde(rename = "agent_proposal_digest_mismatch")]
    ProposalDigestMismatch,
    #[error("agent_network_state_changed")]
    #[serde(rename = "agent_network_state_changed")]
    NetworkStateChanged,
    #[error("agent_proposal_rate_limited")]
    #[serde(rename = "agent_proposal_rate_limited")]
    ProposalRateLimited,
    #[error("agent_proposal_limit_reached")]
    #[serde(rename = "agent_proposal_limit_reached")]
    ProposalLimitReached,
    #[error("agent_confirmation_declined")]
    #[serde(rename = "agent_confirmation_declined")]
    ConfirmationDeclined,
    #[error("agent_action_failed")]
    #[serde(rename = "agent_action_failed")]
    ActionFailed,
    #[error("agent_action_partially_applied")]
    #[serde(rename = "agent_action_partially_applied")]
    PartialApply,
    #[error("agent_action_verification_failed")]
    #[serde(rename = "agent_action_verification_failed")]
    VerificationFailed,
    #[error("agent_bridge_start_failed")]
    #[serde(rename = "agent_bridge_start_failed")]
    BridgeStartFailed,
    #[error("agent_history_clear_failed")]
    #[serde(rename = "agent_history_clear_failed")]
    HistoryClearFailed,
}

pub(crate) type AgentResult<T> = Result<T, AgentCommandError>;
