mod actions;
pub(crate) mod commands;
mod core_probe;
mod diagnostics;
#[cfg(feature = "e2e")]
mod e2e;
mod model;
mod registry;

use tauri::Manager;
use tokio::sync::Mutex;

use crate::client::ChimeraClient;

pub(crate) use actions::AgentFeatureState;
pub(crate) use diagnostics::collect_network_snapshot;
pub(crate) use model::{
    AgentActionRequest, AgentActionResult, AgentCommandError, AgentManifest, AgentNetworkSnapshot,
    AgentProposal, AgentToolError, AgentToolName, AgentToolResult,
};
pub(crate) use registry::{agent_manifest, execute_readonly_tool};

pub(crate) fn setup<R: tauri::Runtime, M: Manager<R>>(manager: &M) {
    let client = manager.state::<ChimeraClient>().inner().clone();
    manager.manage(AgentFeatureState {
        client,
        proposals: Mutex::new(Default::default()),
        execution: Mutex::new(()),
    });
}
