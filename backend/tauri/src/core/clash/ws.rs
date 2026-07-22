use std::{
    collections::VecDeque,
    sync::{Arc, atomic::Ordering},
};

use atomic_enum::atomic_enum;
use parking_lot::Mutex;
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

struct ClashConnectionsConnectorShared {
    state: AtomicClashConnectionsConnectorState,
    connections_tx: tokio::sync::broadcast::Sender<ClashConnectionsConnectorEvent>,
    ws_tx: tokio::sync::broadcast::Sender<ClashWsEvent>,
    info: Mutex<ClashConnectionsInfo>,
    history: Mutex<ClashWsHistory>,
    recording: Mutex<ClashWsRecording>,
}

impl ClashConnectionsConnectorShared {
    fn new() -> Self {
        Self {
            state: AtomicClashConnectionsConnectorState::new(
                ClashConnectionsConnectorState::Disconnected,
            ),
            connections_tx: tokio::sync::broadcast::channel(16).0,
            ws_tx: tokio::sync::broadcast::channel(64).0,
            info: Mutex::new(ClashConnectionsInfo::default()),
            history: Mutex::new(ClashWsHistory::default()),
            recording: Mutex::new(ClashWsRecording::default()),
        }
    }

    fn state(&self) -> ClashConnectionsConnectorState {
        self.state.load(Ordering::Acquire)
    }

    fn snapshot(&self) -> ClashWsSnapshot {
        self.history
            .lock()
            .snapshot(self.state(), self.recording.lock().clone())
    }

    fn dispatch_state_changed(&self, state: ClashConnectionsConnectorState) {
        let event_state = state.clone();
        self.state.store(state, Ordering::Release);
        let _ = self
            .connections_tx
            .send(ClashConnectionsConnectorEvent::StateChanged(
                event_state.clone(),
            ));
        let _ = self.ws_tx.send(ClashWsEvent::StateChanged(event_state));
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.connections_tx.subscribe()
    }

    fn subscribe_ws(&self) -> tokio::sync::broadcast::Receiver<ClashWsEvent> {
        self.ws_tx.subscribe()
    }

    fn set_recording(&self, kind: ClashWsKind, enabled: bool) -> ClashWsRecording {
        let recording = {
            let mut recording = self.recording.lock();
            recording.set(kind, enabled);
            recording.clone()
        };
        let _ = self
            .ws_tx
            .send(ClashWsEvent::RecordingChanged(recording.clone()));
        recording
    }

    fn clear_history(&self, kind: ClashWsKind) {
        self.history.lock().clear(kind);
        let _ = self.ws_tx.send(ClashWsEvent::HistoryCleared(kind));
    }

    fn update_connections(&self, raw: serde_json::Value) {
        let Ok(msg) = serde_json::from_value::<ClashConnectionsMessage>(raw.clone()) else {
            tracing::warn!("failed to parse clash connections message");
            return;
        };

        let mut info = self.info.lock();
        let previous_download_total =
            std::mem::replace(&mut info.download_total, msg.download_total);
        let previous_upload_total = std::mem::replace(&mut info.upload_total, msg.upload_total);
        info.download_speed = msg
            .download_total
            .checked_sub(previous_download_total)
            .unwrap_or_default();
        info.upload_speed = msg
            .upload_total
            .checked_sub(previous_upload_total)
            .unwrap_or_default();

        let _ = self
            .connections_tx
            .send(ClashConnectionsConnectorEvent::Update(*info));

        let snapshot = ClashWsConnectionSnapshot {
            download_total: info.download_total,
            upload_total: info.upload_total,
            download_speed: info.download_speed,
            upload_speed: info.upload_speed,
            memory: msg.memory,
            connections: msg.connections,
        };

        if self.recording.lock().connections {
            push_limited(
                &mut self.history.lock().connections,
                snapshot.clone(),
                MAX_CONNECTIONS_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::ConnectionsUpdated(snapshot));
    }

    fn update_log(&self, raw: serde_json::Value) {
        let Some(log) = parse_log(&raw) else {
            tracing::warn!("failed to parse clash log message");
            return;
        };
        if self.recording.lock().logs {
            push_limited(&mut self.history.lock().logs, log.clone(), MAX_LOGS_HISTORY);
        }
        let _ = self.ws_tx.send(ClashWsEvent::LogAppended(log));
    }

    fn update_traffic(&self, raw: serde_json::Value) {
        let Some(traffic) = parse_traffic(&raw) else {
            tracing::warn!("failed to parse clash traffic message");
            return;
        };
        if self.recording.lock().traffic {
            push_limited(
                &mut self.history.lock().traffic,
                traffic.clone(),
                MAX_TRAFFIC_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::TrafficUpdated(traffic));
    }

    fn update_memory(&self, raw: serde_json::Value) {
        let Some(memory) = normalize_memory(&raw) else {
            tracing::warn!("failed to parse clash memory message");
            return;
        };
        if self.recording.lock().memory {
            push_limited(
                &mut self.history.lock().memory,
                memory.clone(),
                MAX_MEMORY_HISTORY,
            );
        }
        let _ = self.ws_tx.send(ClashWsEvent::MemoryUpdated(memory));
    }

    fn update(&self, kind: ClashWsKind, raw: serde_json::Value) {
        match kind {
            ClashWsKind::Connections => self.update_connections(raw),
            ClashWsKind::Logs => self.update_log(raw),
            ClashWsKind::Traffic => self.update_traffic(raw),
            ClashWsKind::Memory => self.update_memory(raw),
        }
    }
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

    #[tokio::test]
    async fn shared_update_records_connections_and_emits_events() {
        let shared = Arc::new(ClashConnectionsConnectorShared::new());
        let mut connections_rx = shared.subscribe();
        let mut ws_rx = shared.subscribe_ws();

        shared.update(
            ClashWsKind::Connections,
            serde_json::json!({
                "downloadTotal": 100,
                "uploadTotal": 40,
                "memory": 10,
                "connections": [],
            }),
        );

        match connections_rx
            .recv()
            .await
            .expect("event should be emitted")
        {
            ClashConnectionsConnectorEvent::Update(info) => {
                assert_eq!(info.download_speed, 100);
                assert_eq!(info.upload_speed, 40);
            }
            event => panic!("unexpected event: {event:?}"),
        }

        assert!(matches!(
            ws_rx.recv().await.expect("ws event should be emitted"),
            ClashWsEvent::ConnectionsUpdated(_)
        ));
        assert_eq!(shared.snapshot().connections.len(), 1);
    }
}
