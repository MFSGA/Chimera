use backon::ExponentialBuilder;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Manager, Runtime};
use tauri_specta::Event;

/// 2
pub mod api;
/// 1
pub mod core;
/// 3
pub mod proxies;
#[allow(dead_code)]
pub(crate) mod rebuild;
pub(crate) mod runtime_product;
pub(crate) mod system_dns;
pub(crate) mod transaction;
pub mod ws;

pub(crate) mod client;

pub static CLASH_API_DEFAULT_BACKOFF_STRATEGY: Lazy<ExponentialBuilder> = Lazy::new(|| {
    ExponentialBuilder::default()
        .with_min_delay(std::time::Duration::from_millis(50))
        .with_max_delay(std::time::Duration::from_secs(5))
        .with_max_times(5)
});

#[derive(Serialize, Deserialize, Debug, Clone, Type, Event)]
pub struct ClashConnectionsEvent(pub ws::ClashConnectionsConnectorEvent);

pub async fn restart_ws_connector<R: Runtime>(manager: &impl Manager<R>) -> anyhow::Result<()> {
    let connector = manager
        .try_state::<ws::ClashConnectionsConnector>()
        .ok_or_else(|| anyhow::anyhow!("clash websocket connector is not managed"))?
        .inner()
        .clone();
    connector.restart().await
}

pub fn setup<R: Runtime, M: Manager<R>>(manager: &M) -> anyhow::Result<()> {
    manager.manage(client::NyanpasuClient::legacy());

    let ws_connector = ws::ClashConnectionsConnector::new();
    manager.manage(ws_connector.clone());
    let app_handle = manager.app_handle().clone();

    tauri::async_runtime::spawn(async move {
        // TODO: refactor it while clash core manager use tauri event dispatcher to notify the core state changed
        {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;

            // TODO: clash-rs ws authorization is not working
            match ws_connector.start().await {
                Ok(_) => {
                    tracing::info!(
                        "ws_connector started successfully clash-rs may be errored here."
                    );
                }
                // TODO: wait for clash-rs to fix
                Err(e) => {
                    tracing::error!("ws_connector failed to start: {:?}", e);
                }
            }
        }
        let mut rx = ws_connector.subscribe();
        let mut ws_rx = ws_connector.subscribe_ws();
        let ws_app_handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(event) = ws_rx.recv().await {
                event.emit(&ws_app_handle).unwrap();
            }
        });
        while let Ok(event) = rx.recv().await {
            ClashConnectionsEvent(event).emit(&app_handle).unwrap();
        }
    });
    Ok(())
}
