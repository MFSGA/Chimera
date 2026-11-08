use super::*;
use axum::{
    Router,
    extract::{Path, Query, State as AxumState, WebSocketUpgrade, ws::Message},
    response::IntoResponse,
    routing::get,
};
use std::collections::HashMap;
use tokio::sync::mpsc;

struct StaticSource {
    server: String,
}

#[async_trait::async_trait]
impl ClashApiClientSource for StaticSource {
    async fn api_client(&self) -> Result<ApiClient> {
        ApiClient::new_unbound(self.server.clone(), None)
    }
}

struct RotatingSource {
    server: String,
    api: tokio::sync::RwLock<ApiClient>,
}

impl RotatingSource {
    fn new(server: String) -> Self {
        Self {
            api: tokio::sync::RwLock::new(ApiClient::new_unbound(server.clone(), None).unwrap()),
            server,
        }
    }

    async fn rotate(&self) {
        self.api.read().await.revoke();
        *self.api.write().await = ApiClient::new_unbound(self.server.clone(), None).unwrap();
    }
}

#[async_trait::async_trait]
impl ClashApiClientSource for RotatingSource {
    async fn api_client(&self) -> Result<ApiClient> {
        Ok(self.api.read().await.clone())
    }
}

async fn serve(router: Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (address.to_string(), task)
}

