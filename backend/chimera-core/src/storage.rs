//! Shared redb storage owner and publication-order notifications, aligned with ref.

use crate::redb_ext::{RedbFailure, find_failure};
use chimera_contracts::{
    BoxedCause,
    storage::{HotkeyStore, HotkeyStoreOpener, StorageSnapshot},
};
use chimera_utils::io::IoFailure;
use parking_lot::Mutex;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Serialize, de::DeserializeOwned};
use std::{fs, ops::Deref, path::Path, result::Result as StdResult, sync::Arc};
use tokio::sync::broadcast::{self, Receiver, Sender, error::RecvError};

#[derive(Debug, thiserror::Error)]
pub enum StorageOperationError {
    #[error("failed to open database: {0}")]
    OpenDatabase(#[from] redb::DatabaseError),
    #[error("failed to inspect the database at {path}")]
    InspectDatabase {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to preserve unusable database")]
    PreserveDatabase(#[source] chimera_persistence::Error),
    #[error("snapshot destination already exists: {}", .0.display())]
    SnapshotDestinationExists(std::path::PathBuf),
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
    #[error("storage revision conflict: expected {expected:?}, current {current:?}")]
    RevisionConflict {
        expected: StorageRevision,
        current: StorageRevision,
    },
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

/// Export one consistent read transaction, without copying an open redb file.
impl Storage {
    pub fn export_to(&self, destination: &Path) -> Result<()> {
        // A snapshot must never silently overwrite an existing backup artifact.
        match chimera_persistence::retry_io(|| fs::symlink_metadata(destination)) {
            Ok(_) => {
                return Err(StorageOperationError::SnapshotDestinationExists(
                    destination.to_owned(),
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(StorageOperationError::InspectDatabase {
                    path: destination.display().to_string(),
                    source,
                });
            }
        }
        let read_txn = self.inner.instance.begin_read()?;
        let source = read_txn.open_table(NYANPASU_TABLE)?;
        let snapshot = StorageInner::create_and_init_database(destination)?;
        let write_txn = snapshot.begin_write()?;
        {
            let mut dest_table = write_txn.open_table(NYANPASU_TABLE)?;
            for entry in source.iter()? {
                let (key, value) = entry?;
                dest_table.insert(key.value(), value.value())?;
            }
        }
        write_txn.commit()?;
        Ok(())
    }
}

impl StorageSnapshot for Storage {
    fn export_to(&self, destination: &Path) -> StdResult<(), BoxedCause> {
        Storage::export_to(self, destination).map_err(BoxedCause::from)
    }
}

const HOTKEYS_KEY: &str = "hotkeys";

impl HotkeyStore for Storage {
    fn read(&self) -> StdResult<Option<Vec<String>>, BoxedCause> {
        self.get_item::<Vec<String>>(HOTKEYS_KEY)
            .map(|item| item.value)
            .map_err(BoxedCause::from)
    }
    fn write(&self, hotkeys: &[String]) -> StdResult<(), BoxedCause> {
        self.set_item(HOTKEYS_KEY, &hotkeys)
            .map(|_| ())
            .map_err(BoxedCause::from)
    }
    fn remove(&self) -> StdResult<(), BoxedCause> {
        self.remove_item(HOTKEYS_KEY)
            .map(|_| ())
            .map_err(BoxedCause::from)
    }
}

#[derive(Debug)]
pub struct RedbHotkeyStoreOpener;

impl HotkeyStoreOpener for RedbHotkeyStoreOpener {
    fn open(&self, path: &Path) -> StdResult<Box<dyn HotkeyStore>, BoxedCause> {
        let storage = Storage::try_new(path).map_err(BoxedCause::from)?;
        Ok(Box::new(storage))
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

pub use chimera_contracts::storage::{StorageItem, StorageRevision};

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

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<StorageItem<T>>;
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<StorageRevision>;
    fn remove_item(&self, key: impl AsRef<str>) -> Result<StorageRevision>;
    /// Returns all key-value pairs as raw JSON strings (for debug use).
    fn get_all(&self) -> Result<Vec<(String, String)>>;
}

fn transient_database_error(error: &(dyn std::error::Error + 'static)) -> bool {
    matches!(
        find_failure(error),
        Some(RedbFailure::AlreadyOpen | RedbFailure::Io(IoFailure::Transient))
    )
}

fn confirmed_database_problem(error: &StorageOperationError) -> bool {
    matches!(
        find_failure(error),
        Some(RedbFailure::Corrupt | RedbFailure::Incompatible | RedbFailure::MissingTable)
    )
}

fn preserve_database(path: &Path, error: &(dyn std::error::Error + 'static)) -> Result<()> {
    let evidence = chimera_persistence::preserve(path, error)
        .map_err(StorageOperationError::PreserveDatabase)?;
    tracing::warn!(original_path = %path.display(), evidence_path = %evidence.display(),
        error = %error, action = "reset_key_value_storage",
        "preserved unusable key-value storage before rebuilding; stored UI state may be reset");
    Ok(())
}

impl StorageInner {
    fn create_and_init_database(path: &Path) -> Result<redb::Database> {
        let db = chimera_persistence::retry(
            || redb::Database::create(path),
            |error| transient_database_error(error),
        )?;
        let write_txn = db.begin_write()?;
        write_txn.open_table(NYANPASU_TABLE)?;
        write_txn.commit()?;
        Ok(db)
    }

    pub fn try_new(path: &Path) -> Result<Self> {
        Self::try_new_with_open(path, || Self::open_database(path), std::thread::sleep)
    }

    fn try_new_with_open(
        path: &Path,
        open: impl FnMut() -> Result<redb::Database>,
        wait: impl Fn(std::time::Duration) + 'static,
    ) -> Result<Self> {
        let metadata = match chimera_persistence::retry_io(|| fs::symlink_metadata(path)) {
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(StorageOperationError::InspectDatabase {
                    path: path.display().to_string(),
                    source,
                });
            }
        };
        let instance = if let Some(metadata) = metadata {
            if metadata.is_file() && !chimera_persistence::is_link(&metadata) {
                match chimera_persistence::retry_with_delay(
                    open,
                    |error| transient_database_error(error),
                    wait,
                ) {
                    Ok(db) => db,
                    Err(error) if confirmed_database_problem(&error) => {
                        preserve_database(path, &error)?;
                        Self::create_and_init_database(path)?
                    }
                    Err(error) => return Err(error),
                }
            } else {
                let error = std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "database path is not a regular file",
                );
                preserve_database(path, &error)?;
                Self::create_and_init_database(path)?
            }
        } else {
            Self::create_and_init_database(path)?
        };
        Ok(Self {
            instance,
            revision: Mutex::new(StorageRevision::INITIAL),
            tx: broadcast::channel(16).0,
            #[cfg(test)]
            before_publish: None,
        })
    }

    fn open_database(path: &Path) -> Result<redb::Database> {
        let db = redb::Database::open(path)?;
        {
            let read = db.begin_read()?;
            read.open_table(NYANPASU_TABLE)?;
        }
        Ok(db)
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

    /// Removes a namespaced key and its legacy fallback in one database commit.
    /// Only the namespaced key is published because it is the canonical frontend key.
    pub fn remove_item_with_fallback(
        &self,
        key: impl AsRef<str>,
        fallback_key: impl AsRef<str>,
    ) -> Result<StorageRevision> {
        let key = key.as_ref();
        let fallback_key = fallback_key.as_ref();
        if key == fallback_key {
            return self.commit(key, None);
        }

        let mut revision = self.revision.lock();
        let write_txn = self.instance.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.remove(key.as_bytes())?;
            table.remove(fallback_key.as_bytes())?;
        }
        write_txn.commit()?;
        *revision = revision
            .checked_next()
            .expect("an open store never commits u64::MAX times");
        #[cfg(test)]
        if let Some(before_publish) = &self.before_publish {
            before_publish(key);
        }
        let change = StorageChange {
            key: key.to_owned(),
            value: None,
            revision: *revision,
        };
        let _ = self.tx.send(change);
        Ok(*revision)
    }

    pub fn subscribe(&self) -> Receiver<StorageChange> {
        self.tx.subscribe()
    }

    pub fn set_item_if_revision<T: Serialize>(
        &self,
        key: impl AsRef<str>,
        value: &T,
        expected: StorageRevision,
    ) -> Result<StorageRevision> {
        let key = key.as_ref();
        self.commit_if_revision(key, Some(serde_json::to_vec(value)?), expected)
    }

    /// Writes a value or removes its key, then publishes the committed change
    /// before releasing the lock that orders revisions.
    fn commit(&self, key: &str, value: Option<Vec<u8>>) -> Result<StorageRevision> {
        let mut revision = self.revision.lock();
        self.commit_locked(key, value, &mut revision)
    }

    fn commit_if_revision(
        &self,
        key: &str,
        value: Option<Vec<u8>>,
        expected: StorageRevision,
    ) -> Result<StorageRevision> {
        let mut revision = self.revision.lock();
        if *revision != expected {
            return Err(StorageOperationError::RevisionConflict {
                expected,
                current: *revision,
            });
        }
        self.commit_locked(key, value, &mut revision)
    }

    fn commit_locked(
        &self,
        key: &str,
        value: Option<Vec<u8>>,
        revision: &mut StorageRevision,
    ) -> Result<StorageRevision> {
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
        *revision = revision
            .checked_next()
            .expect("an open store never commits u64::MAX times");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, sync::Barrier, thread};

    #[cfg(unix)]
    #[test]
    fn invalid_parent_is_reported_without_resetting_ancestor() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("not-a-directory");
        std::fs::write(&parent, b"keep ancestor").unwrap();
        let path = parent.join("storage.redb");

        let error = StorageInner::try_new(&path)
            .err()
            .expect("invalid path must fail");
        assert!(matches!(
            error,
            StorageOperationError::InspectDatabase { path: ref failed, source }
                if failed == &path.display().to_string()
                    && source.kind() == std::io::ErrorKind::NotADirectory
        ));
        assert_eq!(std::fs::read(&parent).unwrap(), b"keep ancestor");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn database_open_retries_transient_errors_without_touching_existing_data() {
        for failures in [3, 4] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("storage.redb");
            let storage = StorageInner::try_new(&path).unwrap();
            storage.set_item("kept", &42).unwrap();
            drop(storage);
            let mut attempts = 0;
            let delays = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let recorded = delays.clone();
            let result = StorageInner::try_new_with_open(
                &path,
                || {
                    attempts += 1;
                    if attempts <= failures {
                        Err(StorageOperationError::OpenDatabase(
                            redb::DatabaseError::Storage(redb::StorageError::Io(
                                std::io::ErrorKind::TimedOut.into(),
                            )),
                        ))
                    } else {
                        StorageInner::open_database(&path)
                    }
                },
                move |delay| recorded.borrow_mut().push(delay),
            );
            assert_eq!(attempts, 4);
            assert_eq!(
                *delays.borrow(),
                [50, 100, 200].map(std::time::Duration::from_millis)
            );
            if failures == 3 {
                assert_eq!(
                    result.unwrap().get_item::<i32>("kept").unwrap().value,
                    Some(42)
                );
            } else {
                assert!(matches!(
                    result,
                    Err(StorageOperationError::OpenDatabase(_))
                ));
            }
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
            assert_eq!(
                StorageInner::try_new(&path)
                    .unwrap()
                    .get_item::<i32>("kept")
                    .unwrap()
                    .value,
                Some(42)
            );
        }
    }

    #[test]
    fn unconfirmed_database_open_error_preserves_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.redb");
        let storage = StorageInner::try_new(&path).unwrap();
        storage.set_item("kept", &42).unwrap();
        drop(storage);
        let original = std::fs::read(&path).unwrap();
        let mut attempts = 0;
        let result = StorageInner::try_new_with_open(
            &path,
            || {
                attempts += 1;
                Err(StorageOperationError::OpenDatabase(
                    redb::DatabaseError::RepairAborted,
                ))
            },
            |_| panic!("unconfirmed errors must not retry"),
        );
        assert_eq!(attempts, 1);
        assert!(matches!(
            result,
            Err(StorageOperationError::OpenDatabase(
                redb::DatabaseError::RepairAborted
            ))
        ));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[test]
    fn database_with_missing_table_preserves_file_and_rebuilds() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.redb");
        let database = redb::Database::create(&path).unwrap();
        drop(database);

        let storage = StorageInner::try_new(&path).unwrap();
        assert!(storage.get_item::<u64>("unknown").unwrap().value.is_none());
        let evidence = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|entry| entry.to_string_lossy().contains(".corrupt-"))
            .expect("unusable database is preserved");
        assert!(std::fs::metadata(&evidence).unwrap().len() > 0);
        let preserved = redb::Database::open(&evidence).unwrap();
        let read = preserved.begin_read().unwrap();
        assert!(read.open_table(NYANPASU_TABLE).is_err());
        assert!(path.is_file());
    }

    #[test]
    fn a_directory_database_is_preserved_not_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.redb");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("legacy"), b"keep this data").unwrap();
        let storage = StorageInner::try_new(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(storage.get_item::<u64>("unknown").unwrap().value, None);
        let evidence = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|entry| entry.is_dir() && entry.to_string_lossy().contains(".corrupt-"))
            .unwrap();
        assert_eq!(
            std::fs::read(evidence.join("legacy")).unwrap(),
            b"keep this data"
        );
    }

    #[cfg(unix)]
    #[test]
    fn linked_database_preserves_link_without_touching_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("storage.redb");
        let target = dir.path().join("target.redb");
        std::fs::write(&target, b"externally managed").unwrap();
        symlink(&target, &path).unwrap();
        let storage = StorageInner::try_new(&path).unwrap();
        assert_eq!(storage.get_item::<u64>("unknown").unwrap().value, None);
        assert!(path.is_file());
        assert_eq!(std::fs::read(&target).unwrap(), b"externally managed");
        let evidence = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|entry| entry.is_symlink())
            .unwrap();
        assert_eq!(std::fs::read_link(evidence).unwrap(), target);
    }

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
                revision: serde_json::from_value(serde_json::json!(index + 1)).unwrap(),
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
    fn live_snapshot_copies_committed_values_and_never_overwrites_destination() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.redb");
        let dest_path = dir.path().join("snapshot.redb");
        let storage = Storage::try_new(&source_path).unwrap();
        storage.set_item("web:key", &"before").unwrap();
        storage.export_to(&dest_path).unwrap();
        storage.set_item("web:key", &"after").unwrap();
        let snapshot = Storage::try_new(&dest_path).unwrap();
        assert_eq!(
            snapshot
                .get_item::<String>("web:key")
                .unwrap()
                .value
                .as_deref(),
            Some("before")
        );
        assert_eq!(
            storage
                .get_item::<String>("web:key")
                .unwrap()
                .value
                .as_deref(),
            Some("after")
        );
        drop(snapshot);
        let bytes = std::fs::read(&dest_path).unwrap();
        assert!(matches!(
            storage.export_to(&dest_path),
            Err(StorageOperationError::SnapshotDestinationExists(_))
        ));
        assert_eq!(std::fs::read(&dest_path).unwrap(), bytes);
    }

