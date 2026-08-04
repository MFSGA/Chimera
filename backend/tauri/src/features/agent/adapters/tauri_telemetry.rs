use tauri::{AppHandle, Manager};

use crate::core::clash::{
    restart_ws_connector,
    ws::{ClashConnectionsConnector, ClashConnectionsConnectorState},
};

use super::super::{
    model::{AgentConnectorState, AgentTelemetrySnapshot},
    ports::AgentTelemetryPort,
};

pub(crate) struct TauriAgentTelemetry {
    app: AppHandle,
}

impl TauriAgentTelemetry {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl AgentTelemetryPort for TauriAgentTelemetry {
    fn snapshot(&self) -> Option<AgentTelemetrySnapshot> {
        let connector = self.app.try_state::<ClashConnectionsConnector>()?;
        let snapshot = connector.snapshot();
        let latest = snapshot.connections.last();
        Some(AgentTelemetrySnapshot {
            state: match snapshot.state {
                ClashConnectionsConnectorState::Disconnected => AgentConnectorState::Disconnected,
                ClashConnectionsConnectorState::Connecting => AgentConnectorState::Connecting,
                ClashConnectionsConnectorState::Connected => AgentConnectorState::Connected,
            },
            active_connection_count: latest
                .and_then(|sample| sample.connections.as_ref())
                .map(|connections| connections.len() as u32),
            upload_speed: latest.map(|sample| sample.upload_speed),
            download_speed: latest.map(|sample| sample.download_speed),
            upload_total: latest.map(|sample| sample.upload_total),
            download_total: latest.map(|sample| sample.download_total),
            recent_error_count: snapshot
                .logs
                .iter()
                .filter(|entry| {
                    matches!(
                        entry.log_type.to_ascii_lowercase().as_str(),
                        "error" | "fatal"
                    )
                })
                .count() as u32,
        })
    }

    async fn reconnect(&self) -> anyhow::Result<()> {
        restart_ws_connector(&self.app).await
    }
}