async fn wait_for_state(
    events: &mut broadcast::Receiver<ClashWsEvent>,
    expected: ClashConnectionsConnectorState,
) {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if matches!(
                events.recv().await.unwrap(),
                ClashWsEvent::StateChanged(state) if state == expected
            ) {
                break;
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn typed_websocket_workers_publish_all_four_streams() {
    async fn stream(Path(kind): Path<String>, ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            let json = match kind.as_str() {
                "connections" => {
                    r#"{"downloadTotal":100,"uploadTotal":200,"connections":null,"memory":12}"#
                }
                "logs" => r#"{"type":"info","payload":"typed log"}"#,
                "traffic" => r#"{"up":3,"down":4}"#,
                "memory" => r#"{"inuse":12,"oslimit":100}"#,
                _ => unreachable!(),
            };
            socket.send(Message::Text(json.into())).await.unwrap();
            while socket.recv().await.is_some() {}
        })
    }

    let (server, task) = serve(Router::new().route("/{kind}", get(stream))).await;
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = StreamsClient::spawn(
        Arc::new(StaticSource { server }),
        LogLevel::Debug,
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();

    tokio::time::timeout(Duration::from_secs(4), async {
        let mut seen = [false; 4];
        while !seen.iter().all(|seen| *seen) {
            match events.recv().await.unwrap() {
                ClashWsEvent::ConnectionsUpdated(_) => seen[0] = true,
                ClashWsEvent::LogAppended(_) => seen[1] = true,
                ClashWsEvent::TrafficUpdated(_) => seen[2] = true,
                ClashWsEvent::MemoryUpdated(_) => seen[3] = true,
                _ => {}
            }
        }
    })
    .await
    .unwrap();

    let snapshot = client.snapshot();
    assert_eq!(snapshot.state, ClashConnectionsConnectorState::Connected);
    assert_eq!(snapshot.connections[0].download_total, 100);
    assert_eq!(snapshot.logs[0].payload, "typed log");
    assert_eq!(snapshot.traffic, [ClashWsTraffic { up: 3, down: 4 }]);
    assert_eq!(
        snapshot.memory,
        [ClashWsMemory {
            inuse: 12,
            oslimit: 100
        }]
    );

    client.stop().await.unwrap();
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    task.abort();
}

#[tokio::test]
async fn log_level_changes_reopen_only_the_logs_socket() {
    type Opened = mpsc::UnboundedSender<(String, String)>;

    async fn stream(
        Path(kind): Path<String>,
        Query(query): Query<HashMap<String, String>>,
        AxumState(opened): AxumState<Opened>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        let level = query.get("level").cloned().unwrap_or_default();
        ws.on_upgrade(move |mut socket| async move {
            let _ = opened.send((kind.clone(), level.clone()));
            while socket.recv().await.is_some() {}
            let _ = opened.send((format!("{kind} closed"), level));
        })
    }

    async fn receive(
        events: &mut mpsc::UnboundedReceiver<(String, String)>,
        count: usize,
    ) -> Vec<(String, String)> {
        let mut received = Vec::new();
        for _ in 0..count {
            received.push(
                tokio::time::timeout(Duration::from_secs(4), events.recv())
                    .await
                    .unwrap()
                    .unwrap(),
            );
        }
        received.sort();
        received
    }

    let (opened, mut events) = mpsc::unbounded_channel();
    let router = Router::new()
        .route("/{kind}", get(stream))
        .with_state(opened);
    let (server, task) = serve(router).await;
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = StreamsClient::spawn(
        Arc::new(StaticSource { server }),
        LogLevel::Info,
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    client.start().await.unwrap();

    assert_eq!(
        receive(&mut events, 4).await,
        vec![
            ("connections".into(), "".into()),
            ("logs".into(), "info".into()),
            ("memory".into(), "".into()),
            ("traffic".into(), "".into()),
        ]
    );

    client.set_log_level(LogLevel::Error).await.unwrap();
    assert_eq!(
        receive(&mut events, 2).await,
        vec![
            ("logs".into(), "error".into()),
            ("logs closed".into(), "info".into()),
        ]
    );
    client.set_log_level(LogLevel::Error).await.unwrap();
    assert!(events.try_recv().is_err());

    client.set_log_level(LogLevel::Silent).await.unwrap();
    assert_eq!(
        receive(&mut events, 1).await,
        vec![("logs closed".into(), "error".into())]
    );
    client.set_log_level(LogLevel::Debug).await.unwrap();
    assert_eq!(
        receive(&mut events, 1).await,
        vec![("logs".into(), "debug".into())]
    );

    client.stop().await.unwrap();
    assert_eq!(
        receive(&mut events, 4).await,
        vec![
            ("connections closed".into(), "".into()),
            ("logs closed".into(), "debug".into()),
            ("memory closed".into(), "".into()),
            ("traffic closed".into(), "".into()),
        ]
    );

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    task.abort();
}

#[tokio::test]
async fn api_revocation_resets_streams_and_reconnects_with_a_new_capability() {
    async fn stream(Path(kind): Path<String>, ws: WebSocketUpgrade) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            if kind == "connections" {
                socket
                    .send(Message::Text(
                        r#"{"downloadTotal":100,"uploadTotal":200,"connections":null}"#.into(),
                    ))
                    .await
                    .unwrap();
            }
            while socket.recv().await.is_some() {}
        })
    }

    let (server, task) = serve(Router::new().route("/{kind}", get(stream))).await;
    let source = Arc::new(RotatingSource::new(server));
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = StreamsClient::spawn(source.clone(), LogLevel::Silent, shutdown.clone(), &tasks)
        .await
        .unwrap();
    let mut events = client.subscribe_ws();
    client.start().await.unwrap();
    wait_for_state(&mut events, ClashConnectionsConnectorState::Connected).await;
    tokio::time::timeout(Duration::from_secs(4), async {
        while client.snapshot().connections.is_empty() {
            events.recv().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(client.snapshot().connections[0].download_total, 100);

    source.rotate().await;
    wait_for_state(&mut events, ClashConnectionsConnectorState::Disconnected).await;
    wait_for_state(&mut events, ClashConnectionsConnectorState::Connecting).await;
    wait_for_state(&mut events, ClashConnectionsConnectorState::Connected).await;
    tokio::time::timeout(Duration::from_secs(4), async {
        while client.snapshot().connections.is_empty() {
            events.recv().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(client.snapshot().connections[0].download_speed, 100);

    client.stop().await.unwrap();
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    task.abort();
}

#[tokio::test]
async fn typed_api_stream_revocation_interrupts_idle_socket_reads() {
    async fn traffic(
        AxumState(closed): AxumState<mpsc::UnboundedSender<()>>,
        ws: WebSocketUpgrade,
    ) -> impl IntoResponse {
        ws.on_upgrade(move |mut socket| async move {
            socket
                .send(Message::Text(r#"{"up":1,"down":2}"#.into()))
                .await
                .unwrap();
            while socket.recv().await.is_some() {}
            let _ = closed.send(());
        })
    }

    let (closed, mut closed_rx) = mpsc::unbounded_channel();
    let router = Router::new()
        .route("/traffic", get(traffic))
        .with_state(closed);
    let (server, task) = serve(router).await;
    let api = ApiClient::new_unbound(server, None).unwrap();
    let mut stream = api.traffic_ws().await.unwrap();
    let first = stream.next().await.unwrap().unwrap();
    assert_eq!((first.up.get(), first.down.get()), (1, 2));

    api.revoke();
    assert!(matches!(stream.next().await, Some(Err(ApiError::Stale))));
    assert!(stream.next().await.is_none());
    tokio::time::timeout(Duration::from_secs(3), closed_rx.recv())
        .await
        .unwrap()
        .unwrap();
    task.abort();
}
