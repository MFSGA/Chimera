use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chimera_traffic::{ClosedCursor, Dimension, RedbTrafficStore, TrafficFilter, TrafficRange};

use super::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficArgs,
    TrafficClient,
};
use crate::core::clash::ws::{ClashConnectionsFrame, ClashWsConnectionSnapshot};

struct TestProfile;

impl ProfileSelection for TestProfile {
    fn current(&self) -> Option<String> {
        Some("profile-one".into())
    }
}

struct SevenDayRetention;

impl RetentionPolicy for SevenDayRetention {
    fn retention(&self) -> Option<Duration> {
        Some(Duration::from_secs(7 * 24 * 60 * 60))
    }
}

struct NoLocalSource;

impl LocalSourceLocation for NoLocalSource {
    fn addresses(&self) -> LocalSourceIps {
        LocalSourceIps::default()
    }
}

struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| {
                i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
            })
    }
}

#[tokio::test]
async fn connection_frames_become_active_and_then_persist_as_closed() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap());
    let (frames, receiver) = tokio::sync::watch::channel(None);
    let started_at = chrono::Utc::now().to_rfc3339();
    let traffic = TrafficClient::spawn(TrafficArgs {
        store,
        profiles: Arc::new(TestProfile),
        retention: Arc::new(SevenDayRetention),
        clock: Arc::new(SystemClock),
        frames: receiver,
        local_source: Arc::new(NoLocalSource),
    })
    .await
    .unwrap();

    frames.send_replace(Some(Arc::new(ClashConnectionsFrame {
        instance_id: "core-process-1".into(),
        snapshot: ClashWsConnectionSnapshot {
            upload_total: 120,
            download_total: 340,
            upload_speed: 0,
            download_speed: 0,
            memory: None,
            connections: Some(vec![serde_json::json!({
                "id": "00000000-0000-0000-0000-000000000001",
                "metadata": {
                    "network": "tcp",
                    "host": "example.org",
                    "sourceIP": "8.8.8.8",
                    "sourceGeoIP": ["us"],
                    "destinationIP": "1.1.1.1",
                    "destinationGeoIP": ["au"],
                    "process": "curl",
                    "processPath": "/usr/bin/curl",
                    "inboundName": "mixed"
                },
                "upload": 12,
                "download": 34,
                "start": started_at,
                "chains": ["Proxy-A"],
                "rule": "MATCH",
                "rulePayload": "Proxy-A"
            })]),
        },
    })));

    let active_filter = vec![TrafficFilter {
        dimension: Dimension::Process,
        value: "/usr/bin/curl".into(),
    }];
    let active_ids = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let ids = traffic
                .active_connection_ids(active_filter.clone())
                .await
                .unwrap();
            if !ids.is_empty() {
                break ids;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the connection frame should reach the accounting actor");
    assert_eq!(active_ids, ["00000000-0000-0000-0000-000000000001"]);

    frames.send_replace(None);
    tokio::task::yield_now().await;
    assert_eq!(
        traffic
            .active_connection_ids(active_filter.clone())
            .await
            .unwrap(),
        ["00000000-0000-0000-0000-000000000001"],
        "a lost WebSocket feed must preserve the last known active baseline",
    );
    frames.send_replace(Some(Arc::new(ClashConnectionsFrame {
        instance_id: "core-process-1".into(),
        snapshot: ClashWsConnectionSnapshot {
            upload_total: 120,
            download_total: 340,
            upload_speed: 0,
            download_speed: 0,
            memory: None,
            connections: Some(Vec::new()),
        },
    })));
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if traffic
                .active_connection_ids(Vec::new())
                .await
                .unwrap()
                .is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("disconnect should close the observed connection");
    traffic.flush().await.unwrap();

    let closed = traffic
        .closed_connections(TrafficRange::All, active_filter, None::<ClosedCursor>, 10)
        .await
        .unwrap();
    assert_eq!(closed.connections.len(), 1);
    assert_eq!(closed.connections[0].dimensions.target, "example.org");
    assert_eq!(
        closed.connections[0].dimensions.profile.as_deref(),
        Some("profile-one")
    );
    assert_eq!(closed.connections[0].bytes.upload, 12);
    assert_eq!(closed.connections[0].bytes.download, 34);
}
