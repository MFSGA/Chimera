//! Jobs recovery tests over isolated temporary journal paths.
use redb::ReadableDatabase as _;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

fn evidence(path: &Path) -> Vec<PathBuf> {
    let prefix = format!("{}.corrupt-", path.file_name().unwrap().to_string_lossy());
    fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(&prefix)
        })
        .collect()
}
async fn start_jobs(path: &Path) -> Result<(), crate::client::jobs::Error> {
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let started = crate::client::jobs::start(
        path.to_owned(),
        crate::client::jobs::capture(),
        shutdown.clone(),
        &tasks,
    )
    .await
    .map(drop);
    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
    started
}

#[tokio::test]
async fn unusable_jobs_journals_are_preserved_and_rebuilt() {
    const RUNS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("jobs_runs_v1");
    let temp = tempfile::tempdir().unwrap();
    let foreign = temp.path().join("foreign").join("jobs.redb");
    fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    fs::write(&foreign, vec![b'x'; 4096]).unwrap();
    // Opens cleanly; the record fails to decode while the owner recovers.
    let undecodable = temp.path().join("undecodable").join("jobs.redb");
    fs::create_dir_all(undecodable.parent().unwrap()).unwrap();
    {
        let db = redb::Database::create(&undecodable).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(RUNS)
            .unwrap()
            .insert("run", b"not json".as_slice())
            .unwrap();
        tx.commit().unwrap();
    }
    let directory = temp.path().join("directory").join("jobs.redb");
    fs::create_dir_all(&directory).unwrap();

    for path in [&foreign, &undecodable, &directory] {
        start_jobs(path).await.unwrap();

        assert_eq!(evidence(path).len(), 1, "{}", path.display());
        let db = redb::Database::open(path).unwrap();
        let tx = db.begin_read().unwrap();
        assert!(
            tx.open_table(RUNS).unwrap().get("run").unwrap().is_none(),
            "the rebuilt journal starts empty: {}",
            path.display()
        );
    }
    assert_eq!(fs::read(&evidence(&foreign)[0]).unwrap(), vec![b'x'; 4096]);
    assert!(evidence(&directory)[0].is_dir());
}

#[tokio::test]
async fn a_held_jobs_journal_is_reported_without_rebuilding() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.redb");
    let _held = redb::Database::create(&path).unwrap();

    let error = start_jobs(&path).await.unwrap_err();

    assert!(
        matches!(&error, crate::client::jobs::Error::OpenStore { .. })
            && crate::utils::redb_ext::find_failure(&error)
                == Some(crate::utils::redb_ext::RedbFailure::AlreadyOpen),
        "{error:?}"
    );
    assert!(evidence(&path).is_empty());
}

#[tokio::test]
async fn incompatible_jobs_table_is_preserved_before_rebuild() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.redb");
    {
        let db = redb::Database::create(&path).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(redb::TableDefinition::<u64, u64>::new("jobs_runs_v1"))
            .unwrap()
            .insert(5, 7)
            .unwrap();
        tx.commit().unwrap();
    }
    start_jobs(&path).await.unwrap();
    let saved = evidence(&path);
    assert_eq!(saved.len(), 1);
    let original = redb::Database::open(&saved[0]).unwrap();
    let original_tx = original.begin_read().unwrap();
    assert_eq!(
        original_tx
            .open_table(redb::TableDefinition::<u64, u64>::new("jobs_runs_v1"))
            .unwrap()
            .get(5)
            .unwrap()
            .unwrap()
            .value(),
        7
    );
    let fresh = redb::Database::open(&path).unwrap();
    let fresh_tx = fresh.begin_read().unwrap();
    assert!(
        fresh_tx
            .open_table(redb::TableDefinition::<&str, &[u8]>::new("jobs_runs_v1"))
            .is_ok()
    );
}

#[tokio::test]
async fn inaccessible_parent_is_not_treated_as_corrupt_journal() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("parent-is-a-file");
    fs::write(&parent, b"do not touch").unwrap();
    let path = parent.join("jobs.redb");
    let error = start_jobs(&path).await.unwrap_err();
    // Unix reports NotADirectory while Windows can report NotFound for a
    // child of a regular file. Both are a fail-closed IO error: neither is
    // evidence that a jobs journal is corrupt and safe to rebuild.
    assert!(
        matches!(
            &error,
            super::Error::InspectStore { .. }
                | super::Error::OpenStore {
                    source: chimera_jobs::Error::JournalDirectory(_),
                    ..
                }
        ),
        "unexpected parent-path failure: {error:?}"
    );
    assert!(
        !error.unusable_journal(),
        "parent failure cannot trigger recovery"
    );
    assert_eq!(fs::read(&parent).unwrap(), b"do not touch");
    assert!(!path.exists());
    assert_eq!(
        fs::read_dir(temp.path()).unwrap().count(),
        1,
        "no evidence or replacement file is created"
    );
}

#[tokio::test]
async fn healthy_jobs_journal_reopens_without_creating_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("jobs.redb");
    drop(chimera_jobs::RedbJobStore::open(&path).unwrap());
    start_jobs(&path).await.unwrap();
    assert!(evidence(&path).is_empty());
    assert!(redb::Database::open(&path).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_journal_is_preserved_without_touching_its_target() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("keep-this-file");
    fs::write(&target, b"original target").unwrap();
    let journal = temp.path().join("jobs.redb");
    symlink(&target, &journal).unwrap();

    start_jobs(&journal).await.unwrap();

    let saved = evidence(&journal);
    assert_eq!(saved.len(), 1);
    assert!(
        fs::symlink_metadata(&saved[0])
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_link(&saved[0]).unwrap(), target);
    assert_eq!(fs::read(&target).unwrap(), b"original target");
    assert!(fs::symlink_metadata(&journal).unwrap().is_file());
}

#[test]
fn permission_denied_is_not_a_rebuildable_journal() {
    let path = std::path::PathBuf::from("never-read");
    let denied = || std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    assert!(
        !super::Error::InspectStore {
            path: path.clone(),
            source: denied(),
        }
        .unusable_journal()
    );
    assert!(
        !super::Error::OpenStore {
            path,
            source: chimera_jobs::Error::JournalDirectory(denied()),
        }
        .unusable_journal()
    );
}
