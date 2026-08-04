use std::{sync::Arc, time::Duration};

use anyhow::Context;
use ractor::{Actor, ActorRef, rpc::CallResult};

use super::{
    actor::{
        AgentBridgeActor, AgentBridgeActorArgs, AgentBridgeMessage, AgentHistoryActor,
        AgentHistoryActorArgs, AgentHistoryMessage, AgentProposalActor, AgentProposalActorArgs,
        AgentProposalMessage,
    },
    bridge::{AgentBridgeStartResult, AgentBridgeStatus},
    history::{AgentAuditOutcome, AgentHistorySnapshot},
    model::{
        AgentActionRequest, AgentActionResult, AgentCommandError, AgentNetworkSnapshot,
        AgentProposal,
    },
    ports::{
        AgentBridgePort, AgentConfirmationPort, AgentHistoryPersistencePort, AgentRuntimePort,
    },
};

const AGENT_PROPOSAL_TIMEOUT: Duration = Duration::from_secs(30);
const AGENT_EXECUTION_TIMEOUT: Duration = Duration::from_secs(240);
const AGENT_CANCEL_TIMEOUT: Duration = Duration::from_secs(15);
const AGENT_HISTORY_TIMEOUT: Duration = Duration::from_secs(90);
const AGENT_HISTORY_CONFIRMATION_TIMEOUT: Duration = Duration::from_secs(60);
const AGENT_BRIDGE_TIMEOUT: Duration = Duration::from_secs(15);

struct AgentActorHandle<Message> {
    actor: ActorRef<Message>,
}

impl<Message> AgentActorHandle<Message> {
    fn new(actor: ActorRef<Message>) -> Self {
        Self { actor }
    }
}

impl<Message> Drop for AgentActorHandle<Message> {
    fn drop(&mut self) {
        self.actor.stop(None);
    }
}

#[derive(Clone)]
pub(crate) struct AgentHistoryClient {
    inner: Arc<AgentActorHandle<AgentHistoryMessage>>,
}

impl AgentHistoryClient {
    fn new(actor: ActorRef<AgentHistoryMessage>) -> Self {
        Self {
            inner: Arc::new(AgentActorHandle::new(actor)),
        }
    }

    async fn record_snapshot(&self, snapshot: AgentNetworkSnapshot) {
        let result = self
            .inner
            .actor
            .call(
                move |reply| AgentHistoryMessage::RecordSnapshot {
                    snapshot: Box::new(snapshot),
                    reply,
                },
                Some(AGENT_HISTORY_TIMEOUT),
            )
            .await;
        if !matches!(result, Ok(CallResult::Success(()))) {
            tracing::warn!(target: "agent_audit", "failed to record agent diagnostic history");
        }
    }

    pub(crate) async fn record_audit(
        &self,
        proposal_id: &str,
        action: &AgentActionRequest,
        snapshot_revision: &str,
        outcome: AgentAuditOutcome,
    ) {
        let proposal_id = proposal_id.to_owned();
        let action = action.clone();
        let snapshot_revision = snapshot_revision.to_owned();
        let result = self
            .inner
            .actor
            .call(
                move |reply| AgentHistoryMessage::RecordAudit {
                    proposal_id,
                    action,
                    snapshot_revision,
                    outcome,
                    reply,
                },
                Some(AGENT_HISTORY_TIMEOUT),
            )
            .await;
        if !matches!(result, Ok(CallResult::Success(()))) {
            tracing::warn!(target: "agent_audit", "failed to record agent action history");
        }
    }

    async fn snapshot(&self) -> Result<AgentHistorySnapshot, AgentCommandError> {
        match self
            .inner
            .actor
            .call(AgentHistoryMessage::Snapshot, Some(AGENT_HISTORY_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(history)) => Ok(history),
            _ => Err(AgentCommandError::ActionFailed),
        }
    }

    async fn clear(&self) -> Result<AgentHistorySnapshot, AgentCommandError> {
        match self
            .inner
            .actor
            .call(AgentHistoryMessage::Clear, Some(AGENT_HISTORY_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(result)) => result,
            _ => Err(AgentCommandError::HistoryClearFailed),
        }
    }
}

#[derive(Clone)]
struct AgentProposalClient {
    inner: Arc<AgentActorHandle<AgentProposalMessage>>,
}

impl AgentProposalClient {
    fn new(actor: ActorRef<AgentProposalMessage>) -> Self {
        Self {
            inner: Arc::new(AgentActorHandle::new(actor)),
        }
    }

