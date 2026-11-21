use super::*;
use std::sync::Arc;

struct UnavailableSource;

#[async_trait::async_trait]
impl ClashApiClientSource for UnavailableSource {
    async fn api_client(&self) -> Result<ApiClient> {
        anyhow::bail!("no controller is configured for this test")
    }
}

async fn setup() -> (StreamsClient, CancellationToken, TaskTracker) {
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = StreamsClient::spawn(
        Arc::new(UnavailableSource),
        LogLevel::Info,
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    client.start().await.unwrap();
    (client, shutdown, tasks)
}

async fn teardown(client: StreamsClient, shutdown: CancellationToken, tasks: TaskTracker) {
    let _ = client.stop().await;
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

fn api() -> ApiClient {
    ApiClient::new_unbound("127.0.0.1:1".into(), None).unwrap()
}

async fn bind(client: &StreamsClient, api: ApiClient) -> [u64; 4] {
    match deliver(&client.0.actor, Delivery::Bind(api)).await {
        DeliveryResult::Bound(epochs) => epochs,
        result => panic!("unexpected bind result: {result:?}"),
    }
}

fn connections(download_total: i64, upload_total: i64) -> clash_api::ConnectionsSnapshot {
    serde_json::from_value(serde_json::json!({
        "downloadTotal": download_total,
        "uploadTotal": upload_total,
        "memory": 1024,
        "connections": [],
    }))
    .unwrap()
}

fn log(payload: &str) -> clash_api::LogEntry {
    clash_api::LogEntry {
        level: clash_api::ConfigEnum::Known(clash_api::LogLevel::Info),
        payload: payload.to_owned(),
    }
}

fn traffic(up: u64, down: u64) -> clash_api::Traffic {
    serde_json::from_value(serde_json::json!({ "up": up, "down": down })).unwrap()
}

fn memory(inuse: u64, oslimit: u64) -> clash_api::Memory {
    serde_json::from_value(serde_json::json!({ "inuse": inuse, "oslimit": oslimit })).unwrap()
}

#[tokio::test]
async fn actor_owns_all_stream_histories_and_emits_legacy_updates() {
    let (client, shutdown, tasks) = setup().await;
    let api = api();
    let epochs = bind(&client, api.clone()).await;
    let mut events = client.subscribe_ws();
    let mut connections_events = client.subscribe();
    while events.try_recv().is_ok() {}
    while connections_events.try_recv().is_ok() {}

    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api.clone(),
                epochs[ClashWsKind::Connections.index()],
                Sample::Connections(connections(100, 40)),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api.clone(),
                epochs[ClashWsKind::Logs.index()],
                Sample::Log(log("hello")),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api.clone(),
                epochs[ClashWsKind::Traffic.index()],
                Sample::Traffic(traffic(11, 22)),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api.clone(),
                epochs[ClashWsKind::Memory.index()],
                Sample::Memory(memory(1024, 4096)),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));

    let snapshot = client.snapshot();
    assert_eq!(snapshot.connections.len(), 1);
    assert_eq!(snapshot.connections[0].download_speed, 100);
    assert_eq!(snapshot.logs.len(), 1);
    assert_eq!(snapshot.logs[0].payload, "hello");
    assert_eq!(snapshot.traffic, [ClashWsTraffic { up: 11, down: 22 }]);
    assert_eq!(
        snapshot.memory,
        [ClashWsMemory {
            inuse: 1024,
            oslimit: 4096
        }]
    );

    assert!(matches!(
        connections_events.try_recv().unwrap(),
        ClashConnectionsConnectorEvent::Update(ClashConnectionsInfo {
            download_total: 100,
            upload_total: 40,
            download_speed: 100,
            upload_speed: 40,
        })
    ));
    let mut seen = Vec::new();
    while let Ok(event) = events.try_recv() {
        seen.push(event);
    }
    assert!(
        seen.iter()
            .any(|event| matches!(event, ClashWsEvent::ConnectionsUpdated(_)))
    );
    assert!(
        seen.iter()
            .any(|event| matches!(event, ClashWsEvent::LogAppended(_)))
    );
    assert!(
        seen.iter()
            .any(|event| matches!(event, ClashWsEvent::TrafficUpdated(_)))
    );
    assert!(
        seen.iter()
            .any(|event| matches!(event, ClashWsEvent::MemoryUpdated(_)))
    );

    teardown(client, shutdown, tasks).await;
}

