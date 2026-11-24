use chimera_ipc::api::status::CoreState;
use serde::{Deserialize, Serialize};
use specta::Type;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    /// Run as child process directly
    Normal,
    /// Run by Nyanpasu Service via a ipc call
    Service,
    // TODO: Not implemented yet
    /// Run as elevated process, if profile advice to run as elevated
    Elevated,
}

impl RunType {
    pub fn classify(enable_service: bool, connected: bool) -> Self {
        if enable_service && connected {
            Self::Service
        } else {
            Self::Normal
        }
    }
}
#[derive(Debug, Clone)]
pub struct CoreStatusSnapshot {
    pub state: CoreState,
    pub state_changed_at: i64,
    pub run_type: RunType,
    pub instance_id: Option<String>,
}
