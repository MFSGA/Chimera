//! Actor-owned Clash WebSocket subscriptions.
//!
//! The actor owns stream workers, history, connection-rate state and the
//! published snapshots. The host supplies only an instance-bound API client;
//! Tauri owns event delivery and native Channel adapters.

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use chimera_config::clash::config::overrides::LogLevel;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::{
    sync::{broadcast, watch},
    task::{JoinHandle, JoinSet},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    actor_v2::api::{ApiClient, ApiError, connection_snapshot_value},
    clash::connection_rates::{ClashConnectionDetails, DetailCounters, derive_connection_details},
};

const MAX_CONNECTIONS_HISTORY: usize = 32;
const MAX_MEMORY_HISTORY: usize = 32;
const MAX_TRAFFIC_HISTORY: usize = 32;
const MAX_LOGS_HISTORY: usize = 1024;
const MAX_REASONABLE_MEMORY_BYTES: u64 = 16 * 1024_u64.pow(4);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to spawn streams actor")]
    SpawnActor {
        #[source]
        source: ractor::SpawnErr,
    },
}

#[async_trait::async_trait]
pub trait ClashApiClientSource: Send + Sync + 'static {
    async fn api_client(&self) -> Result<ApiClient>;
}

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
    #[specta(type = Option<specta_typescript::Any>)]
    pub connections: Option<Vec<serde_json::Value>>,
}

