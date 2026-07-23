mod actions;
pub(crate) mod commands;
mod core_probe;
mod diagnostics;
mod model;

use tauri::Manager;
use tokio::sync::Mutex;

pub(crate) use actions::AgentFeatureState;
pub(crate) use diagnostics::collect_network_snapshot;
pub(crate) use model::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentNetworkSnapshot, AgentProposal,
};

pub(crate) fn setup<R: tauri::Runtime, M: Manager<R>>(manager: &M) {
    manager.manage(AgentFeatureState {
        proposals: Mutex::new(Default::default()),
        execution: Mutex::new(()),
    });
}
