//! Source-domain changes admitted to the application runtime workflow.
//!
//! This Profile slice follows ref's `DomainChange` / `MutationDomain` contract.
//! Chimera's application and clash writers remain on their current lifecycle
//! path until their migration slices move onto the shared participant.

use std::sync::Arc;

use chimera_config::profile::Profiles;
use chimera_core::state::{Ack, DecisionHandle, StateChange};
use chimera_core_manager::OperationId;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub(crate) enum DomainChange {
    Profiles {
        previous: Option<Arc<Profiles>>,
        candidate: Arc<Profiles>,
    },
}

pub(crate) trait MutationDomain: Clone + Send + Sync + 'static {
    fn domain_change(change: StateChange<Self>) -> DomainChange;
}

impl MutationDomain for Profiles {
    fn domain_change(change: StateChange<Self>) -> DomainChange {
        DomainChange::Profiles {
            previous: change
                .previous
                .map(|previous| Arc::new(previous.state.clone())),
            candidate: change.current,
        }
    }
}

/// One Profile source mutation admitted to the tracked application workflow.
///
/// The source transaction owns the authoritative decision. The workflow owns
/// the runtime attempt and answers prepare before it waits for that decision.
pub(crate) struct MutationRequest {
    pub(crate) operation_id: OperationId,
    pub(crate) change: DomainChange,
    pub(crate) hints: super::impact::MutationHints,
    pub(crate) class: super::policy::CommandClass,
    pub(crate) decision: DecisionHandle,
    pub(crate) ack: Option<oneshot::Sender<TryAck>>,
    pub(crate) completion: Option<oneshot::Sender<Result<(), String>>>,
}

impl MutationRequest {
    pub(crate) fn answer(&mut self, ack: TryAck) {
        if let Some(channel) = self.ack.take() {
            let _ = channel.send(ack);
        }
    }

    pub(crate) fn settle(&mut self, result: Result<(), String>) {
        if let Some(channel) = self.completion.take() {
            let _ = channel.send(result);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TryAck {
    Ok,
    Degraded(String),
    Rejected(String),
    Failed(String),
}

impl From<TryAck> for Ack {
    fn from(ack: TryAck) -> Self {
        match ack {
            TryAck::Ok => Ack::Ok,
            TryAck::Degraded(message) => Ack::Degraded(message),
            TryAck::Rejected(message) => Ack::Rejected(message),
            TryAck::Failed(message) => Ack::Failed(anyhow::anyhow!(message)),
        }
    }
}
