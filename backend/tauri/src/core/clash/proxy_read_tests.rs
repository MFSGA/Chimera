//! Production legacy IPC read-path regressions against a mock Clash controller.
//! An ordinary read burst must share one bound source snapshot; mutations have
//! their own reconciliation and are intentionally not coalesced by this lane.

use super::*;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DEADLINE: Duration = Duration::from_secs(6);

async fn controller() -> (ApiClient, tokio::task::JoinHandle<Vec<String>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock controller must listen");
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let mut paths = Vec::new();
        // A single source snapshot consists of proxies, providers and /group.
        for _ in 0..3 {
            let (mut socket, _) = tokio::time::timeout(DEADLINE, listener.accept())
                .await
                .expect("expected exactly one proxy snapshot")
                .unwrap();
            let mut bytes = [0_u8; 8192];
            let size = socket.read(&mut bytes).await.unwrap();
            let line = String::from_utf8_lossy(&bytes[..size])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let (status, body) = if line.starts_with("GET /proxies ") {
                (
                    "200 OK",
                    r#"{"proxies":{"DIRECT":{"name":"DIRECT","type":"Direct","udp":false,"history":[]},"REJECT":{"name":"REJECT","type":"Reject","udp":false,"history":[]},"GLOBAL":{"name":"GLOBAL","type":"Selector","udp":false,"history":[],"all":["Auto"]},"Auto":{"name":"Auto","type":"Selector","udp":false,"history":[],"all":["DIRECT","REJECT"],"now":"DIRECT"}}}"#,
                )
            } else if line.starts_with("GET /providers/proxies ") {
                ("200 OK", r#"{"providers":{}}"#)
            } else if line.starts_with("GET /group ") {
                ("404 Not Found", r#"{"message":"group route unsupported"}"#)
            } else {
                panic!("unexpected controller request: {line}");
            };
            paths.push(line);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
        paths
    });
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: address.port(),
        server: address.to_string(),
        secret: None,
    })
    .unwrap();
    (api, server)
}

#[tokio::test]
async fn simultaneous_legacy_readers_coalesce_one_snapshot_and_keep_cache_fresh() {
    let (api, server) = controller().await;
    let (sender, _) = broadcast::channel(8);
    let guard = Arc::new(RwLock::new(ProxiesGuard::new(sender)));
    let generation = guard.read().generation();
    let barrier = Arc::new(tokio::sync::Barrier::new(9));
    let readers = (0..8)
        .map(|_| {
            let guard = guard.clone();
            let api = api.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                (&guard)
                    .read_coalesced_if_generation(&api, generation)
                    .await
            })
        })
        .collect::<Vec<_>>();
    barrier.wait().await;

    for reader in readers {
        let value = tokio::time::timeout(DEADLINE, reader)
            .await
            .expect("coalesced proxy read timed out")
            .expect("reader task panicked")
            .expect("proxy read failed")
            .expect("same generation must publish");
        assert_eq!(value.groups[0].name, "Auto");
        assert_eq!(value.groups[0].now.as_deref(), Some("DIRECT"));
    }
    let requests = tokio::time::timeout(DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 3, "a burst must not send duplicate GETs");
    assert!(guard.read().is_updated());
}

#[tokio::test]
async fn fresh_snapshot_from_another_core_is_never_returned_without_new_source_read() {
    let (api, server) = controller().await;
    let (sender, _) = broadcast::channel(4);
    let mut old = Proxies::default();
    old.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("REJECT".into()),
        ..Default::default()
    });
    let guard = Arc::new(RwLock::new(ProxiesGuard::test_guard(
        old,
        Some(42),
        chrono::Utc::now().timestamp() as u64,
        0,
        sender,
    )));
    guard.write().source_instance = Some("retired-instance".into());
    assert!(
        guard.read().is_updated(),
        "old source has a fresh timestamp"
    );

    let received = (&guard)
        .read_coalesced_if_generation(&api, 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received.groups[0].now.as_deref(), Some("DIRECT"));
    assert_eq!(guard.read().source_instance, None, "unbound mock client");
    let requests = tokio::time::timeout(DEADLINE, server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests.len(), 3, "a new source must always be rechecked");
}

#[test]
fn late_successful_controller_read_cannot_rewind_legacy_snapshot_or_notify_ui() {
    let (sender, mut changes) = broadcast::channel(4);
    let mut previous = Proxies::default();
    previous.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("REJECT".into()),
        ..Default::default()
    });
    let mut guard = ProxiesGuard::test_guard(
        previous,
        Some(41),
        chrono::Utc::now().timestamp() as u64,
        0,
        sender,
    );
    let generation = guard.generation();
    let old_read_revision = guard.revision();
    let mut newer = Proxies::default();
    newer.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("DIRECT".into()),
        ..Default::default()
    });
    assert!(guard.replace_if_generation(generation, old_read_revision, newer, 42));
    changes
        .try_recv()
        .expect("fresh controller observation must notify");

    let mut late = Proxies::default();
    late.groups.push(ProxyGroupItem {
        name: "Auto".into(),
        now: Some("REJECT".into()),
        ..Default::default()
    });
    assert!(
        !guard.replace_if_generation(generation, old_read_revision, late, 43),
        "a slow GET started before the newest reply must be discarded"
    );
    assert_eq!(guard.inner().groups[0].now.as_deref(), Some("DIRECT"));
    assert!(guard.is_updated());
    assert!(
        changes.try_recv().is_err(),
        "discarding an old successful read must not wake subscribers"
    );
}

#[tokio::test]
async fn waiting_read_from_old_profile_never_dispatches_or_publishes() {
    let (sender, _) = broadcast::channel(1);
    let guard = Arc::new(RwLock::new(ProxiesGuard::new(sender)));
    let old_generation = guard.read().generation();
    let lane = guard.read().read_turn.clone();
    let holding = lane.lock().await;
    let api = ApiClient::new(crate::config::clash::ClashInfo {
        port: 1,
        server: "127.0.0.1:1".into(),
        secret: None,
    })
    .unwrap();
    let read_guard = guard.clone();
    let waiter = tokio::spawn(async move {
        (&read_guard)
            .read_coalesced_if_generation(&api, old_generation)
            .await
    });
    tokio::task::yield_now().await;
    guard.write().invalidate();
    drop(holding);

    let result = tokio::time::timeout(DEADLINE, waiter)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(result.is_none());
    assert!(!guard.read().is_updated());
}
