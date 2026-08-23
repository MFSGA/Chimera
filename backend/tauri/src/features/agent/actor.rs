use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use tokio::{sync::Semaphore, task::AbortHandle};

use super::{
    actions::AgentProposalState,
    bridge::{AgentBridgeStartResult, AgentBridgeStatus},
    client::AgentHistoryClient,
    history::{AgentAuditOutcome, AgentHistorySnapshot, AgentHistoryStore},
    model::{
        AgentActionRequest, AgentActionResult, AgentCommandError, AgentNetworkSnapshot,
        AgentProposal,
    },
    ports::{
        AgentBridgePort, AgentConfirmationPort, AgentHistoryPersistencePort, AgentRuntimePort,
    },
};

pub(crate) enum AgentProposalMessage {
    Propose {
        owner_label: String,
        action: AgentActionRequest,
        reply: RpcReplyPort<Result<AgentProposal, AgentCommandError>>,
    },
    Execute {
        owner_label: String,
        proposal_id: String,
        digest: String,
        reply: RpcReplyPort<Result<AgentActionResult, AgentCommandError>>,
    },
    Cancel {
        owner_label: String,
        proposal_id: String,
        reply: RpcReplyPort<bool>,
    },
}

pub(crate) struct AgentProposalActorArgs {
    pub(crate) runtime: Arc<dyn AgentRuntimePort>,
    pub(crate) confirmation: Arc<dyn AgentConfirmationPort>,
    pub(crate) history: AgentHistoryClient,
}

pub(crate) struct AgentProposalActorState {
    proposals: AgentProposalState,
    runtime: Arc<dyn AgentRuntimePort>,
    confirmation: Arc<dyn AgentConfirmationPort>,
    execution_gate: Arc<Semaphore>,
    background_tasks: Vec<AbortHandle>,
}

pub(crate) struct AgentProposalActor;

impl Actor for AgentProposalActor {
    type Msg = AgentProposalMessage;
    type State = AgentProposalActorState;
    type Arguments = AgentProposalActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(AgentProposalActorState {
            proposals: AgentProposalState::new(args.history),
            runtime: args.runtime,
            confirmation: args.confirmation,
            execution_gate: Arc::new(Semaphore::new(1)),
            background_tasks: Vec::new(),
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            AgentProposalMessage::Propose {
                owner_label,
                action,
                reply,
            } => match state
                .proposals
                .propose(state.runtime.as_ref(), &owner_label, action)
                .await
            {
                Ok(proposal) => {
                    state.background_tasks.retain(|task| !task.is_finished());
                    let history = state.proposals.history_client();
                    let response_task = tokio::spawn(async move {
                        let proposal = record_proposed_audit(history, proposal).await;
                        let _ = reply.send(Ok(proposal));
                    });
                    state.background_tasks.push(response_task.abort_handle());
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            AgentProposalMessage::Execute {
                owner_label,
                proposal_id,
                digest,
                reply,
            } => match state.proposals.begin_execute(
                state.runtime.clone(),
                state.confirmation.clone(),
                state.execution_gate.clone(),
                owner_label,
                proposal_id,
                digest,
            ) {
                Ok(execution) => {
                    state.background_tasks.retain(|task| !task.is_finished());
                    let execution_task = tokio::spawn(execution);
                    state.background_tasks.push(execution_task.abort_handle());
                    let response_task = tokio::spawn(async move {
                        let result = execution_task
                            .await
                            .unwrap_or(Err(AgentCommandError::ActionFailed));
                        let _ = reply.send(result);
                    });
                    state.background_tasks.push(response_task.abort_handle());
                }
                Err(error) => {
                    let _ = reply.send(Err(error));
                }
            },
            AgentProposalMessage::Cancel {
                owner_label,
                proposal_id,
                reply,
            } => {
                let _ = reply.send(state.proposals.cancel(&owner_label, &proposal_id));
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        for task in state.background_tasks.drain(..) {
            task.abort();
        }
        Ok(())
    }
}

pub(crate) async fn record_proposed_audit(
    history: AgentHistoryClient,
    proposal: AgentProposal,
) -> AgentProposal {
    history
        .record_audit(
            &proposal.id,
            &proposal.action,
            &proposal.snapshot_revision,
            AgentAuditOutcome::Proposed,
        )
        .await;
    proposal
}

pub(crate) enum AgentHistoryMessage {
    RecordSnapshot {
        snapshot: Box<AgentNetworkSnapshot>,
        reply: RpcReplyPort<()>,
    },
    RecordAudit {
        proposal_id: String,
        action: AgentActionRequest,
        snapshot_revision: String,
        outcome: AgentAuditOutcome,
        reply: RpcReplyPort<()>,
    },
    Snapshot(RpcReplyPort<AgentHistorySnapshot>),
    Clear(RpcReplyPort<Result<AgentHistorySnapshot, AgentCommandError>>),
}

pub(crate) struct AgentHistoryActorArgs {
    pub(crate) persistence: Arc<dyn AgentHistoryPersistencePort>,
}

pub(crate) struct AgentHistoryActor;

impl Actor for AgentHistoryActor {
    type Msg = AgentHistoryMessage;
    type State = AgentHistoryStore;
    type Arguments = AgentHistoryActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(AgentHistoryStore::new(args.persistence))
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            AgentHistoryMessage::RecordSnapshot { snapshot, reply } => {
                state.record_snapshot(snapshot.as_ref()).await;
                let _ = reply.send(());
            }
            AgentHistoryMessage::RecordAudit {
                proposal_id,
                action,
                snapshot_revision,
                outcome,
                reply,
            } => {
                state
                    .record_audit(&proposal_id, &action, &snapshot_revision, outcome)
                    .await;
                let _ = reply.send(());
            }
            AgentHistoryMessage::Snapshot(reply) => {
                let _ = reply.send(state.snapshot().await);
            }
            AgentHistoryMessage::Clear(reply) => {
                let _ = reply.send(state.clear().await);
            }
        }
        Ok(())
    }
}

pub(crate) enum AgentBridgeMessage {
    Start(RpcReplyPort<Result<AgentBridgeStartResult, AgentCommandError>>),
    Status(RpcReplyPort<AgentBridgeStatus>),
    Stop(RpcReplyPort<AgentBridgeStatus>),
}

pub(crate) struct AgentBridgeActorArgs {
    pub(crate) bridge: Box<dyn AgentBridgePort>,
}

pub(crate) struct AgentBridgeActor;

impl Actor for AgentBridgeActor {
    type Msg = AgentBridgeMessage;
    type State = Box<dyn AgentBridgePort>;
    type Arguments = AgentBridgeActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(args.bridge)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            AgentBridgeMessage::Start(reply) => {
                let _ = reply.send(state.start().await);
            }
            AgentBridgeMessage::Status(reply) => {
                let _ = reply.send(state.status().await);
            }
            AgentBridgeMessage::Stop(reply) => {
                let _ = reply.send(state.stop().await);
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop().await;
        Ok(())
    }
}
