use crate::utils::dirs;
use anyhow::Context;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use specta::Type;
use std::{fs, ops::Deref, result::Result as StdResult, sync::Arc};
use tauri::{Emitter, Manager};
use tauri_specta::Event;
use tokio::sync::broadcast::{self, Receiver, Sender, error::RecvError};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use parking_lot::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum StorageOperationError {
    #[error("failed to open database: {0}")]
    OpenDatabase(#[from] redb::DatabaseError),
    #[error("internal redb error: {0}")]
    Redb(#[from] redb::Error),
    #[error("internal redb table error: {0}")]
    RedbTable(#[from] redb::TableError),
    #[error("internal redb storage error: {0}")]
    RedbStorage(#[from] redb::StorageError),
    #[error("failed to start transaction: {0}")]
    RedbTransaction(#[from] redb::TransactionError),
    #[error("failed to commit transaction: {0}")]
    RedbCommit(#[from] redb::CommitError),
    #[error("failed to serialize or deserialize data: {0}")]
    Serialize(#[from] serde_json::Error),
}

pub const NYANPASU_TABLE: TableDefinition<&[u8], &[u8]> = TableDefinition::new("clash-nyanpasu");

type Result<T> = StdResult<T, StorageOperationError>;

/// storage is a wrapper or called a facade for the rocksdb
/// Maybe provide a facade for a kv storage is a good idea?
#[derive(Clone)]
pub struct Storage {
    inner: Arc<StorageInner>,
}

impl Storage {
    pub fn try_new(path: &std::path::Path) -> Result<Self> {
        let inner = StorageInner::try_new(path)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }
}

impl Deref for Storage {
    type Target = Arc<StorageInner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

pub struct StorageInner {
    instance: redb::Database,
    /// Orders committed values and their notifications. Reads hold the same
    /// lock while observing the database so each snapshot has one revision.
    revision: Mutex<StorageRevision>,
    tx: Sender<StorageChange>,
    #[cfg(test)]
    before_publish: Option<Box<dyn Fn(&str) + Send + Sync>>,
}

/// Orders every commit made while this application process has the store open.
/// Revisions restart at zero when the store is reopened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct StorageRevision(u64);

impl StorageRevision {
    fn next(self) -> Self {
        Self(
            self.0
                .checked_add(1)
                .expect("an open store never commits u64::MAX times"),
        )
    }
}

/// A stored value, or `None` when its key is absent, as of `revision`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct StorageItem<T> {
    pub value: Option<T>,
    pub revision: StorageRevision,
}

/// A committed change in publication order.
#[derive(Debug, Clone)]
pub struct StorageChange {
    pub key: String,
    pub value: Option<Vec<u8>>,
    pub revision: StorageRevision,
}

#[derive(Debug)]
pub enum StorageNotification {
    Changed(StorageChange),
    Resync,
}

/// Emitted after the change stream falls behind. Consumers should reread keys.
/// Event name: `storage-resync-event`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageResyncEvent {}

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
    pub revision: StorageRevision,
}

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<StorageItem<T>>;
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<StorageRevision>;
    fn remove_item(&self, key: impl AsRef<str>) -> Result<StorageRevision>;
    /// Returns all key-value pairs as raw JSON strings (for debug use).
    fn get_all(&self) -> Result<Vec<(String, String)>>;
}

impl StorageInner {
    fn create_and_init_database(path: &std::path::Path) -> Result<redb::Database> {
        let db = redb::Database::create(path)?;
        // Create table
        let write_txn = db.begin_write()?;
        write_txn.open_table(NYANPASU_TABLE)?;
        write_txn.commit()?;
        Ok(db)
    }

    pub fn try_new(path: &std::path::Path) -> Result<Self> {
        let metadata = fs::metadata(path).ok();
        let instance: redb::Database = if metadata.as_ref().is_some_and(|m| m.is_file()) {
            match redb::Database::open(path) {
                Ok(db) => db,
                // In redb v3 upgrading point, we only store the task history, and frontend persist state,
                // such as memorized router, which is NOT very valuable to make us keep two redb versions,
                // intended to support upgrade database formats.
                Err(redb::DatabaseError::UpgradeRequired(ver)) => {
                    tracing::error!("database upgrade required {ver:?}, removing...");
                    fs::remove_file(path).unwrap();
                    Self::create_and_init_database(path)?
                }
                Err(e) => return Err(e.into()),
            }
        } else {
            // Remove previous rocksdb files
            if metadata.is_some_and(|m| m.is_dir()) {
                fs::remove_dir_all(path).unwrap();
            }
            Self::create_and_init_database(path)?
        };
        Ok(Self {
            instance,
            revision: Mutex::new(StorageRevision(0)),
            tx: broadcast::channel(16).0,
            #[cfg(test)]
            before_publish: None,
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }

    /// Reads the current key, falling back to a legacy key within the same
    /// database snapshot. Both values and the returned revision are atomic.
    pub fn get_item_with_fallback<T: DeserializeOwned>(
        &self,
        key: impl AsRef<str>,
        fallback_key: impl AsRef<str>,
    ) -> Result<StorageItem<T>> {
        let revision = self.revision.lock();
        let read_txn = self.instance.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let value = match table.get(key.as_ref().as_bytes())? {
            Some(value) => Some(serde_json::from_slice(value.value())?),
            None => match table.get(fallback_key.as_ref().as_bytes())? {
                Some(value) => Some(serde_json::from_slice(value.value())?),
                None => None,
            },
        };
        Ok(StorageItem {
            value,
            revision: *revision,
        })
    }

    pub fn subscribe(&self) -> Receiver<StorageChange> {
        self.tx.subscribe()
    }

    /// Writes a value or removes its key, then publishes the committed change
    /// before releasing the lock that orders revisions.
    fn commit(&self, key: &str, value: Option<Vec<u8>>) -> Result<StorageRevision> {
        let mut revision = self.revision.lock();
        let write_txn = self.instance.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            match &value {
                Some(value) => {
                    table.insert(key.as_bytes(), value.as_slice())?;
                }
                None => {
                    table.remove(key.as_bytes())?;
                }
            }
        }
        write_txn.commit()?;
        *revision = revision.next();
        #[cfg(test)]
        if let Some(before_publish) = &self.before_publish {
            before_publish(key);
        }
        let change = StorageChange {
            key: key.to_owned(),
            value,
            revision: *revision,
        };
        // No subscriber is a normal state; the database commit already succeeded.
        let _ = self.tx.send(change);
        Ok(*revision)
    }
}

impl WebStorage for StorageInner {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<StorageItem<T>> {
        let key = key.as_ref();
        let revision = self.revision.lock();
        let db = self.get_instance();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let value = match table.get(key.as_bytes())? {
            Some(value) => {
                let value = value.value();
                Some(serde_json::from_slice(value)?)
            }
            None => None,
        };
        Ok(StorageItem {
            value,
            revision: *revision,
        })
    }

    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<StorageRevision> {
        let key = key.as_ref();
        self.commit(key, Some(serde_json::to_vec(value)?))
    }

    fn remove_item(&self, key: impl AsRef<str>) -> Result<StorageRevision> {
        self.commit(key.as_ref(), None)
    }

    fn get_all(&self) -> Result<Vec<(String, String)>> {
        let db = self.get_instance();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let mut result = Vec::new();
        for entry in table.iter()? {
            let (key, value) = entry?;
            let key = String::from_utf8_lossy(key.value()).to_string();
            let value = String::from_utf8_lossy(value.value()).to_string();
            result.push((key, value));
        }
        Ok(result)
    }
}

pub async fn forward_storage_changes(
    mut rx: Receiver<StorageChange>,
    mut emit: impl FnMut(StorageNotification),
) {
    loop {
        match rx.recv().await {
            Ok(change) => emit(StorageNotification::Changed(change)),
            Err(RecvError::Lagged(_)) => emit(StorageNotification::Resync),
            Err(RecvError::Closed) => break,
        }
    }
}

pub fn register_web_storage_listener(
    app_handle: &tauri::AppHandle,
    shutdown: CancellationToken,
    tasks: TaskTracker,
) {
    let storage = app_handle.state::<Storage>();
    let rx = storage.subscribe();
    let app_handle = app_handle.clone();
    let runtime_handle = tauri::async_runtime::handle();
    tasks.spawn_on(
        async move {
            tokio::select! {
                _ = shutdown.cancelled() => {},
                _ = forward_storage_changes(rx, move |notification| {
                    match notification {
                        StorageNotification::Changed(change) => {
                            let value = change.value.map(|value| String::from_utf8_lossy(&value).into_owned());
                            let event = StorageValueChangedEvent {
                                key: change.key.clone(),
                                value: value.clone(),
                                revision: change.revision,
                            };
                            if let Err(error) = event.emit(&app_handle) {
                                tracing::warn!(%error, "failed to emit typed storage value changed event");
                            }
                            // Keep the tuple event while the legacy storage subscribers are mounted.
                            if let Err(error) = app_handle.emit("storage_value_changed", (change.key, value)) {
                                tracing::warn!(%error, "failed to emit legacy storage value changed event");
                            }
                        }
                        StorageNotification::Resync => {
                            if let Err(error) = (StorageResyncEvent {}).emit(&app_handle) {
                                tracing::warn!(%error, "failed to emit typed storage resync event");
                            }
                            // The legacy listener treats any non-negative count as a resync request.
                            if let Err(error) = app_handle.emit("storage_resync_required", 0_u64) {
                                tracing::warn!(%error, "failed to emit legacy storage resync event");
                            }
                        }
                    }
                }) => {},
            }
        },
        runtime_handle.inner(),
    );
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> anyhow::Result<()> {
    let storage_path = dirs::storage_path().context("failed to get storage path")?;
    let storage = Storage::try_new(&storage_path)?;
    app.manage(storage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Barrier, thread};

    fn open(dir: &tempfile::TempDir) -> StorageInner {
        StorageInner::try_new(&dir.path().join("storage.redb")).expect("storage should open")
    }

    #[tokio::test]
    async fn lag_emits_resync_and_forwarding_continues() {
        let (tx, rx) = broadcast::channel(16);
        for index in 0..17 {
            tx.send(StorageChange {
                key: format!("web:{index}"),
                value: None,
                revision: StorageRevision(index + 1),
            })
            .unwrap();
        }
        drop(tx);

        let mut notifications = Vec::new();
        forward_storage_changes(rx, |notification| notifications.push(notification)).await;

        assert!(matches!(notifications[0], StorageNotification::Resync));
        let keys: Vec<_> = notifications[1..]
            .iter()
            .map(|notification| match notification {
                StorageNotification::Changed(change) => change.key.clone(),
                StorageNotification::Resync => panic!("only the first receive lags"),
            })
            .collect();
        let expected: Vec<_> = (1..17).map(|index| format!("web:{index}")).collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn a_change_is_published_before_the_write_returns() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let mut rx = storage.subscribe();

        let written = storage.set_item("web:key", &"value").unwrap();
        let changed = rx.try_recv().unwrap();
        assert_eq!(changed.revision, written);
        assert_eq!(changed.key, "web:key");
        assert_eq!(changed.value.as_deref(), Some(br#""value""#.as_slice()));

        let removal = storage.remove_item("web:key").unwrap();
        let removed = rx.try_recv().unwrap();
        assert_eq!(removed.revision, removal);
        assert_eq!(removed.key, "web:key");
        assert_eq!(removed.value, None);
    }

    #[test]
    fn a_commit_holds_write_order_until_its_change_is_published() {
        let dir = tempfile::tempdir().unwrap();
        let mut storage = open(&dir);
        let committed = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        storage.before_publish = Some(Box::new({
            let committed = committed.clone();
            let resume = resume.clone();
            move |key| {
                if key == "web:a" {
                    committed.wait();
                    resume.wait();
                }
            }
        }));
        let mut rx = storage.subscribe();

        let (held, first, second) = thread::scope(|scope| {
            let first = scope.spawn(|| storage.set_item("web:a", &1).unwrap());
            committed.wait();
            let held = storage.revision.try_lock().is_none();
            resume.wait();
            let second = storage.set_item("web:b", &2).unwrap();
            (held, first.join().unwrap(), second)
        });

        assert!(held, "the revision lock must span publication");
        let published: Vec<_> = [rx.try_recv().unwrap(), rx.try_recv().unwrap()]
            .into_iter()
            .map(|change| (change.key, change.revision))
            .collect();
        assert_eq!(
            published,
            [("web:a".to_owned(), first), ("web:b".to_owned(), second)]
        );
        assert!(first < second);
    }

    #[test]
    fn every_commit_advances_the_revision_and_reads_report_it() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let mut rx = storage.subscribe();
        let empty = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(empty.value, None);
        assert_eq!(empty.revision, StorageRevision(0));

        let revisions = [
            storage.set_item("web:key", &1).unwrap(),
            storage.set_item("web:key", &1).unwrap(),
            storage.remove_item("web:key").unwrap(),
            storage.remove_item("web:key").unwrap(),
            storage.set_item("internal", &2).unwrap(),
        ];
        assert!(revisions.windows(2).all(|pair| pair[0] < pair[1]));
        for revision in revisions {
            assert_eq!(rx.try_recv().unwrap().revision, revision);
        }

        let item = storage.get_item::<i32>("web:key").unwrap();
        assert_eq!(item.value, None);
        assert_eq!(item.revision, revisions[4]);
    }

    #[test]
    fn a_failed_encode_does_not_advance_or_publish_a_commit() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let written = storage.set_item("web:key", &"kept").unwrap();
        let mut rx = storage.subscribe();
        let invalid = BTreeMap::from([((1_u8, 2_u8), 3_u8)]);

        assert!(matches!(
            storage.set_item("web:key", &invalid),
            Err(StorageOperationError::Serialize(_))
        ));
        let item = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(item.value.as_deref(), Some("kept"));
        assert_eq!(item.revision, written);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn reopening_preserves_values_and_restarts_revision() {
        let dir = tempfile::tempdir().unwrap();
        open(&dir).set_item("web:key", &"saved").unwrap();

        let item = open(&dir).get_item::<String>("web:key").unwrap();

        assert_eq!(item.value.as_deref(), Some("saved"));
        assert_eq!(item.revision, StorageRevision(0));
    }

    #[test]
    fn a_namespaced_value_takes_precedence_over_legacy_storage() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        storage.set_item("layout", &"legacy").unwrap();

        let legacy = storage
            .get_item_with_fallback::<String>("web:layout", "layout")
            .unwrap();
        assert_eq!(legacy.value.as_deref(), Some("legacy"));

        storage.set_item("web:layout", &"current").unwrap();
        let current = storage
            .get_item_with_fallback::<String>("web:layout", "layout")
            .unwrap();
        assert_eq!(current.value.as_deref(), Some("current"));
        assert!(current.revision > legacy.revision);
    }
}
