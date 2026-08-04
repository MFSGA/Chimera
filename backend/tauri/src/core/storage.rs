use crate::{log_err, utils::dirs};
use anyhow::Context;
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use specta::Type;
use std::{fs, ops::Deref, result::Result as StdResult, sync::Arc};
use tauri::{Emitter, Manager};
use tauri_specta::Event;

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
    tx: tokio::sync::broadcast::Sender<(String, Option<Vec<u8>>)>,
}

/// Event emitted to all windows when a storage value changes.
/// Event name: `storage-value-changed-event`
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct StorageValueChangedEvent {
    pub key: String,
    /// The new JSON-encoded value, or `None` if the key was removed.
    pub value: Option<String>,
}

pub trait WebStorage {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<Option<T>>;
    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()>;
    fn remove_item(&self, key: impl AsRef<str>) -> Result<()>;
    fn remove_items(&self, keys: &[String]) -> Result<()>;
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
            tx: tokio::sync::broadcast::channel(16).0,
        })
    }

    pub fn get_instance(&self) -> &redb::Database {
        &self.instance
    }

    fn notify_subscribers(&self, key: impl AsRef<str>, value: Option<&[u8]>) {
        let key = key.as_ref().to_string();
        let value = value.map(|v| v.to_vec());
        let _ = self.tx.send((key, value));
    }

    pub(crate) fn get_rx(&self) -> tokio::sync::broadcast::Receiver<(String, Option<Vec<u8>>)> {
        self.tx.subscribe()
    }

    /// Removes all keys in one transaction, then notifies subscribers after commit.
    fn remove_items_with_before_commit<F>(&self, keys: &[String], before_commit: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        let db = self.get_instance();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            for key in keys {
                table.remove(key.as_bytes())?;
            }
        }
        before_commit()?;
        write_txn.commit()?;
        for key in keys {
            self.notify_subscribers(key, None);
        }
        Ok(())
    }
}

impl WebStorage for StorageInner {
    fn get_item<T: DeserializeOwned>(&self, key: impl AsRef<str>) -> Result<Option<T>> {
        let key = key.as_ref().as_bytes();
        let db = self.get_instance();
        let read_txn = db.begin_read()?;
        let table = read_txn.open_table(NYANPASU_TABLE)?;
        let result = table.get(key)?;
        match result {
            Some(value) => {
                let value = value.value();
                let value = serde_json::from_slice(value)?;
                Ok(Some(value))
            }
            None => Ok(None),
        }
    }

    fn set_item<T: Serialize>(&self, key: impl AsRef<str>, value: &T) -> Result<()> {
        let key_str = key.as_ref();
        let key = key_str.as_bytes();
        let value = serde_json::to_vec(value)?;
        let db = self.get_instance();
        let write_txn = db.begin_write()?;
        {
            let mut table = write_txn.open_table(NYANPASU_TABLE)?;
            table.insert(key, &*value)?;
        }
        write_txn.commit()?;
        self.notify_subscribers(key_str, Some(&value));
        Ok(())
    }

    fn remove_item(&self, key: impl AsRef<str>) -> Result<()> {
        self.remove_items(&[key.as_ref().to_string()])
    }