#[tokio::test]
async fn disabled_recording_still_emits_live_updates_and_clear_is_actor_owned() {
    let (client, shutdown, tasks) = setup().await;
    let api = api();
    let epochs = bind(&client, api.clone()).await;
    let mut events = client.subscribe_ws();
    while events.try_recv().is_ok() {}

    client
        .set_recording(ClashWsKind::Connections, false)
        .await
        .unwrap();
    while events.try_recv().is_ok() {}
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api.clone(),
                epochs[ClashWsKind::Connections.index()],
                Sample::Connections(connections(20, 10)),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));
    assert!(client.snapshot().connections.is_empty());
    assert!(
        matches!(
            events.try_recv().unwrap(),
            ClashWsEvent::ConnectionsUpdated(_)
        ),
        "live updates are not recording-gated"
    );

    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Logs.index()],
            Sample::Log(log("clear me")),
        ),
    )
    .await;
    assert_eq!(client.snapshot().logs.len(), 1);
    client.clear_history(ClashWsKind::Logs).await.unwrap();
    assert!(client.snapshot().logs.is_empty());
    let mut cleared = false;
    while let Ok(event) = events.try_recv() {
        if matches!(event, ClashWsEvent::HistoryCleared(ClashWsKind::Logs)) {
            cleared = true;
        }
    }
    assert!(cleared);

    teardown(client, shutdown, tasks).await;
}

#[tokio::test]
async fn replacement_and_stream_epoch_fences_reject_queued_old_samples() {
    let (client, shutdown, tasks) = setup().await;
    let old_api = api();
    let old_epochs = bind(&client, old_api.clone()).await;
    deliver(
        &client.0.actor,
        Delivery::Sample(
            old_api.clone(),
            old_epochs[ClashWsKind::Logs.index()],
            Sample::Log(log("old instance")),
        ),
    )
    .await;

    let new_api = api();
    let new_epochs = bind(&client, new_api.clone()).await;
    assert!(client.snapshot().logs.is_empty());
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                old_api.clone(),
                old_epochs[ClashWsKind::Logs.index()],
                Sample::Log(log("late old instance")),
            ),
        )
        .await,
        DeliveryResult::Rejected
    ));

    client.set_log_level(LogLevel::Debug).await.unwrap();
    let refreshed_epochs = bind(&client, new_api.clone()).await;
    assert_ne!(
        refreshed_epochs[ClashWsKind::Logs.index()],
        new_epochs[ClashWsKind::Logs.index()]
    );
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                new_api.clone(),
                new_epochs[ClashWsKind::Logs.index()],
                Sample::Log(log("late old socket")),
            ),
        )
        .await,
        DeliveryResult::Rejected
    ));
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                new_api.clone(),
                refreshed_epochs[ClashWsKind::Logs.index()],
                Sample::Log(log("new socket")),
            ),
        )
        .await,
        DeliveryResult::Accepted
    ));
    assert_eq!(client.snapshot().logs.len(), 1);
    assert_eq!(client.snapshot().logs[0].payload, "new socket");

    teardown(client, shutdown, tasks).await;
}

#[tokio::test]
async fn reset_clears_history_and_frames_but_preserves_recording_preferences() {
    let (client, shutdown, tasks) = setup().await;
    let api = api();
    let epochs = bind(&client, api.clone()).await;
    let frames = client.subscribe_connection_frames();
    client
        .set_recording(ClashWsKind::Traffic, false)
        .await
        .unwrap();

    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Connections.index()],
            Sample::Connections(connections(100, 40)),
        ),
    )
    .await;
    assert!(frames.borrow().is_some());
    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Logs.index()],
            Sample::Log(log("old")),
        ),
    )
    .await;
    assert_eq!(client.snapshot().logs.len(), 1);

    assert!(matches!(
        deliver(&client.0.actor, Delivery::Invalidated).await,
        DeliveryResult::Accepted
    ));
    let snapshot = client.snapshot();
    assert!(snapshot.connections.is_empty());
    assert!(snapshot.logs.is_empty());
    assert!(snapshot.traffic.is_empty());
    assert!(snapshot.memory.is_empty());
    assert!(!snapshot.recording.traffic);
    assert!(frames.borrow().is_none());
    assert_eq!(snapshot.state, ClashConnectionsConnectorState::Disconnected);

    teardown(client, shutdown, tasks).await;
}

