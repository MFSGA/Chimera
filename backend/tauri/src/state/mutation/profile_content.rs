//! Ref ProfileContentReader: read a committed Profiles document and its managed
//! files under the same fence used for materialization promotion/recovery.
//! Until Profiles moves to the standalone state engine, its existing versioned
//! snapshot is intentionally used here; no new document writer is created.

use chimera_config::profile::Profiles;
use chimera_core::state::{StateSnapshot, VersionedState};
use std::{future::Future, sync::Arc};
use tokio::sync::{OwnedRwLockWriteGuard, RwLock};

#[derive(Clone, Default)]
pub(crate) struct ProfileContentLock(Arc<RwLock<()>>);

impl ProfileContentLock {
    pub(crate) async fn write(&self) -> OwnedRwLockWriteGuard<()> {
        self.0.clone().write_owned().await
    }
}

#[derive(Clone)]
pub(crate) struct ProfileContentReader {
    committed: StateSnapshot<Profiles>,
    lock: ProfileContentLock,
}

impl ProfileContentReader {
    pub(crate) fn new(committed: StateSnapshot<Profiles>, lock: ProfileContentLock) -> Self {
        Self { committed, lock }
    }

    /// Read the document and all of its file inputs while no file promotion,
    /// restoration, or durable materialization recovery may run.
    pub(crate) async fn committed<T, Fut>(
        &self,
        read: impl FnOnce(Arc<VersionedState<Profiles>>) -> Fut,
    ) -> T
    where
        Fut: Future<Output = T>,
    {
        let _guard = self.lock.0.read().await;
        read(self.committed.load()).await
    }

    /// A candidate supplies its own document, so it only needs to fence the
    /// filesystem reads. Do not hold this fence during a Core check or apply.
    pub(crate) async fn candidate<T>(&self, read: impl Future<Output = T>) -> T {
        let _guard = self.lock.0.read().await;
        read.await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_core::state::PersistentStateManagerSetup;

    #[tokio::test]
    async fn committed_read_pins_snapshot_and_blocks_a_concurrent_file_writer() {
        let dir = tempfile::tempdir().unwrap();
        let file = camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
        let manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(file)
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let lock = ProfileContentLock::default();
        let reader = ProfileContentReader::new(manager.snapshot_handle(), lock.clone());
        let (entered, read_entered) = tokio::sync::oneshot::channel();
        let (resume, resume_read) = tokio::sync::oneshot::channel();
        let reader_task = tokio::spawn(async move {
            reader
                .committed(|snapshot| async move {
                    entered.send(()).unwrap();
                    resume_read.await.unwrap();
                    snapshot.version
                })
                .await
        });
        read_entered.await.unwrap();
        let mut write = Box::pin(lock.write());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut write)
                .await
                .is_err()
        );
        resume.send(()).unwrap();
        assert_eq!(
            reader_task.await.unwrap(),
            chimera_core::state::Version::new(0)
        );
        let _guard = tokio::time::timeout(std::time::Duration::from_secs(1), write)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn promoted_file_cannot_be_read_with_its_pre_commit_document() {
        let dir = tempfile::tempdir().unwrap();
        let config = camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
        let managed = dir.path().join("active.yaml");
        tokio::fs::write(&managed, b"before").await.unwrap();
        let mut manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(config)
            .assemble()
            .from_state(Profiles::default())
            .await
            .unwrap();
        let lock = ProfileContentLock::default();
        let reader = ProfileContentReader::new(manager.snapshot_handle(), lock.clone());
        let (promoted, promoted_rx) = tokio::sync::oneshot::channel();
        let (publish, publish_rx) = tokio::sync::oneshot::channel();
        let file_for_writer = managed.clone();
        let writer = tokio::spawn(async move {
            let guard = lock.write().await;
            tokio::fs::write(&file_for_writer, b"after").await.unwrap();
            promoted.send(()).unwrap();
            publish_rx.await.unwrap();
            manager.upsert(Profiles::default()).await.unwrap();
            drop(guard);
        });
        promoted_rx.await.unwrap();
        let mut read = Box::pin(reader.committed(|snapshot| async move {
            let bytes = tokio::fs::read(&managed).await.unwrap();
            (snapshot.version, bytes)
        }));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut read)
                .await
                .is_err()
        );
        publish.send(()).unwrap();
        let (version, bytes) = read.await;
        assert_eq!(version, chimera_core::state::Version::new(1));
        assert_eq!(bytes, b"after");
        writer.await.unwrap();
    }

    #[tokio::test]
    async fn candidate_read_cannot_observe_a_file_during_promotion() {
        let lock = ProfileContentLock::default();
        let guard = lock.write().await;
        let reader = ProfileContentReader::new(
            PersistentStateManagerSetup::<Profiles>::builder()
                .config_path(camino::Utf8PathBuf::from("unused.yaml"))
                .assemble()
                .from_state(Profiles::default())
                .await
                .unwrap()
                .snapshot_handle(),
            lock,
        );
        let mut read = Box::pin(reader.candidate(async { 42 }));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), &mut read)
                .await
                .is_err()
        );
        drop(guard);
        assert_eq!(read.await, 42);
    }
}
