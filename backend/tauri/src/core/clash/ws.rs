use std::collections::VecDeque;

use atomic_enum::atomic_enum;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

const MAX_CONNECTIONS_HISTORY: usize = 32;
const MAX_MEMORY_HISTORY: usize = 32;
const MAX_TRAFFIC_HISTORY: usize = 32;
const MAX_LOGS_HISTORY: usize = 1024;
const MAX_REASONABLE_MEMORY_BYTES: u64 = 16 * 1024_u64.pow(4);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClashConnectionsMessage {
    download_total: u64,
    upload_total: u64,
    memory: Option<u64>,
    connections: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Default, Copy, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashConnectionsInfo {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsConnectionSnapshot {
    pub download_total: u64,
    pub upload_total: u64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub memory: Option<u64>,
    // TODO: specta 2.0.0-rc.25 cannot export recursive inline types (serde_json::Value expands
    // infinitely via Vec<Value>). Replace with a concrete ClashConnection struct once the specta
    // bug is fixed or a proper named recursive JsonValue type is available.
    #[specta(type = Option<specta_typescript::Any>)]
    pub connections: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
pub enum ClashConnectionsConnectorState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClashWsKind {
    Connections,
    Logs,
    Traffic,
    Memory,
}

impl ClashWsKind {
    fn path(self) -> &'static str {
        match self {
            Self::Connections => "connections",
            Self::Logs => "logs",
            Self::Traffic => "traffic",
            Self::Memory => "memory",
        }
    }
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
pub struct ClashWsRecording {
    pub connections: bool,
    pub logs: bool,
    pub traffic: bool,
    pub memory: bool,
}

impl Default for ClashWsRecording {
    fn default() -> Self {
        Self {
            connections: true,
            logs: true,
            traffic: true,
            memory: true,
        }
    }
}

impl ClashWsRecording {
    fn set(&mut self, kind: ClashWsKind, enabled: bool) {
        match kind {
            ClashWsKind::Connections => self.connections = enabled,
            ClashWsKind::Logs => self.logs = enabled,
            ClashWsKind::Traffic => self.traffic = enabled,
            ClashWsKind::Memory => self.memory = enabled,
        }
    }
}

#[derive(Debug, Clone, Default, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsMemory {
    pub inuse: u64,
    pub oslimit: u64,
}

#[derive(Debug, Clone, Default, Type, Serialize, Deserialize)]
pub struct ClashWsTraffic {
    pub up: u64,
    pub down: u64,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
pub struct ClashWsLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub time: Option<String>,
    pub payload: String,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsSnapshot {
    pub state: ClashConnectionsConnectorState,
    pub recording: ClashWsRecording,
    pub connections: Vec<ClashWsConnectionSnapshot>,
    pub logs: Vec<ClashWsLog>,
    pub traffic: Vec<ClashWsTraffic>,
    pub memory: Vec<ClashWsMemory>,
}

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

#[derive(Default)]
struct ClashWsHistory {
    connections: VecDeque<ClashWsConnectionSnapshot>,
    logs: VecDeque<ClashWsLog>,
    traffic: VecDeque<ClashWsTraffic>,
    memory: VecDeque<ClashWsMemory>,
}

impl ClashWsHistory {
    fn clear(&mut self, kind: ClashWsKind) {
        match kind {
            ClashWsKind::Connections => self.connections.clear(),
            ClashWsKind::Logs => self.logs.clear(),
            ClashWsKind::Traffic => self.traffic.clear(),
            ClashWsKind::Memory => self.memory.clear(),
        }
    }

    fn snapshot(
        &self,
        state: ClashConnectionsConnectorState,
        recording: ClashWsRecording,
    ) -> ClashWsSnapshot {
        ClashWsSnapshot {
            state,
            recording,
            connections: self.connections.iter().cloned().collect(),
            logs: self.logs.iter().cloned().collect(),
            traffic: self.traffic.iter().cloned().collect(),
            memory: self.memory.iter().cloned().collect(),
        }
    }
}

fn push_limited<T>(items: &mut VecDeque<T>, item: T, limit: usize) {
    items.push_back(item);
    while items.len() > limit {
        items.pop_front();
    }
}

fn value_to_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value {
        Some(serde_json::Value::Number(number)) => number.as_u64(),
        Some(serde_json::Value::String(value)) => value.parse().ok(),
        _ => None,
    }
}

fn normalize_memory(raw: &serde_json::Value) -> Option<ClashWsMemory> {
    let object = raw.as_object()?;
    let mut inuse = value_to_u64(object.get("inuse"))?;
    let oslimit = value_to_u64(object.get("oslimit")).unwrap_or_default();

    if oslimit > 0 && inuse > oslimit.saturating_mul(2) {
        if inuse / 8 <= oslimit.saturating_mul(2) {
            inuse /= 8;
        }

        while inuse > oslimit.saturating_mul(2) && inuse % 1024 == 0 {
            inuse /= 1024;
        }

        if inuse > oslimit.saturating_mul(2) {
            inuse = oslimit;
        }
    } else if oslimit == 0 && inuse > MAX_REASONABLE_MEMORY_BYTES {
        return None;
    }

    Some(ClashWsMemory { inuse, oslimit })
}

fn parse_traffic(raw: &serde_json::Value) -> Option<ClashWsTraffic> {
    let object = raw.as_object()?;
    Some(ClashWsTraffic {
        up: value_to_u64(object.get("up"))?,
        down: value_to_u64(object.get("down"))?,
    })
}

fn parse_log(raw: &serde_json::Value) -> Option<ClashWsLog> {
    let object = raw.as_object()?;
    Some(ClashWsLog {
        log_type: object.get("type")?.as_str()?.to_string(),
        time: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
        payload: object.get("payload")?.as_str()?.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_memory_clamps_obvious_unit_mismatch() {
        let memory = normalize_memory(&serde_json::json!({
            "inuse": 8000,
            "oslimit": 1000,
        }))
        .expect("memory should parse");

        assert_eq!(memory.inuse, 1000);
        assert_eq!(memory.oslimit, 1000);
    }

    #[test]
    fn push_limited_keeps_latest_items() {
        let mut items = VecDeque::new();
        push_limited(&mut items, 1, 2);
        push_limited(&mut items, 2, 2);
        push_limited(&mut items, 3, 2);
        assert_eq!(items.into_iter().collect::<Vec<_>>(), vec![2, 3]);
    }
}
