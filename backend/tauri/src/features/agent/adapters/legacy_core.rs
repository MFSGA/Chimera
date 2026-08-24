use chimera_ipc::api::status::CoreState;

use crate::core::clash::core::{CoreManager, RunType};

use super::super::{
    model::{AgentCoreState, AgentRunType},
    ports::{CoreLifecyclePort, CoreLifecycleStatus},
};

// TODO(actor-migration): temporary bridge to the legacy global service.
// Reason: core lifecycle is still owned by the CoreManager singleton.
// Remove when: CoreClient is injected through NyanpasuClient.
pub(crate) struct LegacyCoreLifecycle;

impl LegacyCoreLifecycle {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl CoreLifecyclePort for LegacyCoreLifecycle {
    async fn status(&self) -> CoreLifecycleStatus {
        let (state, state_changed_at, run_type) = CoreManager::global().status().await;
        CoreLifecycleStatus {
            state: map_core_state(state.as_ref()),
            run_type: map_run_type(run_type),
            state_changed_at,
        }
    }

    async fn ensure_running(&self) -> anyhow::Result<()> {
        CoreManager::global().ensure_core_running().await
    }

    async fn restart(&self) -> anyhow::Result<()> {
        CoreManager::global().run_core().await
    }
}

fn map_core_state(state: &CoreState) -> AgentCoreState {
    match state {
        CoreState::Running => AgentCoreState::Running,
        CoreState::Stopped(_) => AgentCoreState::Stopped,
    }
}

fn map_run_type(run_type: RunType) -> AgentRunType {
    match run_type {
        RunType::Normal => AgentRunType::Normal,
        RunType::Service => AgentRunType::Service,
        RunType::Elevated => AgentRunType::Elevated,
    }
}