#[tokio::test]
async fn details_are_published_only_for_subscribers_and_frames_always_update() {
    let (client, shutdown, tasks) = setup().await;
    let api = api();
    let epochs = bind(&client, api.clone()).await;
    let frames = client.subscribe_connection_frames();

    let no_details = serde_json::from_value::<clash_api::ConnectionsSnapshot>(serde_json::json!({
        "downloadTotal": 10,
        "uploadTotal": 5,
        "connections": [],
    }))
    .unwrap();
    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Connections.index()],
            Sample::Connections(no_details),
        ),
    )
    .await;
    assert!(frames.borrow().is_some());

    let details = client.subscribe_connection_details().await.unwrap();
    let row = serde_json::json!({
        "id": "11111111-1111-1111-1111-111111111111",
        "metadata": null,
        "upload": 10,
        "download": 20,
        "start": "2024-01-01T00:00:00Z",
        "chains": ["DIRECT"],
        "rule": "MATCH",
        "rulePayload": "",
    });
    let with_details =
        serde_json::from_value::<clash_api::ConnectionsSnapshot>(serde_json::json!({
            "downloadTotal": 20,
            "uploadTotal": 10,
            "connections": [row],
        }))
        .unwrap();
    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Connections.index()],
            Sample::Connections(with_details),
        ),
    )
    .await;
    assert_eq!(details.borrow().as_ref().unwrap().connections.len(), 1);

    drop(details);
    let after_drop = client.subscribe_connection_details().await.unwrap();
    assert!(after_drop.borrow().is_none());

    teardown(client, shutdown, tasks).await;
}

#[tokio::test]
async fn stop_rejects_late_messages_and_clears_the_published_snapshot() {
    let (client, shutdown, tasks) = setup().await;
    let api = api();
    let epochs = bind(&client, api.clone()).await;
    deliver(
        &client.0.actor,
        Delivery::Sample(
            api.clone(),
            epochs[ClashWsKind::Logs.index()],
            Sample::Log(log("before stop")),
        ),
    )
    .await;
    assert_eq!(client.snapshot().logs.len(), 1);

    client.stop().await.unwrap();
    assert!(client.snapshot().logs.is_empty());
    assert!(matches!(
        deliver(
            &client.0.actor,
            Delivery::Sample(
                api,
                epochs[ClashWsKind::Logs.index()],
                Sample::Log(log("after stop")),
            ),
        )
        .await,
        DeliveryResult::Rejected
    ));

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[test]
fn normalize_memory_clamps_obvious_unit_mismatch() {
    let memory = normalize_memory(memory(8000, 1000)).unwrap();
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

#[path = "ws_integration_tests.rs"]
mod integration;

// Source-only regression coverage for the ref-aligned stream-owner drain.
// The maintainer asked to continue migration without running these tests.
#[tokio::test]
async fn cancelling_stream_worker_reaps_socket_before_replacing_it() {
    use tokio::sync::oneshot;

    struct DropNotice(Option<oneshot::Sender<()>>);
    impl Drop for DropNotice {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    let (ready, receiver) = oneshot::channel();
    let (on_drop, dropped) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let _notice = DropNotice(Some(on_drop));
        let _ = ready.send(());
        std::future::pending::<()>().await;
    });
    receiver.await.expect("socket task must begin");
    let mut slot = Some(handle);
    let mut first_panic = None;
    cancel_stream_worker(&mut slot, "test socket", &mut first_panic).await;
    assert!(slot.is_none());
    assert!(first_panic.is_none());
    dropped
        .await
        .expect("socket must be dropped before drain returns");
}

#[tokio::test]
async fn a_fatal_stream_join_can_be_propagated_after_other_workers_are_reaped() {
    let faulty = tokio::spawn(async { panic!("broken websocket worker") });
    let healthy = tokio::spawn(async {});
    let mut first_panic = None;
    collect_stream_join(faulty.await, "faulty socket", &mut first_panic);
    collect_stream_join(healthy.await, "healthy socket", &mut first_panic);
    assert!(first_panic.is_some(), "fatal task panic must not disappear");
}
