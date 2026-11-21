//! Keeps build inputs consistent across managed profile file and document writes.

use std::sync::Arc;

use chimera_config::profile::Profiles;
use chimera_core::state::{StateSnapshot, VersionedState};

/// Held by the Profiles owner through document publication or compensation: a
/// profile write promotes its file before the document is published, so the
/// files do not match the committed document in between. Validation and runtime
/// application never hold it. Readers go through [`ProfileContentReader`].
#[derive(Clone, Default)]
pub(crate) struct ProfileContentLock(Arc<tokio::sync::RwLock<()>>);

impl ProfileContentLock {
    // Chimera's detached Profiles persistence owner holds this guard across
    // cancellation and local-write compensation. Unlike ref's borrowed guard,
    // it must be transferable to the owner without borrowing its local lock.
    pub(crate) async fn write(&self) -> tokio::sync::OwnedRwLockWriteGuard<()> {
        self.0.clone().write_owned().await
    }
}

/// The only way to read managed profile files together with the document they
/// belong to. A reader that loads the document and the files separately can
/// pair a promoted file with the document that preceded it, or read content a
/// failed write is about to compensate.
#[derive(Clone)]
pub(crate) struct ProfileContentReader {
    committed: StateSnapshot<Profiles>,
    lock: ProfileContentLock,
}

impl ProfileContentReader {
    pub(crate) fn new(committed: StateSnapshot<Profiles>, lock: ProfileContentLock) -> Self {
        Self { committed, lock }
    }

    /// Runs `read` on the committed document. No profile write replaces a file
    /// or publishes a document until the returned future completes, so `read`
    /// must stay local: it holds every profile write back.
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

    /// Reads the files a candidate document refers to, under the same guarantee.
    /// The caller brings the candidate; the committed document is not involved.
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