    async fn propose(
        &self,
        owner_label: &str,
        action: AgentActionRequest,
    ) -> Result<AgentProposal, AgentCommandError> {
        let owner_label = owner_label.to_owned();
        match self
            .inner
            .actor
            .call(
                move |reply| AgentProposalMessage::Propose {
                    owner_label,
                    action,
                    reply,
                },
                Some(AGENT_PROPOSAL_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            _ => Err(AgentCommandError::ActionFailed),
        }
    }

    async fn execute(
        &self,
        owner_label: &str,
        proposal_id: &str,
        digest: &str,
    ) -> Result<AgentActionResult, AgentCommandError> {
        let owner_label = owner_label.to_owned();
        let proposal_id = proposal_id.to_owned();
        let digest = digest.to_owned();
        match self
            .inner
            .actor
            .call(
                move |reply| AgentProposalMessage::Execute {
                    owner_label,
                    proposal_id,
                    digest,
                    reply,
                },
                Some(AGENT_EXECUTION_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(result)) => result,
            _ => Err(AgentCommandError::ActionFailed),
        }
    }

    async fn cancel(
        &self,
        owner_label: &str,
        proposal_id: &str,
    ) -> Result<bool, AgentCommandError> {
        let owner_label = owner_label.to_owned();
        let proposal_id = proposal_id.to_owned();
        match self
            .inner
            .actor
            .call(
                move |reply| AgentProposalMessage::Cancel {
                    owner_label,
                    proposal_id,
                    reply,
                },
                Some(AGENT_CANCEL_TIMEOUT),
            )
            .await
        {
            Ok(CallResult::Success(cancelled)) => Ok(cancelled),
            _ => Err(AgentCommandError::ActionFailed),
        }
    }
}

#[derive(Clone)]
struct AgentBridgeClient {
    inner: Arc<AgentActorHandle<AgentBridgeMessage>>,
}

impl AgentBridgeClient {
    fn new(actor: ActorRef<AgentBridgeMessage>) -> Self {
        Self {
            inner: Arc::new(AgentActorHandle::new(actor)),
        }
    }

    async fn start(&self) -> Result<AgentBridgeStartResult, AgentCommandError> {
        match self
            .inner
            .actor
            .call(AgentBridgeMessage::Start, Some(AGENT_BRIDGE_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(result)) => result,
            _ => Err(AgentCommandError::BridgeStartFailed),
        }
    }

    async fn status(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        match self
            .inner
            .actor
            .call(AgentBridgeMessage::Status, Some(AGENT_BRIDGE_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => Ok(status),
            _ => Err(AgentCommandError::BridgeStartFailed),
        }
    }

    async fn stop(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        match self
            .inner
            .actor
            .call(AgentBridgeMessage::Stop, Some(AGENT_BRIDGE_TIMEOUT))
            .await
        {
            Ok(CallResult::Success(status)) => Ok(status),
            _ => Err(AgentCommandError::BridgeStartFailed),
        }
    }
}

async fn confirm_history_clear(
    confirmation: &dyn AgentConfirmationPort,
    owner_label: &str,
    timeout: Duration,
) -> Result<(), AgentCommandError> {
    match tokio::time::timeout(timeout, confirmation.confirm_history_clear(owner_label)).await {
        Ok(Ok(true)) => Ok(()),
        Ok(Ok(false)) => Err(AgentCommandError::ConfirmationDeclined),
        _ => Err(AgentCommandError::HistoryClearFailed),
    }
}

#[derive(Clone)]
pub(crate) struct AgentClient {
    runtime: Arc<dyn AgentRuntimePort>,
    confirmation: Arc<dyn AgentConfirmationPort>,
    proposals: AgentProposalClient,
    history: AgentHistoryClient,
    bridge: AgentBridgeClient,
}

impl AgentClient {
    pub(crate) fn new(
        runtime: Arc<dyn AgentRuntimePort>,
        confirmation: Arc<dyn AgentConfirmationPort>,
        bridge: Box<dyn AgentBridgePort>,
        history_persistence: Arc<dyn AgentHistoryPersistencePort>,
    ) -> anyhow::Result<Self> {
        let runtime_for_proposals = runtime.clone();
        let confirmation_for_proposals = confirmation.clone();
        let (history, bridge, proposals) = tauri::async_runtime::block_on(async move {
            let history = Actor::spawn(
                None,
                AgentHistoryActor,
                AgentHistoryActorArgs {
                    persistence: history_persistence,
                },
            )
            .await
            .context("failed to spawn agent history actor")?
            .0;
            let history_client = AgentHistoryClient::new(history);

            let bridge = Actor::spawn(None, AgentBridgeActor, AgentBridgeActorArgs { bridge })
                .await
                .context("failed to spawn agent bridge actor")?
                .0;
            let bridge_client = AgentBridgeClient::new(bridge);

            let proposals = Actor::spawn(
                None,
                AgentProposalActor,
                AgentProposalActorArgs {
                    runtime: runtime_for_proposals,
                    confirmation: confirmation_for_proposals,
                    history: history_client.clone(),
                },
            )
            .await
            .context("failed to spawn agent proposal actor")?
            .0;

            anyhow::Ok((history_client, bridge_client, proposals))
        })?;

        Ok(Self {
            runtime,
            confirmation,
            proposals: AgentProposalClient::new(proposals),
            history,
            bridge,
        })
    }

    pub(crate) async fn network_snapshot(&self) -> Result<AgentNetworkSnapshot, AgentCommandError> {
        let snapshot = self.runtime.snapshot().await;
        self.history.record_snapshot(snapshot.clone()).await;
        Ok(snapshot)
    }

    pub(crate) async fn history(&self) -> Result<AgentHistorySnapshot, AgentCommandError> {
        self.history.snapshot().await
    }

