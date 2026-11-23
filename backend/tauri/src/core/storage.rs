//! Tauri storage wiring and legacy window-event compatibility.

use crate::utils::path::PathResolver;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{Emitter, Manager};
use tauri_specta::Event;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub use chimera_core::storage::{
    NYANPASU_TABLE, Storage, StorageChange, StorageInner, StorageItem, StorageNotification,
    StorageOperationError, StorageRevision, WebStorage, forward_storage_changes,
};

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

fn open_storage(paths: &PathResolver) -> anyhow::Result<Storage> {
    let storage_path = paths.storage_path();
    Storage::try_new(&storage_path).with_context(|| {
        format!(
            "failed to open application storage at {}",
            storage_path.display()
        )
    })
}

pub fn setup<R: tauri::Runtime, M: tauri::Manager<R>>(
    app: &M,
    paths: &PathResolver,
) -> anyhow::Result<()> {
    let storage = open_storage(paths)?;
    app.manage(storage);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_storage_uses_the_injected_data_root() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("isolated-config");
        let data_dir = temp.path().join("isolated-data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir.clone(), data_dir.clone());

        let _storage = open_storage(&paths).expect("startup storage should use injected paths");

        assert!(paths.storage_path().is_file());
        assert!(!config_dir.exists());
        assert_eq!(std::fs::read_dir(&data_dir).unwrap().count(), 1);
    }

    #[test]
    fn startup_preserves_confirmed_bad_database_before_rebuilding() {
        let temp = tempfile::tempdir().unwrap();
        let data_dir = temp.path().join("isolated-data");
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("isolated-config"), data_dir.clone());
        let storage_path = paths.storage_path();
        let original = b"preserve this file";
        std::fs::write(&storage_path, original).unwrap();

        let storage = open_storage(&paths).expect("confirmed corrupt storage can be rebuilt");
        assert_eq!(storage.get_item::<u64>("unknown").unwrap().value, None);
        let evidence: Vec<_> = std::fs::read_dir(&data_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|entry| entry.to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(evidence.len(), 1, "the original must be preserved once");
        assert_eq!(std::fs::read(&evidence[0]).unwrap(), original);
        assert!(storage_path.is_file(), "the replacement must exist");
    }
}
