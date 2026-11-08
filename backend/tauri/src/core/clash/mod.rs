use backon::ExponentialBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};
use tauri_specta::Event;

/// 2
pub mod api;
pub mod connection_details;
/// 1
pub mod core;
/// 3
pub mod proxies;
pub(crate) mod transaction;
pub mod ws;

pub static CLASH_API_DEFAULT_BACKOFF_STRATEGY: Lazy<ExponentialBuilder> = Lazy::new(|| {
    ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(50))
        .with_max_delay(std::time::Duration::from_secs(5))
        .with_max_times(5)
});

#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct ClashConnectionsEvent(pub chimera_core::clash::ws::ClashConnectionsConnectorEvent);

pub async fn restart_ws_connector<R: Runtime>(manager: &impl Manager<R>) -> anyhow::Result<()> {
    let client = manager
        .try_state::<crate::client::ChimeraClient>()
        .ok_or_else(|| anyhow::anyhow!("ChimeraClient is not managed"))?
        .inner()
        .clone();
    client.streams().restart().await
}

pub fn setup<R: Runtime, M: Manager<R>>(manager: &M) -> anyhow::Result<()> {
    let client = manager
        .state::<crate::client::ChimeraClient>()
        .inner()
        .clone();
    let mut proxy_changes = client.subscribe_proxy_changes();
    tauri::async_runtime::spawn(async move {
        while proxy_changes.changed().await.is_ok() {
            crate::core::handle::Handle::mutate_proxies();
        }
    });
    let streams = client.streams().clone();
    if let Err(error) = client.start_traffic(streams.subscribe_connection_frames()) {
        tracing::error!(%error, "failed to start optional traffic recording");
    }
    manager.manage(connection_details::ConnectionDetailSubscriptions::new());
    let app_handle = manager.app_handle().clone();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let mut connections = streams.subscribe();
        let mut events = streams.subscribe_ws();
        if let Err(error) = streams.start().await {
            tracing::error!(%error, "Clash streams actor failed to start");
        }

        let ws_app_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(event) = events.recv().await {
                if let Err(error) = ws::ClashWsEvent::from(event).emit(&ws_app_handle) {
                    tracing::debug!(%error, "failed to emit Clash WebSocket event");
                }
            }
        });

        while let Ok(event) = connections.recv().await {
            if let Err(error) = ClashConnectionsEvent(event).emit(&app_handle) {
                tracing::debug!(%error, "failed to emit Clash connections event");
            }
        }
    });
    Ok(())
}