    fn remove_items(&self, keys: &[String]) -> Result<()> {
        self.remove_items_with_before_commit(keys, || Ok(()))
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

#[derive(Debug, PartialEq)]
enum StorageListenerEvent {
    Change((String, Option<Vec<u8>>)),
    ResyncRequired { skipped: u64 },
}

async fn receive_storage_change(
    rx: &mut tokio::sync::broadcast::Receiver<(String, Option<Vec<u8>>)>,
) -> Option<StorageListenerEvent> {
    match rx.recv().await {
        Ok(change) => Some(StorageListenerEvent::Change(change)),
        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
            tracing::warn!(
                skipped,
                "storage listener lagged behind; requesting a complete resync"
            );
            Some(StorageListenerEvent::ResyncRequired { skipped })
        }
        Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
    }
}

pub fn register_web_storage_listener(app_handle: &tauri::AppHandle) {
    let storage = app_handle.state::<Storage>();
    let rx = storage.get_rx();
    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        nyanpasu_utils::runtime::block_on(async {
            let mut rx = rx;

            while let Some(event) = receive_storage_change(&mut rx).await {
                match event {
                    StorageListenerEvent::Change((key, value)) => {
                        let value = value.map(|v| String::from_utf8_lossy(&v).to_string());
                        let payload = (key, value);
                        log_err!(app_handle.emit_filter(
                            "storage_value_changed",
                            payload,
                            |t| matches!(t, tauri::EventTarget::WebviewWindow { label } if label == "main"),
                        ), "failed to emit storage_value_changed event");
                    }
                    StorageListenerEvent::ResyncRequired { skipped } => {
                        log_err!(app_handle.emit_filter(
                            "storage_resync_required",
                            skipped,
                            |t| matches!(t, tauri::EventTarget::WebviewWindow { label } if label == "main"),
                        ), "failed to emit storage_resync_required event");
                    }
                }
            }
        });
    });
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(app: &M) -> anyhow::Result<()> {
    let storage_path = dirs::storage_path().context("failed to get storage path")?;
    let storage = Storage::try_new(&storage_path)?;
    app.manage(storage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use tempfile::tempdir;

    use super::{Storage, StorageListenerEvent, WebStorage, receive_storage_change};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestValue {
        enabled: bool,
        retries: u8,
    }

    #[test]
    fn storage_supports_crud_and_raw_listing() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        let value = TestValue {
            enabled: true,
            retries: 3,
        };

        storage
            .set_item("settings", &value)
            .expect("failed to write storage value");
        assert_eq!(
            storage
                .get_item::<TestValue>("settings")
                .expect("failed to read storage value"),
            Some(value)
        );
        assert_eq!(
            storage.get_all().expect("failed to list storage values"),
            vec![(
                "settings".to_string(),
                r#"{"enabled":true,"retries":3}"#.to_string()
            )]
        );

        storage
            .remove_item("settings")
            .expect("failed to remove storage value");
        assert_eq!(
            storage
                .get_item::<TestValue>("settings")
                .expect("failed to read removed storage value"),
            None
        );
    }

    #[test]
    fn storage_values_survive_database_reopen() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");

        {
            let storage = Storage::try_new(&path).expect("failed to create storage");
            storage
                .set_item("route", &"/settings")
                .expect("failed to persist route");
        }

        let reopened = Storage::try_new(&path).expect("failed to reopen storage");
        assert_eq!(
            reopened
                .get_item::<String>("route")
                .expect("failed to read persisted route"),
            Some("/settings".to_string())
        );
    }

    #[tokio::test]
    async fn batch_removal_commits_all_keys_before_notifying_in_order() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        storage
            .set_item("web:route", &"/settings")
            .expect("failed to write route");
        storage
            .set_item("web:theme", &"dark")
            .expect("failed to write theme");
        storage
            .set_item("internal:history", &"keep")
            .expect("failed to write internal value");
        let mut receiver = storage.get_rx();
        let keys = vec!["web:route".to_string(), "web:theme".to_string()];
        storage
            .remove_items(&keys)
            .expect("failed to remove storage values in one transaction");

        assert_eq!(
            storage
                .get_item::<String>("web:route")
                .expect("failed to read removed route"),
            None
        );
        assert_eq!(
            storage
                .get_item::<String>("web:theme")
                .expect("failed to read removed theme"),
            None
        );
        assert_eq!(
            storage
                .get_item::<String>("internal:history")
                .expect("failed to read preserved internal value"),
            Some("keep".to_string())
        );

        for expected_key in keys {
            let notification =
                tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
                    .await
                    .expect("timed out waiting for batch removal notification")
                    .expect("storage notification channel closed");
            assert_eq!(notification, (expected_key, None));
        }
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn lagged_storage_receiver_resumes_with_buffered_events() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        let mut receiver = storage.get_rx();

        for index in 0..20 {
            storage
                .set_item(format!("web:key-{index}"), &index)
                .expect("failed to fill storage notification buffer");
        }

        let resync = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            receive_storage_change(&mut receiver),
        )
        .await
        .expect("timed out waiting for storage resync signal after lag")
        .expect("storage notification channel unexpectedly closed");
        assert_eq!(resync, StorageListenerEvent::ResyncRequired { skipped: 4 });

        let buffered = receive_storage_change(&mut receiver)
            .await
            .expect("storage notification channel unexpectedly closed");
        assert_eq!(
            buffered,
            StorageListenerEvent::Change(("web:key-4".to_string(), Some(b"4".to_vec())))
        );

        storage
            .set_item("web:after-lag", &"visible")
            .expect("failed to write notification after lag recovery");
        let mut observed_after_lag = false;
        for _ in 0..16 {
            let Some(event) = receive_storage_change(&mut receiver).await else {
                break;
            };
            if matches!(
                event,
                StorageListenerEvent::Change((ref key, _)) if key == "web:after-lag"
            ) {
                observed_after_lag = true;
                break;
            }
        }
        assert!(
            observed_after_lag,
            "receiver must remain active after lag recovery"
        );
    }

    #[tokio::test]
    async fn lagged_storage_receiver_can_resync_from_complete_snapshot() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        let mut receiver = storage.get_rx();

        for revision in 0..20 {
            storage
                .set_item("web:route", &format!("/revision-{revision}"))
                .expect("failed to write route revision");
        }
        storage
            .set_item("web:theme", &"dark")
            .expect("failed to write independent snapshot value");

        let resync = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            receive_storage_change(&mut receiver),
        )
        .await
        .expect("timed out waiting for storage resync signal after lag")
        .expect("storage notification channel unexpectedly closed");
        assert_eq!(resync, StorageListenerEvent::ResyncRequired { skipped: 5 });

        let snapshot = storage
            .get_all()
            .expect("failed to rebuild storage state from a complete snapshot");
        assert_eq!(
            snapshot,
            vec![
                ("web:route".to_string(), r#""/revision-19""#.to_string()),
                ("web:theme".to_string(), r#""dark""#.to_string()),
            ]
        );

        storage
            .remove_item("web:theme")
            .expect("failed to remove value after snapshot resync");
        let mut observed_removal = false;
        for _ in 0..17 {
            let Some(event) = receive_storage_change(&mut receiver).await else {
                break;
            };
            if event == StorageListenerEvent::Change(("web:theme".to_string(), None)) {
                observed_removal = true;
                break;
            }
        }
        assert!(
            observed_removal,
            "receiver must continue delivering changes after snapshot resync"
        );
    }

    #[tokio::test]
    async fn failed_batch_removal_rolls_back_all_keys_without_notifying() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        storage
            .set_item("web:route", &"/settings")
            .expect("failed to write route");
        storage
            .set_item("web:theme", &"dark")
            .expect("failed to write theme");
        let mut receiver = storage.get_rx();
        let keys = vec!["web:route".to_string(), "web:theme".to_string()];

        let error = storage
            .remove_items_with_before_commit(&keys, || {
                Err(super::StorageOperationError::Serialize(
                    serde_json::from_str::<serde_json::Value>("{")
                        .expect_err("invalid JSON should produce a test error"),
                ))
            })
            .expect_err("injected pre-commit failure should abort batch removal");
        assert!(
            error
                .to_string()
                .contains("failed to serialize or deserialize data")
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            storage
                .get_item::<String>("web:route")
                .expect("failed to read rolled back route"),
            Some("/settings".to_string())
        );
        assert_eq!(
            storage
                .get_item::<String>("web:theme")
                .expect("failed to read rolled back theme"),
            Some("dark".to_string())
        );

        drop(storage);
        let reopened = Storage::try_new(&path).expect("failed to reopen storage after rollback");
        assert_eq!(
            reopened
                .get_item::<String>("web:route")
                .expect("failed to read persisted route after rollback"),
            Some("/settings".to_string())
        );
        assert_eq!(
            reopened
                .get_item::<String>("web:theme")
                .expect("failed to read persisted theme after rollback"),
            Some("dark".to_string())
        );
    }

    #[tokio::test]
    async fn storage_notifies_all_subscribers_in_write_order() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        let storage = Storage::try_new(&path).expect("failed to create storage");
        let mut first = storage.get_rx();
        let mut second = storage.get_rx();

        storage
            .set_item("route", &"/settings")
            .expect("failed to write storage value");
        storage
            .remove_item("route")
            .expect("failed to remove storage value");

        for receiver in [&mut first, &mut second] {
            let written = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
                .await
                .expect("timed out waiting for storage write notification")
                .expect("storage write notification channel closed");
            assert_eq!(written.0, "route");
            assert_eq!(written.1, Some(br#""/settings""#.to_vec()));

            let removed = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
                .await
                .expect("timed out waiting for storage remove notification")
                .expect("storage remove notification channel closed");
            assert_eq!(removed, ("route".to_string(), None));
        }
    }

    #[test]
    fn storage_replaces_a_directory_at_the_database_path() {
        let directory = tempdir().expect("failed to create storage temp directory");
        let path = directory.path().join("storage.db");
        std::fs::create_dir(&path).expect("failed to create conflicting directory");
        std::fs::write(path.join("stale"), "old data")
            .expect("failed to create conflicting directory contents");

        let storage = Storage::try_new(&path).expect("failed to replace directory with database");
        storage
            .set_item("ready", &true)
            .expect("failed to write replacement database");

        assert!(path.is_file());
        assert_eq!(
            storage
                .get_item::<bool>("ready")
                .expect("failed to read replacement database"),
            Some(true)
        );
    }
}