    #[test]
    fn hotkey_store_port_handles_missing_write_and_remove() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hotkeys.redb");
        let opener = RedbHotkeyStoreOpener;
        let store = opener.open(&path).unwrap();
        assert_eq!(store.read().unwrap(), None);
        let hotkeys = vec!["Ctrl+Shift+A".to_owned(), "Ctrl+Shift+B".to_owned()];
        store.write(&hotkeys).unwrap();
        assert_eq!(store.read().unwrap(), Some(hotkeys));
        store.remove().unwrap();
        assert_eq!(store.read().unwrap(), None);
        drop(store);
        assert!(path.is_file());
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
    fn a_stale_conditional_write_cannot_replace_a_newer_value() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        let committed = storage.set_item("web:key", &"current").unwrap();
        let mut rx = storage.subscribe();

        assert!(matches!(
            storage.set_item_if_revision("web:key", &"stale", StorageRevision::INITIAL),
            Err(StorageOperationError::RevisionConflict {
                expected: StorageRevision::INITIAL,
                current
            }) if current == committed
        ));
        assert!(rx.try_recv().is_err());
        let current = storage.get_item::<String>("web:key").unwrap();
        assert_eq!(current.value.as_deref(), Some("current"));
        assert_eq!(current.revision, committed);

        let next = storage
            .set_item_if_revision("web:key", &"next", committed)
            .unwrap();
        assert!(next > committed);
        assert_eq!(rx.try_recv().unwrap().revision, next);
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
        assert_eq!(empty.revision, StorageRevision::INITIAL);

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
        assert_eq!(item.revision, StorageRevision::INITIAL);
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

    #[test]
    fn removing_a_namespaced_value_also_removes_its_legacy_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let storage = open(&dir);
        storage.set_item("layout", &"legacy").unwrap();
        storage.set_item("web:layout", &"current").unwrap();
        let mut rx = storage.subscribe();

        let revision = storage
            .remove_item_with_fallback("web:layout", "layout")
            .unwrap();

        let removed = storage
            .get_item_with_fallback::<String>("web:layout", "layout")
            .unwrap();
        assert_eq!(removed.value, None);
        assert_eq!(removed.revision, revision);
        let change = rx.try_recv().unwrap();
        assert_eq!(change.key, "web:layout");
        assert_eq!(change.value, None);
        assert_eq!(change.revision, revision);
        assert!(rx.try_recv().is_err());
    }
}
