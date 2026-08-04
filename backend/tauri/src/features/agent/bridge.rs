use serde::Serialize;
use specta::Type;

#[derive(Clone, Serialize, Type)]
pub struct AgentBridgeStartResult {
    pub running: bool,
    pub base_url: String,
    pub token: Option<String>,
}

impl AgentBridgeStartResult {
    pub(crate) fn started(base_url: String, token: String) -> Self {
        Self {
            running: true,
            base_url,
            token: Some(token),
        }
    }

    pub(crate) fn already_running(base_url: String) -> Self {
        Self {
            running: true,
            base_url,
            token: None,
        }
    }
}

#[derive(Clone, Serialize, Type)]
pub struct AgentBridgeStatus {
    pub running: bool,
    pub base_url: Option<String>,
}
