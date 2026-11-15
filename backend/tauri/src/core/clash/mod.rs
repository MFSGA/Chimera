use backon::ExponentialBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};
use tauri_specta::Event;

use crate::utils::task::spawn_tracked_on_tauri_runtime;

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
    let owners = manager.state::<crate::setup::AppShutdown>().inner().clone();
    let shutdown = owners.shutdown.child_token();
    let tasks = owners.tasks.clone();
    let mut proxy_changes = client.subscribe_proxy_changes();
    let proxy_shutdown = shutdown.clone();
    spawn_tracked_on_tauri_runtime(&tasks, async move {
        loop {
            let changed = tokio::select! {
                _ = proxy_shutdown.cancelled() => break,
                changed = proxy_changes.changed() => changed,
            };
            if changed.is_err() {
                break;
            }
            crate::core::handle::Handle::mutate_proxies();
        }
    });
    let streams = client.streams().clone();
    if let Err(error) = client.start_traffic(streams.subscribe_connection_frames()) {
        tracing::error!(%error, "failed to start optional traffic recording");
    }
    manager.manage(connection_details::ConnectionDetailSubscriptions::new());
    let app_handle = manager.app_handle().clone();
    let stream_shutdown = shutdown.clone();
    let stream_tasks = tasks.clone();

    spawn_tracked_on_tauri_runtime(&tasks, async move {
        let mut connections = streams.subscribe();
        let mut events = streams.subscribe_ws();
        if stream_shutdown.is_cancelled() {
            return;
        }
        if let Err(error) = streams.start().await {
            tracing::error!(%error, "Clash streams actor failed to start");
        }
        if stream_shutdown.is_cancelled() {
            return;
        }

        let ws_app_handle = app_handle.clone();
        let ws_shutdown = stream_shutdown.clone();
        spawn_tracked_on_tauri_runtime(&stream_tasks, async move {
            loop {
                let event = tokio::select! {
                    _ = ws_shutdown.cancelled() => break,
                    event = events.recv() => event,
                };
                let Ok(event) = event else {
                    break;
                };
                if let Err(error) = ws::ClashWsEvent::from(event).emit(&ws_app_handle) {
                    tracing::debug!(%error, "failed to emit Clash WebSocket event");
                }
            }
        });

        loop {
            let event = tokio::select! {
                _ = stream_shutdown.cancelled() => break,
                event = connections.recv() => event,
            };
            let Ok(event) = event else {
                break;
            };
            if let Err(error) = ClashConnectionsEvent(event).emit(&app_handle) {
                tracing::debug!(%error, "failed to emit Clash connections event");
            }
        }
    });
    Ok(())
}
