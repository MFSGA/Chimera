//! Shared file opener. Keep both v3 and TSDB files on failure; recovery
//! always reopens the existing dataset instead of deleting it.
use std::{path::PathBuf, sync::Arc};

use chimera_observers::traffic::{
    SpawnError, TrafficArgs, TrafficClient, TrafficOwnerFactory, TrafficStartArgs, TrafficStorage,
};
use chimera_traffic::{TrafficDb, store::Opened};
use chimera_tsdb::{
    ErrorKind, Profile, RecoveryError,
    storage::{StorageError, redb::RedbStorage},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct TrafficFileStorage {
    path: PathBuf,
    cache_bytes: u64,
}
impl TrafficFileStorage {
    pub fn new(path: PathBuf, cache_bytes: u64) -> Self {
        Self { path, cache_bytes }
    }
    fn initialize_once(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        let storage = self
            .open()
            .map_err(|source| RecoveryError::Reopen { source })?;
        TrafficDb::open(storage, profile)
    }

    fn initialize_store(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        // Never delete or replace an existing dataset during this migration.
        // An unreadable destination disables optional recording; recovery
        // reopens the same file, not a newly created empty database.
        self.initialize_once(profile)
    }

    fn initialize_with_legacy(
        &self,
        profile: Profile,
    ) -> Result<Opened<RedbStorage>, RecoveryError> {
        let opened = self.initialize_store(profile.clone())?;
        let legacy = self.path.with_file_name("traffic.redb");
        if !legacy.exists() {
            return Ok(opened);
        }
        let (mut db, _) = opened;
        // Progress is committed with each imported batch. Leave the old file
        // intact even after success, and on failure resume on next startup
        // instead of abandoning the import or exposing partial data.
        db.import_legacy(&legacy, profile.batch_bytes, self.cache_bytes)
            .map_err(|source| RecoveryError::Restore {
                kind: ErrorKind::BackendFailed,
                source: Box::new(source),
            })?;
        drop(db);
        self.initialize_once(profile)
    }
}

#[async_trait::async_trait]
impl TrafficOwnerFactory for TrafficFileStorage {
    async fn spawn(
        self: Arc<Self>,
        args: TrafficStartArgs,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Result<TrafficClient, SpawnError> {
        TrafficClient::spawn(
            TrafficArgs::<RedbStorage> {
                storage: self,
                profile: args.profile,
                profiles: args.profiles,
                retention: args.retention,
                clock: args.clock,
                frames: args.frames,
                geo: args.geo,
                local_source: args.local_source,
            },
            shutdown,
            tasks,
        )
        .await
    }
}

impl TrafficStorage<RedbStorage> for TrafficFileStorage {
    fn open(&self) -> Result<RedbStorage, StorageError> {
        RedbStorage::open(&self.path, self.cache_bytes)
    }
    fn initialize(&self, profile: Profile) -> Result<Opened<RedbStorage>, RecoveryError> {
        self.initialize_with_legacy(profile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_import_preserves_legacy_and_never_opens_live_session() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("traffic.redb");
        let original = b"not a redb file";
        std::fs::write(&legacy, original).unwrap();
        let profile = Profile::desktop();
        let storage =
            TrafficFileStorage::new(dir.path().join("traffic.tsdb"), profile.backend_cache);
        assert!(storage.initialize(profile).is_err());
        assert_eq!(std::fs::read(&legacy).unwrap(), original);
    }
}
