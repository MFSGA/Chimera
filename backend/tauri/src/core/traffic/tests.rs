use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chimera_geodata::IpIndex;
use chimera_traffic::{
    ClosedCursor, Dimension, Metric, RedbTrafficStore, ReportRequest, TrafficFilter, TrafficQuery,
    TrafficRange, TrafficScope,
};
use mockall::Sequence;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    Clock, LocalSourceIps, LocalSourceLocation, ProfileSelection, RetentionPolicy, TrafficArgs,
    TrafficClient,
};
use crate::core::clash::ws::{ClashConnectionsFrame, ClashWsConnectionSnapshot};
use crate::core::geo::{
    CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexArgs, GeoIndexClient,
    GeodataMode, IndexKey, Loaded, MockCountryIndexSource,
};

struct WaitingCore;

#[async_trait::async_trait]
impl CoreGeodataModeSource for WaitingCore {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        std::future::pending().await
    }
}

fn binding(instance_id: &str, mode: GeodataMode) -> CoreGeodataBinding {
    CoreGeodataBinding {
        instance_id: instance_id.into(),
        mode,
    }
}

struct FrameModeSource(AtomicUsize);

impl FrameModeSource {
    fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl CoreGeodataModeSource for FrameModeSource {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(binding("core-instance-1", GeodataMode::Dat)),
            _ => Ok(binding("core-instance-2", GeodataMode::Mmdb)),
        }
    }
}

struct RetryingFrameModeSource(AtomicUsize);

impl RetryingFrameModeSource {
    fn calls(&self) -> usize {
        self.0.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl CoreGeodataModeSource for RetryingFrameModeSource {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => Ok(binding("core-instance-1", GeodataMode::Dat)),
            1 => anyhow::bail!("controller is restarting"),
            2.. => Ok(binding("core-instance-2", GeodataMode::Dat)),
        }
    }
}

fn index_key(mode: GeodataMode, byte: u8) -> IndexKey {
    IndexKey {
        mode,
        sha256: [byte; 32],
    }
}

fn country_index() -> Arc<IpIndex> {
    country_index_for("US")
}

fn country_index_for(country: &str) -> Arc<IpIndex> {
    Arc::new(
        IpIndex::from_geoip_dat(&crate::core::geo::fixtures::geoip_dat(&[(
            country,
            &["8.0.0.0/8"],
        )]))
        .unwrap(),
    )
}

fn frame(instance_id: &str, connection_id: &str) -> ClashConnectionsFrame {
    ClashConnectionsFrame {
        instance_id: instance_id.into(),
        core_instance_id: instance_id.into(),
        snapshot: ClashWsConnectionSnapshot {
            download_total: 0,
            upload_total: 0,
            download_speed: 0,
            upload_speed: 0,
            memory: None,
            connections: Some(vec![serde_json::json!({
                "id": connection_id,
                "metadata": {"network": "tcp", "host": "example.test"},
                "upload": 0,
                "download": 0,
                "start": "2026-10-19T00:00:00Z",
                "chains": ["Proxy"],
                "rule": "MATCH",
                "rulePayload": "Proxy"
            })]),
        },
    }
}

