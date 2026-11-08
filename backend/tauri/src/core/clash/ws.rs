//! Tauri event adapter for the core-owned Clash streams actor.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use chimera_core::clash::ws::{
    ClashConnectionsConnectorState, ClashWsConnectionSnapshot, ClashWsKind, ClashWsLog,
    ClashWsMemory, ClashWsRecording, ClashWsTraffic,
};

#[derive(Debug, Clone, Type, Serialize, Deserialize, Event)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashWsEvent {
    StateChanged(ClashConnectionsConnectorState),
    ConnectionsUpdated(ClashWsConnectionSnapshot),
    LogAppended(ClashWsLog),
    TrafficUpdated(ClashWsTraffic),
    MemoryUpdated(ClashWsMemory),
    RecordingChanged(ClashWsRecording),
    HistoryCleared(ClashWsKind),
}

impl From<chimera_core::clash::ws::ClashWsEvent> for ClashWsEvent {
    fn from(event: chimera_core::clash::ws::ClashWsEvent) -> Self {
        match event {
            chimera_core::clash::ws::ClashWsEvent::StateChanged(state) => Self::StateChanged(state),
            chimera_core::clash::ws::ClashWsEvent::ConnectionsUpdated(snapshot) => {
                Self::ConnectionsUpdated(snapshot)
            }
            chimera_core::clash::ws::ClashWsEvent::LogAppended(log) => Self::LogAppended(log),
            chimera_core::clash::ws::ClashWsEvent::TrafficUpdated(traffic) => {
                Self::TrafficUpdated(traffic)
            }
            chimera_core::clash::ws::ClashWsEvent::MemoryUpdated(memory) => {
                Self::MemoryUpdated(memory)
            }
            chimera_core::clash::ws::ClashWsEvent::RecordingChanged(recording) => {
                Self::RecordingChanged(recording)
            }
            chimera_core::clash::ws::ClashWsEvent::HistoryCleared(kind) => {
                Self::HistoryCleared(kind)
            }
        }
    }
}
