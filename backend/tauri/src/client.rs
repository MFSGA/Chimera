use std::sync::Arc;

use crate::features::agent::{
    AgentActionRequest, AgentActionResult, AgentAutonomyPolicyRequest, AgentAutonomyPolicyResult,
    AgentAutonomyPolicySnapshot, AgentBridgeStartResult, AgentBridgeStatus, AgentClient,
    AgentCommandError, AgentExecuteReadOnlyIntentRequest, AgentExecuteReadOnlyIntentResult,
    AgentHistorySnapshot, AgentIntentRequest, AgentIntentResolution, AgentManifest,
    AgentNetworkSnapshot, AgentProposal, agent_manifest, resolve_intent,
};

struct NyanpasuClientInner {
    agent: AgentClient,
}

#[derive(Clone)]
pub(crate) struct NyanpasuClient {
    inner: Arc<NyanpasuClientInner>,
}

impl NyanpasuClient {
    pub(crate) fn new(agent: AgentClient) -> Self {
        Self {
            inner: Arc::new(NyanpasuClientInner { agent }),
        }
    }

    pub(crate) fn agent_manifest(&self) -> AgentManifest {
        agent_manifest()
    }

    pub(crate) fn agent_resolve_intent(
        &self,
        request: AgentIntentRequest,
    ) -> AgentIntentResolution {
        resolve_intent(request)
    }

    pub(crate) async fn agent_network_snapshot(
        &self,
    ) -> Result<AgentNetworkSnapshot, AgentCommandError> {
        self.inner.agent.network_snapshot().await
    }

    pub(crate) async fn agent_execute_read_only_intent(
        &self,
        request: AgentExecuteReadOnlyIntentRequest,
    ) -> Result<AgentExecuteReadOnlyIntentResult, AgentCommandError> {
        self.inner.agent.execute_read_only_intent(request).await
    }

    pub(crate) fn agent_authorize_autonomy(
        &self,
        request: AgentAutonomyPolicyRequest,
    ) -> AgentAutonomyPolicyResult {
        self.inner.agent.authorize_autonomy(request)
    }

    pub(crate) fn agent_autonomy_policy(&self) -> AgentAutonomyPolicySnapshot {
        self.inner.agent.autonomy_policy()
    }

    pub(crate) fn agent_revoke_autonomy(&self) -> AgentAutonomyPolicySnapshot {
        self.inner.agent.revoke_autonomy()
    }

    pub(crate) async fn agent_history(&self) -> Result<AgentHistorySnapshot, AgentCommandError> {
        self.inner.agent.history().await
    }

    pub(crate) async fn agent_clear_history(
        &self,
        owner_label: &str,
    ) -> Result<AgentHistorySnapshot, AgentCommandError> {
        self.inner.agent.clear_history(owner_label).await
    }

    pub(crate) async fn agent_propose_action(
        &self,
        owner_label: &str,
        action: AgentActionRequest,
    ) -> Result<AgentProposal, AgentCommandError> {
        self.inner.agent.propose_action(owner_label, action).await
    }

    pub(crate) async fn agent_execute_action(
        &self,
        owner_label: &str,
        proposal_id: &str,
        digest: &str,
    ) -> Result<AgentActionResult, AgentCommandError> {
        self.inner
            .agent
            .execute_action(owner_label, proposal_id, digest)
            .await
    }

    pub(crate) async fn agent_cancel_action(
        &self,
        owner_label: &str,
        proposal_id: &str,
    ) -> Result<bool, AgentCommandError> {
        self.inner
            .agent
            .cancel_action(owner_label, proposal_id)
            .await
    }

    pub(crate) async fn agent_start_bridge(
        &self,
    ) -> Result<AgentBridgeStartResult, AgentCommandError> {
        self.inner.agent.start_bridge().await
    }

    pub(crate) async fn agent_bridge_status(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        self.inner.agent.bridge_status().await
    }

    pub(crate) async fn agent_stop_bridge(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        self.inner.agent.stop_bridge().await
    }
}
