use std::{future::pending, sync::Arc, time::Duration};

use chimera_geodata::IpIndex;
use mockall::Sequence;
use tokio::sync::watch;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::{
    GeoIndexArgs, GeoIndexClient, MockCountryIndexSource,
    actor::Message,
    fixtures::geoip_dat,
    ports::{
        CoreGeodataBinding, CoreGeodataModeSource, CountryIndexSnapshot, GeoIndexError,
        GeodataMode, IndexKey, Loaded,
    },
};

struct WaitingCore;

#[async_trait::async_trait]
impl CoreGeodataModeSource for WaitingCore {
    async fn mode(&self) -> anyhow::Result<CoreGeodataBinding> {
        pending().await
    }
}

fn binding(instance_id: &str, mode: GeodataMode) -> CoreGeodataBinding {
    CoreGeodataBinding {
        instance_id: instance_id.into(),
        mode,
    }
}

fn test_index() -> Arc<IpIndex> {
    Arc::new(IpIndex::from_geoip_dat(&geoip_dat(&[("US", &["8.0.0.0/8"])])).unwrap())
}

fn key(byte: u8) -> IndexKey {
    key_for(GeodataMode::Dat, byte)
}

fn key_for(mode: GeodataMode, byte: u8) -> IndexKey {
    IndexKey {
        mode,
        sha256: [byte; 32],
    }
}

/// A source whose watch succeeds; the core never answers, so tests report the mode themselves.
fn source() -> MockCountryIndexSource {
    let mut source = MockCountryIndexSource::new();
    source.expect_watch().returning(|_| Ok(Box::new(())));
    source
}

async fn spawn(source: MockCountryIndexSource) -> GeoIndexClient {
    GeoIndexClient::spawn(
        GeoIndexArgs {
            source: Arc::new(source),
            mode_source: Arc::new(WaitingCore),
        },
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap()
}

async fn next(
    published: &mut watch::Receiver<Option<Arc<CountryIndexSnapshot>>>,
) -> Option<Arc<CountryIndexSnapshot>> {
    tokio::time::timeout(Duration::from_secs(5), published.changed())
        .await
        .expect("an index is published")
        .unwrap();
    published.borrow_and_update().clone()
}

#[tokio::test]
async fn a_reported_mode_loads_and_publishes_its_index() {
    let index = test_index();
    let mut source = source();
    let published_index = index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: published_index.clone(),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::Mode(binding("core-one", GeodataMode::Dat)));

    let snapshot = next(&mut published).await.unwrap();
    assert_eq!(snapshot.instance_id, "core-one");
    assert!(Arc::ptr_eq(&snapshot.index, &index));
}

#[tokio::test]
async fn a_fallback_refresh_reloads_the_current_mode_after_a_missed_write() {
    let first_index = test_index();
    let second_index = test_index();
    let mut sequence = Sequence::new();
    let mut source = source();
    let initial = first_index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: initial.clone(),
            })
        });
    let refreshed = second_index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && *current == Some(key(1)))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(2),
                index: refreshed.clone(),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::Mode(binding("core-one", GeodataMode::Dat)));
    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &first_index
    ));

    // Frequent core polls are deduplicated; the periodic fallback still reloads the same mode.
    client.send(Message::Mode(binding("core-one", GeodataMode::Dat)));
    client.send(Message::Refresh);

    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &second_index
    ));
}

#[tokio::test]
async fn changing_core_mode_drops_the_incompatible_index_when_reload_fails() {
    let index = test_index();
    let mut sequence = Sequence::new();
    let mut source = source();
    let published_index = index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: published_index.clone(),
            })
        });
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Mmdb && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Err(GeoIndexError::ListHome {
                source: std::io::Error::other("temporarily unavailable"),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client
        .set_mode(binding("core-dat", GeodataMode::Dat))
        .await
        .unwrap();
    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &index
    ));

    client
        .set_mode(binding("core-mmdb", GeodataMode::Mmdb))
        .await
        .unwrap();

    assert!(next(&mut published).await.is_none());
}

#[tokio::test]
async fn rebinding_the_same_core_mode_drops_the_prior_index_when_reload_fails() {
    let index = test_index();
    let mut sequence = Sequence::new();
    let mut source = source();
    let published_index = index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: published_index.clone(),
            })
        });
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Err(GeoIndexError::ListHome {
                source: std::io::Error::other("temporarily unavailable"),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client
        .set_mode(binding("core-before", GeodataMode::Dat))
        .await
        .unwrap();
    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &index
    ));

    client
        .set_mode(binding("core-after", GeodataMode::Dat))
        .await
        .unwrap();

    assert!(next(&mut published).await.is_none());
}

#[tokio::test]
async fn retrying_a_bound_core_mode_does_not_reload_its_index() {
    let index = test_index();
    let mut source = source();
    let published_index = index.clone();
    source
        .expect_load()
        .withf(|mode, current| *mode == GeodataMode::Dat && current.is_none())
        .times(1)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: published_index.clone(),
            })
        });
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client
        .set_mode(binding("core-one", GeodataMode::Dat))
        .await
        .unwrap();
    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &index
    ));

    client
        .set_mode(binding("core-one", GeodataMode::Dat))
        .await
        .unwrap();
}

#[tokio::test]
async fn a_written_database_waits_for_the_core_s_mode() {
    let mut source = source();
    // Only the mode's message loads; the earlier change has no database to read yet.
    source
        .expect_load()
        .withf(|mode, _| *mode == GeodataMode::Mmdb)
        .times(1)
        .returning(|_, _| Ok(Loaded::Missing));
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::FilesChanged);
    client.send(Message::Mode(binding("core-one", GeodataMode::Mmdb)));

    assert!(next(&mut published).await.is_none());
}

#[tokio::test]
async fn a_failed_reload_keeps_the_index_and_its_key() {
    let index = test_index();
    let mut sequence = Sequence::new();
    let mut source = source();
    let first = index.clone();
    source
        .expect_load()
        .times(1)
        .in_sequence(&mut sequence)
        .returning(move |_, _| {
            Ok(Loaded::Index {
                key: key(1),
                index: first.clone(),
            })
        });
    source
        .expect_load()
        .withf(|_, current| *current == Some(key(1)))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| {
            Err(GeoIndexError::ListHome {
                source: std::io::Error::other("denied"),
            })
        });
    // Still handed the first key: the failure did not forget what is published.
    source
        .expect_load()
        .withf(|_, current| *current == Some(key(1)))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_, _| Ok(Loaded::Missing));
    let client = spawn(source).await;
    let mut published = client.subscribe();

    client.send(Message::Mode(binding("core-one", GeodataMode::Dat)));
    assert!(Arc::ptr_eq(
        &next(&mut published).await.unwrap().index,
        &index
    ));
    client.send(Message::FilesChanged);
    client.send(Message::FilesChanged);

    assert!(next(&mut published).await.is_none());
}
