//! Tauri transport adapter for the ref packet.
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;
#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
pub struct ClashWsEvent(pub chimera_core::clash::ws::ClashWsPacket);