/// Internal accounting frame consumed by the traffic recorder.
#[derive(Debug, Clone)]
pub struct ClashConnectionsFrame {
    /// Changes when the stream connector is explicitly restarted.
    pub instance_id: String,
    /// Controller process identity that owned the API capability.
    pub core_instance_id: String,
    pub snapshot: ClashWsConnectionSnapshot,
}

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[serde(tag = "kind", content = "data")]
pub enum ClashConnectionsConnectorEvent {
    StateChanged(ClashConnectionsConnectorState),
    Update(ClashConnectionsInfo),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClashConnectionsConnectorState {
    #[default]
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
    fn index(self) -> usize {
        match self {
            Self::Connections => 0,
            Self::Logs => 1,
            Self::Traffic => 2,
            Self::Memory => 3,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Type, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClashWsMemory {
    pub inuse: u64,
    pub oslimit: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Type, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Type, Serialize, Deserialize)]
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

fn normalize_memory(sample: clash_api::Memory) -> Option<ClashWsMemory> {
    let mut inuse = sample.in_use;
    let oslimit = sample.os_limit;

    if oslimit > 0 && inuse > oslimit.saturating_mul(2) {
        if inuse / 8 <= oslimit.saturating_mul(2) {
            inuse /= 8;
        }

        while inuse > oslimit.saturating_mul(2) && inuse.is_multiple_of(1024) {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stream {
    Connections,
    Logs(clash_api::LogLevel),
    Traffic,
    Memory,
}

enum Sample {
    Connections(clash_api::ConnectionsSnapshot),
    Log(clash_api::LogEntry),
    Traffic(clash_api::Traffic),
    Memory(clash_api::Memory),
}

enum Delivery {
    Bind(ApiClient),
    State(ApiClient, u64, ClashConnectionsConnectorState),
    Sample(ApiClient, u64, Sample),
    Invalidated,
}

#[derive(Debug, Clone, Copy)]
enum DeliveryResult {
    Rejected,
    Accepted,
    Bound([u64; 4]),
}

enum Message {
    Start(RpcReplyPort<()>),
    Stop(RpcReplyPort<()>),
    Restart(RpcReplyPort<Result<()>>),
    SubscribeDetails(RpcReplyPort<watch::Receiver<Option<Arc<ClashConnectionDetails>>>>),
    DetailsClosed,
    Recording(ClashWsKind, bool, RpcReplyPort<ClashWsRecording>),
    Clear(ClashWsKind, RpcReplyPort<Result<()>>),
    Deliver(Box<Delivery>, RpcReplyPort<DeliveryResult>),
    SetLogLevel(LogLevel, RpcReplyPort<()>),
}

struct Args {
    source: Arc<dyn ClashApiClientSource>,
    log_level: LogLevel,
    shutdown: CancellationToken,
    connections: broadcast::Sender<ClashConnectionsConnectorEvent>,
    events: broadcast::Sender<ClashWsEvent>,
    details: watch::Sender<Option<Arc<ClashConnectionDetails>>>,
    frames: watch::Sender<Option<Arc<ClashConnectionsFrame>>>,
    snapshot: watch::Sender<ClashWsSnapshot>,
}

struct StreamsActor;

struct State {
    args: Args,
    task: Option<JoinHandle<()>>,
    logs: Option<JoinHandle<()>>,
    api: Option<ApiClient>,
    status: ClashConnectionsConnectorState,
    sequence: u64,
    history: ClashWsHistory,
    recording: ClashWsRecording,
    previous_download_total: u64,
    previous_upload_total: u64,
    detail_counters: DetailCounters,
    details_closed: Option<JoinHandle<()>>,
    capture_id: String,
    stream_epochs: [u64; 4],
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(task) = self.details_closed.take() {
            task.abort();
        }
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(task) = self.logs.take() {
            task.abort();
        }
    }
}

impl State {
    fn snapshot(&self) -> ClashWsSnapshot {
        self.history.snapshot(self.status, self.recording.clone())
    }

    fn publish_snapshot(&self) {
        self.args.snapshot.send_replace(self.snapshot());
    }

    fn emit(&mut self, event: ClashWsEvent) {
        self.sequence = self.sequence.wrapping_add(1);
        let _ = self.args.events.send(event);
    }

    fn status(&mut self, status: ClashConnectionsConnectorState) {
        if self.status == status {
            return;
        }
        self.status = status;
        if status != ClashConnectionsConnectorState::Connected {
            self.previous_download_total = 0;
            self.previous_upload_total = 0;
            self.detail_counters = DetailCounters::default();
            let _ = self.args.details.send_replace(None);
            let _ = self.args.frames.send_replace(None);
        }
        let _ = self
            .args
            .connections
            .send(ClashConnectionsConnectorEvent::StateChanged(status));
        self.emit(ClashWsEvent::StateChanged(status));
    }

    async fn reset(&mut self) {
        if let Some(task) = self.logs.take() {
            task.abort();
            let _ = task.await;
        }
        self.api = None;
        self.history = ClashWsHistory::default();
        self.previous_download_total = 0;
        self.previous_upload_total = 0;
        self.detail_counters = DetailCounters::default();
        self.capture_id = uuid::Uuid::new_v4().to_string();
        for epoch in &mut self.stream_epochs {
            *epoch = epoch.wrapping_add(1);
        }
        let _ = self.args.details.send_replace(None);
        let _ = self.args.frames.send_replace(None);
        self.status(ClashConnectionsConnectorState::Disconnected);
        let _ = self
            .args
            .connections
            .send(ClashConnectionsConnectorEvent::Update(
                ClashConnectionsInfo::default(),
            ));
        for kind in [
            ClashWsKind::Connections,
            ClashWsKind::Logs,
            ClashWsKind::Traffic,
            ClashWsKind::Memory,
        ] {
            self.emit(ClashWsEvent::HistoryCleared(kind));
        }
        self.publish_snapshot();
    }

    fn accepts(&self, api: &ApiClient) -> bool {
        self.api
            .as_ref()
            .is_some_and(|current| current.same_instance(api))
    }

    fn restart_logs(&mut self, actor: &ActorRef<Message>) {
        let epoch = &mut self.stream_epochs[ClashWsKind::Logs.index()];
        *epoch = epoch.wrapping_add(1);
        if let Some(task) = self.logs.take() {
            task.abort();
        }
        if let Some(api) = self.api.clone()
            && self.args.log_level != LogLevel::Silent
        {
            let stream = Stream::Logs(subscription_level(self.args.log_level));
            self.logs = Some(tokio::spawn(run_stream(actor.clone(), api, stream, *epoch)));
        }
    }

    async fn stop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(task) = self.logs.take() {
            task.abort();
            let _ = task.await;
        }
        self.reset().await;
    }

    fn update(&mut self, sample: Sample) {
        match sample {
            Sample::Connections(sample) => {
                let value = match connection_snapshot_value(sample) {
                    Ok(value) => value,
                    Err(error) => {
                        tracing::warn!(%error, "failed to serialize Clash connections sample");
                        return;
                    }
                };
                let msg = match serde_json::from_value::<ClashConnectionsMessage>(value) {
                    Ok(msg) => msg,
                    Err(error) => {
                        tracing::warn!(%error, "failed to parse Clash connections sample");
                        return;
                    }
                };
                let download_speed = msg
                    .download_total
                    .saturating_sub(self.previous_download_total);
                let upload_speed = msg.upload_total.saturating_sub(self.previous_upload_total);
                self.previous_download_total = msg.download_total;
                self.previous_upload_total = msg.upload_total;

                let info = ClashConnectionsInfo {
                    download_total: msg.download_total,
                    upload_total: msg.upload_total,
                    download_speed,
                    upload_speed,
                };
                let _ = self
                    .args
                    .connections
                    .send(ClashConnectionsConnectorEvent::Update(info));

                let snapshot = ClashWsConnectionSnapshot {
                    download_total: msg.download_total,
                    upload_total: msg.upload_total,
                    download_speed,
                    upload_speed,
                    memory: msg.memory,
                    connections: msg.connections,
                };
                if self.recording.connections {
                    push_limited(
                        &mut self.history.connections,
                        snapshot.clone(),
                        MAX_CONNECTIONS_HISTORY,
                    );
                }
                self.emit(ClashWsEvent::ConnectionsUpdated(snapshot.clone()));

                if self.args.details.receiver_count() > 0 {
                    let rows = snapshot.connections.as_deref().unwrap_or_default();
                    let connections =
                        derive_connection_details(&mut self.detail_counters, rows, Instant::now());
                    let _ =
                        self.args
                            .details
                            .send_replace(Some(Arc::new(ClashConnectionDetails {
                                sequence: self.sequence,
                                connections,
                            })));
                } else {
                    self.detail_counters = DetailCounters::default();
                    let _ = self.args.details.send_replace(None);
                }

                if let Some(api) = &self.api {
                    let _ = self
                        .args
                        .frames
                        .send_replace(Some(Arc::new(ClashConnectionsFrame {
                            instance_id: self.capture_id.clone(),
                            core_instance_id: api.instance_id().unwrap_or("unbound").to_owned(),
                            snapshot,
                        })));
                }
            }
            Sample::Log(sample) => {
                let log = ClashWsLog {
                    log_type: sample.level.as_str().to_owned(),
                    time: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
                    payload: sample.payload,
                };
                if self.recording.logs {
                    push_limited(&mut self.history.logs, log.clone(), MAX_LOGS_HISTORY);
                }
                self.emit(ClashWsEvent::LogAppended(log));
            }
            Sample::Traffic(sample) => {
                let (Ok(up), Ok(down)) = (
                    u64::try_from(sample.up.get()),
                    u64::try_from(sample.down.get()),
                ) else {
                    return;
                };
                let traffic = ClashWsTraffic { up, down };
                if self.recording.traffic {
                    push_limited(
                        &mut self.history.traffic,
                        traffic.clone(),
                        MAX_TRAFFIC_HISTORY,
                    );
                }
                self.emit(ClashWsEvent::TrafficUpdated(traffic));
            }
            Sample::Memory(sample) => {
                if let Some(memory) = normalize_memory(sample) {
                    if self.recording.memory {
                        push_limited(&mut self.history.memory, memory.clone(), MAX_MEMORY_HISTORY);
                    }
                    self.emit(ClashWsEvent::MemoryUpdated(memory));
                }
            }
        }
    }
}

async fn deliver(actor: &ActorRef<Message>, delivery: Delivery) -> DeliveryResult {
    match actor
        .call(|reply| Message::Deliver(Box::new(delivery), reply), None)
        .await
    {
        Ok(CallResult::Success(result)) => result,
        _ => DeliveryResult::Rejected,
    }
}

async fn run(
    actor: ActorRef<Message>,
    source: Arc<dyn ClashApiClientSource>,
    shutdown: CancellationToken,
) {
    loop {
        let api = tokio::select! {
            biased;
            _ = shutdown.cancelled() => return,
            result = source.api_client() => match result {
                Ok(api) => api,
                Err(error) => {
                    tracing::debug!(%error, "Clash stream API is not available yet");
                    tokio::select! {
                        _ = shutdown.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                    }
                }
            }
        };

        let epochs = match deliver(&actor, Delivery::Bind(api.clone())).await {
            DeliveryResult::Bound(epochs) => epochs,
            DeliveryResult::Accepted => unreachable!("binding must return stream epochs"),
            DeliveryResult::Rejected if api.is_revoked() => {
                tokio::select! {
                    _ = shutdown.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(1)) => continue,
                }
            }
            DeliveryResult::Rejected => return,
        };

        let mut streams = JoinSet::new();
        streams.spawn(run_stream(
            actor.clone(),
            api.clone(),
            Stream::Connections,
            epochs[ClashWsKind::Connections.index()],
        ));
        streams.spawn(run_stream(
            actor.clone(),
            api.clone(),
            Stream::Traffic,
            epochs[ClashWsKind::Traffic.index()],
        ));
        streams.spawn(run_stream(
            actor.clone(),
            api.clone(),
            Stream::Memory,
            epochs[ClashWsKind::Memory.index()],
        ));

        tokio::select! {
            biased;
            _ = shutdown.cancelled() => {},
            _ = api.cancelled() => {},
            result = streams.join_next() => {
                if let Some(Err(error)) = result {
                    match error.try_into_panic() {
                        Ok(panic) => std::panic::resume_unwind(panic),
                        Err(error) => tracing::warn!("Clash stream task failed: {error}"),
                    }
                }
            }
        }
        streams.abort_all();
        while streams.join_next().await.is_some() {}

        if matches!(
            deliver(&actor, Delivery::Invalidated).await,
            DeliveryResult::Rejected
        ) {
            return;
        }
        if shutdown.is_cancelled() {
            return;
        }
    }
}

fn subscription_level(level: LogLevel) -> clash_api::LogLevel {
    match level {
        LogLevel::Silent => clash_api::LogLevel::Silent,
        LogLevel::Error => clash_api::LogLevel::Error,
        LogLevel::Warning => clash_api::LogLevel::Warning,
        LogLevel::Info => clash_api::LogLevel::Info,
        LogLevel::Debug => clash_api::LogLevel::Debug,
    }
}

async fn run_stream(actor: ActorRef<Message>, api: ApiClient, kind: Stream, epoch: u64) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if kind == Stream::Connections
            && !matches!(
                deliver(
                    &actor,
                    Delivery::State(
                        api.clone(),
                        epoch,
                        ClashConnectionsConnectorState::Connecting,
                    ),
                )
                .await,
                DeliveryResult::Accepted
            )
        {
            return;
        }

        macro_rules! consume {
            ($open:expr, $variant:ident) => {{
                match $open.await {
                    Ok(mut stream) => {
                        if kind == Stream::Connections
                            && !matches!(
                                deliver(
                                    &actor,
                                    Delivery::State(
                                        api.clone(),
                                        epoch,
                                        ClashConnectionsConnectorState::Connected,
                                    ),
                                )
                                .await,
                                DeliveryResult::Accepted
                            )
                        {
                            return;
                        }
                        while let Some(frame) = stream.next().await {
                            match frame {
                                Ok(sample) => {
                                    backoff = Duration::from_secs(1);
                                    if !matches!(
                                        deliver(
                                            &actor,
                                            Delivery::Sample(
                                                api.clone(),
                                                epoch,
                                                Sample::$variant(sample),
                                            ),
                                        )
                                        .await,
                                        DeliveryResult::Accepted
                                    ) {
                                        return;
                                    }
                                }
                                Err(ApiError::Protocol(clash_api::Error::Decode { .. })) => {
                                    tracing::warn!(?kind, "discarded malformed Clash stream frame");
                                }
                                Err(ApiError::Stale) => break,
                                Err(error) => {
                                    tracing::warn!(?kind, "Clash stream failed: {error:#}");
                                    break;
                                }
                            }
                        }
                    }
                    Err(ApiError::Stale) => {
                        tracing::debug!(?kind, "Clash stream instance retired");
                    }
                    Err(error) => {
                        tracing::warn!(?kind, "Clash stream handshake failed: {error:#}");
                    }
                }
            }};
        }

        match kind {
            Stream::Connections => consume!(api.connections_ws(), Connections),
            Stream::Logs(level) => consume!(api.logs_ws(level), Log),
            Stream::Traffic => consume!(api.traffic_ws(), Traffic),
            Stream::Memory => consume!(api.memory_ws(), Memory),
        }

        if kind == Stream::Connections
            && !matches!(
                deliver(
                    &actor,
                    Delivery::State(
                        api.clone(),
                        epoch,
                        ClashConnectionsConnectorState::Disconnected,
                    ),
                )
                .await,
                DeliveryResult::Accepted
            )
        {
            return;
        }

        tokio::select! {
            _ = api.cancelled() => return,
            _ = tokio::time::sleep(backoff) => {},
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

impl Actor for StreamsActor {
    type Msg = Message;
    type State = State;
    type Arguments = Args;

    async fn pre_start(
        &self,
        _: ActorRef<Message>,
        args: Args,
    ) -> Result<State, ActorProcessingErr> {
        Ok(State {
            args,
            task: None,
            logs: None,
            api: None,
            status: ClashConnectionsConnectorState::Disconnected,
            sequence: 0,
            history: ClashWsHistory::default(),
            recording: ClashWsRecording::default(),
            previous_download_total: 0,
            previous_upload_total: 0,
            detail_counters: DetailCounters::default(),
            details_closed: None,
            capture_id: uuid::Uuid::new_v4().to_string(),
            stream_epochs: [0; 4],
        })
    }

    async fn handle(
        &self,
        actor: ActorRef<Message>,
        message: Message,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Start(reply) => {
                if !state.args.shutdown.is_cancelled() && state.task.is_none() {
                    state.task = Some(tokio::spawn(run(
                        actor,
                        state.args.source.clone(),
                        state.args.shutdown.clone(),
                    )));
                }
                let _ = reply.send(());
            }
            Message::Stop(reply) => {
                state.stop().await;
                let _ = reply.send(());
            }
            Message::Restart(reply) => {
                state.stop().await;
                if !state.args.shutdown.is_cancelled() {
                    state.task = Some(tokio::spawn(run(
                        actor,
                        state.args.source.clone(),
                        state.args.shutdown.clone(),
                    )));
                    let _ = reply.send(Ok(()));
                } else {
                    let _ = reply.send(Err(anyhow::anyhow!("streams actor is shutting down")));
                }
            }
            Message::SubscribeDetails(reply) => {
                if state.args.details.receiver_count() == 0 {
                    let _ = state.args.details.send_replace(None);
                }
                let receiver = state.args.details.subscribe();
                if let Some(task) = state.details_closed.take() {
                    task.abort();
                }
                let details = state.args.details.clone();
                let owner = actor.clone();
                state.details_closed = Some(tokio::spawn(async move {
                    details.closed().await;
                    let _ = owner.cast(Message::DetailsClosed);
                }));
                let _ = reply.send(receiver);
            }
            Message::DetailsClosed => {
                if state.args.details.receiver_count() == 0 {
                    state.detail_counters = DetailCounters::default();
                    let _ = state.args.details.send_replace(None);
                }
            }
            Message::Recording(kind, enabled, reply) => {
                state.recording.set(kind, enabled);
                state.emit(ClashWsEvent::RecordingChanged(state.recording.clone()));
                state.publish_snapshot();
                let _ = reply.send(state.recording.clone());
            }
            Message::Clear(kind, reply) => {
                state.history.clear(kind);
                state.emit(ClashWsEvent::HistoryCleared(kind));
                state.publish_snapshot();
                let _ = reply.send(Ok(()));
            }
            Message::Deliver(delivery, reply) => {
                let mut result = DeliveryResult::Rejected;
                if !state.args.shutdown.is_cancelled() && state.task.is_some() {
                    match *delivery {
                        Delivery::Bind(api) => {
                            if !api.is_revoked() {
                                if !state.accepts(&api) {
                                    state.reset().await;
                                }
                                state.api = Some(api);
                                state.restart_logs(&actor);
                                result = DeliveryResult::Bound(state.stream_epochs);
                            }
                        }
                        Delivery::State(api, epoch, status) => {
                            if state.accepts(&api)
                                && epoch == state.stream_epochs[ClashWsKind::Connections.index()]
                            {
                                state.status(status);
                                result = DeliveryResult::Accepted;
                            }
                        }
                        Delivery::Sample(api, epoch, sample) => {
                            let kind = match &sample {
                                Sample::Connections(_) => ClashWsKind::Connections,
                                Sample::Log(_) => ClashWsKind::Logs,
                                Sample::Traffic(_) => ClashWsKind::Traffic,
                                Sample::Memory(_) => ClashWsKind::Memory,
                            };
                            if state.accepts(&api) && epoch == state.stream_epochs[kind.index()] {
                                state.update(sample);
                                result = DeliveryResult::Accepted;
                            }
                        }
                        Delivery::Invalidated => {
                            if state.api.is_some() {
                                state.reset().await;
                                result = DeliveryResult::Accepted;
                            }
                        }
                    }
                }
                state.publish_snapshot();
                let _ = reply.send(result);
            }
            Message::SetLogLevel(level, reply) => {
                if state.args.log_level != level {
                    state.args.log_level = level;
                    state.restart_logs(&actor);
                }
                let _ = reply.send(());
            }
        }
        state.publish_snapshot();
        Ok(())
    }

    async fn post_stop(
        &self,
        _: ActorRef<Message>,
        state: &mut State,
    ) -> Result<(), ActorProcessingErr> {
        state.stop().await;
        state.publish_snapshot();
        Ok(())
    }
}

#[derive(Clone)]
pub struct StreamsClient(Arc<Inner>);

struct Inner {
    actor: ActorRef<Message>,
    shutdown: CancellationToken,
    connections: broadcast::Sender<ClashConnectionsConnectorEvent>,
    events: broadcast::Sender<ClashWsEvent>,
    snapshot: watch::Receiver<ClashWsSnapshot>,
    frames: watch::Receiver<Option<Arc<ClashConnectionsFrame>>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.actor.stop(None);
    }
}

impl StreamsClient {
    pub async fn spawn(
        source: Arc<dyn ClashApiClientSource>,
        log_level: LogLevel,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> std::result::Result<Self, Error> {
        let actor_shutdown = shutdown.child_token();
        let connections = broadcast::channel(16).0;
        let events = broadcast::channel(64).0;
        let details = watch::channel(None).0;
        let (frames, frames_rx) = watch::channel(None);
        let initial_snapshot = ClashWsHistory::default().snapshot(
            ClashConnectionsConnectorState::Disconnected,
            ClashWsRecording::default(),
        );
        let (snapshot, snapshot_rx) = watch::channel(initial_snapshot);
        let (actor, _) = Actor::spawn(
            None,
            StreamsActor,
            Args {
                source,
                log_level,
                shutdown: actor_shutdown.clone(),
                connections: connections.clone(),
                events: events.clone(),
                details,
                frames: frames.clone(),
                snapshot,
            },
        )
        .await
        .map_err(|source| Error::SpawnActor { source })?;

        let shutdown_actor = actor.clone();
        let shutdown_task = actor_shutdown.clone();
        tasks.spawn(async move {
            shutdown_task.cancelled().await;
            let _ = shutdown_actor.stop_and_wait(None, None).await;
        });

        Ok(Self(Arc::new(Inner {
            actor,
            shutdown: actor_shutdown,
            connections,
            events,
            snapshot: snapshot_rx,
            frames: frames_rx,
        })))
    }

    async fn call<T: Send + 'static>(
        &self,
        message: impl FnOnce(RpcReplyPort<T>) -> Message,
    ) -> Result<T> {
        match self
            .0
            .actor
            .call(message, None)
            .await
            .context("Clash streams actor unavailable")?
        {
            CallResult::Success(value) => Ok(value),
            _ => anyhow::bail!("Clash streams actor reply dropped"),
        }
    }

    pub async fn start(&self) -> Result<()> {
        self.call(Message::Start).await
    }

    pub async fn stop(&self) -> Result<()> {
        self.call(Message::Stop).await
    }

    pub async fn restart(&self) -> Result<()> {
        self.call(Message::Restart).await?
    }

    pub async fn set_log_level(&self, level: LogLevel) -> Result<()> {
        self.call(|reply| Message::SetLogLevel(level, reply)).await
    }

    pub fn state(&self) -> ClashConnectionsConnectorState {
        self.0.snapshot.borrow().state
    }

    pub fn snapshot(&self) -> ClashWsSnapshot {
        self.0.snapshot.borrow().clone()
    }

    pub async fn set_recording(
        &self,
        kind: ClashWsKind,
        enabled: bool,
    ) -> Result<ClashWsRecording> {
        self.call(|reply| Message::Recording(kind, enabled, reply))
            .await
    }

    pub async fn clear_history(&self, kind: ClashWsKind) -> Result<()> {
        self.call(|reply| Message::Clear(kind, reply)).await?
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ClashConnectionsConnectorEvent> {
        self.0.connections.subscribe()
    }

    pub fn subscribe_ws(&self) -> broadcast::Receiver<ClashWsEvent> {
        self.0.events.subscribe()
    }

    pub fn subscribe_connection_frames(
        &self,
    ) -> watch::Receiver<Option<Arc<ClashConnectionsFrame>>> {
        self.0.frames.clone()
    }

    pub async fn subscribe_connection_details(
        &self,
    ) -> Result<watch::Receiver<Option<Arc<ClashConnectionDetails>>>> {
        self.call(Message::SubscribeDetails).await
    }
}

#[cfg(test)]
#[path = "ws_tests.rs"]
mod tests;