    pub(crate) async fn clear_history(
        &self,
        owner_label: &str,
    ) -> Result<AgentHistorySnapshot, AgentCommandError> {
        confirm_history_clear(
            self.confirmation.as_ref(),
            owner_label,
            AGENT_HISTORY_CONFIRMATION_TIMEOUT,
        )
        .await?;
        self.history.clear().await
    }

    pub(crate) async fn propose_action(
        &self,
        owner_label: &str,
        action: AgentActionRequest,
    ) -> Result<AgentProposal, AgentCommandError> {
        self.proposals.propose(owner_label, action).await
    }

    pub(crate) async fn execute_action(
        &self,
        owner_label: &str,
        proposal_id: &str,
        digest: &str,
    ) -> Result<AgentActionResult, AgentCommandError> {
        self.proposals
            .execute(owner_label, proposal_id, digest)
            .await
    }

    pub(crate) async fn cancel_action(
        &self,
        owner_label: &str,
        proposal_id: &str,
    ) -> Result<bool, AgentCommandError> {
        self.proposals.cancel(owner_label, proposal_id).await
    }

    pub(crate) async fn start_bridge(&self) -> Result<AgentBridgeStartResult, AgentCommandError> {
        self.bridge.start().await
    }

    pub(crate) async fn bridge_status(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        self.bridge.status().await
    }

    pub(crate) async fn stop_bridge(&self) -> Result<AgentBridgeStatus, AgentCommandError> {
        self.bridge.stop().await
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

    use super::{AGENT_HISTORY_TIMEOUT, AgentActorHandle, confirm_history_clear};
    use crate::features::agent::{
        model::{AgentCommandError, AgentProposal},
        ports::AgentConfirmationPort,
    };

    #[test]
    fn history_rpc_budget_covers_corrupt_recovery_and_durable_io() {
        assert!(AGENT_HISTORY_TIMEOUT > Duration::from_secs(60));
    }

    enum LifecycleMessage {
        Ping(RpcReplyPort<()>),
    }

    struct LifecycleActor;

    struct DecliningHistoryConfirmation;

    #[async_trait::async_trait]
    impl AgentConfirmationPort for DecliningHistoryConfirmation {
        async fn confirm(
            &self,
            _owner_label: &str,
            _proposal: &AgentProposal,
        ) -> Result<bool, AgentCommandError> {
            Ok(false)
        }

        async fn confirm_history_clear(
            &self,
            _owner_label: &str,
        ) -> Result<bool, AgentCommandError> {
            Ok(false)
        }
    }

    struct PendingHistoryConfirmation;

    #[async_trait::async_trait]
    impl AgentConfirmationPort for PendingHistoryConfirmation {
        async fn confirm(
            &self,
            _owner_label: &str,
            _proposal: &AgentProposal,
        ) -> Result<bool, AgentCommandError> {
            Ok(false)
        }

        async fn confirm_history_clear(
            &self,
            _owner_label: &str,
        ) -> Result<bool, AgentCommandError> {
            std::future::pending().await
        }
    }

    impl Actor for LifecycleActor {
        type Msg = LifecycleMessage;
        type State = ();
        type Arguments = ();

        async fn pre_start(
            &self,
            _myself: ActorRef<Self::Msg>,
            _args: Self::Arguments,
        ) -> Result<Self::State, ActorProcessingErr> {
            Ok(())
        }

        async fn handle(
            &self,
            _myself: ActorRef<Self::Msg>,
            message: Self::Msg,
            _state: &mut Self::State,
        ) -> Result<(), ActorProcessingErr> {
            let LifecycleMessage::Ping(reply) = message;
            let _ = reply.send(());
            Ok(())
        }
    }

    #[tokio::test]
    async fn history_clear_confirmation_decline_is_a_bounded_cancellation() {
        let result = confirm_history_clear(
            &DecliningHistoryConfirmation,
            "main",
            Duration::from_secs(1),
        )
        .await;
        assert!(matches!(
            result,
            Err(AgentCommandError::ConfirmationDeclined)
        ));
    }

    #[tokio::test]
    async fn history_clear_confirmation_timeout_fails_closed() {
        let result = confirm_history_clear(
            &PendingHistoryConfirmation,
            "main",
            Duration::from_millis(20),
        )
        .await;
        assert!(matches!(result, Err(AgentCommandError::HistoryClearFailed)));
    }

    #[tokio::test]
    async fn shared_actor_handle_stops_only_after_the_last_client_clone() {
        let actor = Actor::spawn(None, LifecycleActor, ())
            .await
            .expect("spawn lifecycle actor")
            .0;
        let handle = Arc::new(AgentActorHandle::new(actor.clone()));
        let clone = handle.clone();

        drop(handle);
        let response = actor
            .call(LifecycleMessage::Ping, Some(Duration::from_secs(1)))
            .await
            .expect("call live actor");
        assert!(matches!(response, CallResult::Success(())));

        drop(clone);
        actor
            .wait(Some(Duration::from_secs(1)))
            .await
            .expect("last client drop must stop the actor");
    }
}