fn frame_with_geo(instance_id: &str, connection_id: &str) -> ClashConnectionsFrame {
    let mut frame = frame(instance_id, connection_id);
    let metadata = &mut frame.snapshot.connections.as_mut().unwrap()[0]["metadata"];
    metadata["sourceIP"] = serde_json::json!("8.8.8.8");
    metadata["sourceGeoIP"] = serde_json::json!(["ca"]);
    frame
}

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
    let (_, geo) = tokio::sync::watch::channel(None::<Arc<CountryIndexSnapshot>>);
    let started_at = chrono::Utc::now().to_rfc3339();
    let traffic = TrafficClient::spawn(TrafficArgs {
        store,
        profiles: Arc::new(TestProfile),
        retention: Arc::new(SevenDayRetention),
        clock: Arc::new(SystemClock),
        frames: receiver,
        geo,
        geo_mode_source: None,
        geo_mode_refresh: None,
        local_source: Arc::new(NoLocalSource),
    })
    .await
    .unwrap();

    frames.send_replace(Some(Arc::new(ClashConnectionsFrame {
        instance_id: "core-process-1".into(),
        core_instance_id: "core-process-1".into(),
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
        core_instance_id: "core-process-1".into(),
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

#[tokio::test]
async fn a_new_core_frame_is_recorded_only_after_its_country_mode_is_loaded() {
    let mut index_source = MockCountryIndexSource::new();
    index_source.expect_watch().returning(|_| Ok(Box::new(())));
    let mut sequence = Sequence::new();
    let first_index = country_index();
    let first_loaded = first_index.clone();
    index_source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: index_key(GeodataMode::Dat, 1),
                index: first_loaded.clone(),
            })
        });
    let second_loaded = country_index_for("JP");
    let second_index = second_loaded.clone();
    index_source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Mmdb && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: index_key(GeodataMode::Mmdb, 2),
                index: second_loaded.clone(),
            })
        });

    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let geo_index = GeoIndexClient::spawn(
        GeoIndexArgs {
            source: Arc::new(index_source),
            mode_source: Arc::new(WaitingCore),
        },
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let frame_modes = Arc::new(FrameModeSource(AtomicUsize::new(0)));
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap());
    let (frames, receiver) = tokio::sync::watch::channel(None);
    let traffic = TrafficClient::spawn(TrafficArgs {
        store,
        profiles: Arc::new(TestProfile),
        retention: Arc::new(SevenDayRetention),
        clock: Arc::new(SystemClock),
        frames: receiver,
        geo: geo_index.subscribe(),
        geo_mode_source: Some(frame_modes.clone()),
        geo_mode_refresh: Some(geo_index.clone()),
        local_source: Arc::new(NoLocalSource),
    })
    .await
    .unwrap();

    frames.send_replace(Some(Arc::new(frame(
        "core-instance-1",
        "00000000-0000-0000-0000-000000000001",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000001").await;
    assert_eq!(frame_modes.calls(), 1);
    assert!(Arc::ptr_eq(
        &geo_index.subscribe().borrow().as_ref().unwrap().index,
        &first_index
    ));

    frames.send_replace(Some(Arc::new(frame(
        "core-instance-2",
        "00000000-0000-0000-0000-000000000002",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000002").await;
    assert_eq!(frame_modes.calls(), 2);
    assert!(Arc::ptr_eq(
        &geo_index.subscribe().borrow().as_ref().unwrap().index,
        &second_index
    ));

    // A late frame from core-instance-1 must not use core-instance-2's index.
    frames.send_replace(Some(Arc::new(frame_with_geo(
        "core-instance-1",
        "00000000-0000-0000-0000-000000000003",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000003").await;
    assert_eq!(frame_modes.calls(), 3);
    assert_eq!(report_source_regions(&traffic).await, vec!["CA"]);

    traffic.stop_for_test().await.unwrap();
    drop(geo_index);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn failed_new_core_mode_read_never_uses_old_geo_index_and_recovers_on_retry() {
    let mut index_source = MockCountryIndexSource::new();
    index_source.expect_watch().returning(|_| Ok(Box::new(())));
    let mut sequence = Sequence::new();
    let first_loaded = country_index();
    let first_index = first_loaded.clone();
    index_source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: index_key(GeodataMode::Dat, 1),
                index: first_loaded.clone(),
            })
        });
    let recovered = country_index_for("JP");
    let recovered_index = recovered.clone();
    index_source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: index_key(GeodataMode::Dat, 2),
                index: recovered_index.clone(),
            })
        });

    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let geo_index = GeoIndexClient::spawn(
        GeoIndexArgs {
            source: Arc::new(index_source),
            mode_source: Arc::new(WaitingCore),
        },
        shutdown.clone(),
        &tasks,
    )
    .await
    .unwrap();
    let frame_modes = Arc::new(RetryingFrameModeSource(AtomicUsize::new(0)));
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(RedbTrafficStore::open(&dir.path().join("traffic.redb")).unwrap());
    let (frames, receiver) = tokio::sync::watch::channel(None);
    let traffic = TrafficClient::spawn(TrafficArgs {
        store,
        profiles: Arc::new(TestProfile),
        retention: Arc::new(SevenDayRetention),
        clock: Arc::new(SystemClock),
        frames: receiver,
        geo: geo_index.subscribe(),
        geo_mode_source: Some(frame_modes.clone()),
        geo_mode_refresh: Some(geo_index.clone()),
        local_source: Arc::new(NoLocalSource),
    })
    .await
    .unwrap();

    frames.send_replace(Some(Arc::new(frame_with_geo(
        "core-instance-1",
        "00000000-0000-0000-0000-000000000011",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000011").await;
    assert_eq!(report_source_regions(&traffic).await, vec!["US"]);
    assert!(Arc::ptr_eq(
        &geo_index.subscribe().borrow().as_ref().unwrap().index,
        &first_index
    ));

    frames.send_replace(Some(Arc::new(frame_with_geo(
        "core-instance-2",
        "00000000-0000-0000-0000-000000000012",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000012").await;
    assert_eq!(report_source_regions(&traffic).await, vec!["CA"]);
    assert!(Arc::ptr_eq(
        &geo_index.subscribe().borrow().as_ref().unwrap().index,
        &first_index
    ));

    tokio::time::sleep(Duration::from_secs(1)).await;
    frames.send_replace(Some(Arc::new(frame_with_geo(
        "core-instance-2",
        "00000000-0000-0000-0000-000000000013",
    ))));
    wait_for_active_id(&traffic, "00000000-0000-0000-0000-000000000013").await;
    assert_eq!(frame_modes.calls(), 3);
    assert_eq!(report_source_regions(&traffic).await, vec!["JP"]);
    assert!(Arc::ptr_eq(
        &geo_index.subscribe().borrow().as_ref().unwrap().index,
        &recovered
    ));

    traffic.stop_for_test().await.unwrap();
    drop(geo_index);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

async fn report_source_regions(traffic: &TrafficClient) -> Vec<String> {
    traffic
        .report(ReportRequest {
            query: TrafficQuery {
                range: TrafficRange::All,
                scope: TrafficScope::Active,
                filters: Vec::new(),
            },
            metric: Metric::Connections,
            rankings: vec![Dimension::SourceRegion],
            ranking_limit: 10,
            topology: None,
        })
        .await
        .unwrap()
        .rankings[0]
        .groups
        .iter()
        .map(|group| group.key.clone())
        .collect()
}

async fn wait_for_active_id(traffic: &TrafficClient, id: &str) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if traffic.active_connection_ids(Vec::new()).await.unwrap() == [id] {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the latest core frame should reach traffic accounting");
}
