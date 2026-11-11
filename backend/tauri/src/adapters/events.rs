use chimera_core::client::app_update::AppUpdateSnapshot;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AppUpdateStateChanged(pub AppUpdateSnapshot);
