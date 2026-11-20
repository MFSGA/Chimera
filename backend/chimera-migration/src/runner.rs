//! Pure migration scheduling decisions from the ref Runner path.
//! IO, backup creation, and module execution remain injected by the application adapter.

use crate::{
    MigrationAdvice, MigrationState, StepSpec,
    context::Ctx,
    store::{MigrationStore, STORE_FILE_NAME},
};
use anyhow::Context as _;
use semver::Version;
use std::{fs, io};

/// A snapshot producer injected by the application shell. The shared Runner
/// decides *whether* migration must be blocked on a backup, while the shell
/// provides platform-aware file enumeration and verified manifest creation.
pub trait BackupPort {
    type Snapshot;

    fn create_snapshot(&mut self, ctx: &Ctx) -> anyhow::Result<Self::Snapshot>;
}

/// Ensure that existing user state is snapshotted exactly once before any
/// migration rewrite. A failed snapshot is not recorded, so a later retry can
/// create a new one. No writes happen in this shared gate itself.
pub fn ensure_backup<P: BackupPort>(
    ctx: &Ctx,
    existing: &mut Option<P::Snapshot>,
    port: &mut P,
) -> anyhow::Result<bool> {
    if existing.is_some() || has_nothing_to_protect(ctx)? {
        return Ok(false);
    }
    let snapshot = port.create_snapshot(ctx)?;
    *existing = Some(snapshot);
    Ok(true)
}

/// A fresh installation may have only migration-state baselines. Never treat
/// a dangling database symlink as absent or ignore an inaccessible root.
pub fn has_nothing_to_protect(ctx: &Ctx) -> anyhow::Result<bool> {
    let storage_path = ctx.storage_path();
    if crate::fs::config_node_exists(&storage_path)
        .with_context(|| format!("failed to inspect {}", storage_path.display()))?
    {
        return Ok(false);
    }
    let config = ctx.paths().app_config_dir();
    let entries = match fs::read_dir(config) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to list {}", config.display()));
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to inspect {}", config.display()))?;
        if entry.file_name() != STORE_FILE_NAME {
            return Ok(false);
        }
    }
    Ok(true)
}

pub fn advice_step(
    store: &MigrationStore,
    step: StepSpec<'_>,
    target: &Version,
    force: bool,
    enabled: bool,
) -> MigrationAdvice {
    if !enabled {
        return MigrationAdvice::Ignored;
    }
    if force {
        return MigrationAdvice::Pending;
    }
    match store.task_state(step.id) {
        Some(MigrationState::Completed | MigrationState::Skipped) => return MigrationAdvice::Done,
        Some(MigrationState::Failed | MigrationState::InProgress | MigrationState::NotStarted) => {
            return MigrationAdvice::Pending;
        }
        None => {}
    }
    if step.revision > store.module_state(step.module).applied_revision
        && introduced_in_reached(step.introduced_in, target)
    {
        MigrationAdvice::Pending
    } else {
        MigrationAdvice::Ignored
    }
}

/// Compare the release triple so an rc build executes migrations for the same release.
pub fn introduced_in_reached(introduced_in: &Version, target: &Version) -> bool {
    (
        introduced_in.major,
        introduced_in.minor,
        introduced_in.patch,
    ) <= (target.major, target.minor, target.patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::context::MigrationPaths;
    use chimera_contracts::{
        BoxedCause,
        storage::{HotkeyStore, HotkeyStoreOpener},
    };
    use std::{path::Path, sync::Arc};

    #[derive(Debug)]
    struct NeverOpenStore;
    impl HotkeyStoreOpener for NeverOpenStore {
        fn open(&self, _path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
            unreachable!("backup detection must never open the database")
        }
    }

    fn context(root: &Path) -> Ctx {
        Ctx::new(
            MigrationPaths::new(root.join("cfg"), root.join("data")),
            Arc::new(NeverOpenStore),
        )
    }

    #[derive(Default)]
    struct RecordingBackup {
        calls: usize,
        fail: bool,
    }
    impl BackupPort for RecordingBackup {
        type Snapshot = usize;
        fn create_snapshot(&mut self, _ctx: &Ctx) -> anyhow::Result<usize> {
            self.calls += 1;
            if self.fail {
                anyhow::bail!("injected snapshot failure")
            }
            Ok(self.calls)
        }
    }

    #[test]
    fn fresh_roots_and_baseline_file_need_no_snapshot_or_directory_creation() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert!(snapshot.is_none());
        assert_eq!(port.calls, 0);
        assert!(!ctx.paths().app_config_dir().exists());
        assert!(!ctx.paths().app_data_dir().exists());

        fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
        fs::write(ctx.state_path(), b"store_schema_version: 2\n").unwrap();
        assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(port.calls, 0);
        assert!(snapshot.is_none());
    }

    #[test]
    fn config_or_storage_requires_one_snapshot_before_rewrites() {
        for source in ["config", "storage"] {
            let root = tempfile::tempdir().unwrap();
            let ctx = context(root.path());
            if source == "config" {
                fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
                fs::write(ctx.chimera_config_path(), b"user settings").unwrap();
            } else {
                fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
                fs::write(ctx.storage_path(), b"user storage").unwrap();
            }
            let mut snapshot = None;
            let mut port = RecordingBackup::default();
            assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
            assert_eq!(snapshot, Some(1));
            assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
            assert_eq!(port.calls, 1);
        }
    }

    #[test]
    fn failed_snapshot_does_not_mark_protection_complete() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
        let old = b"hotkeys: [Control+Q]";
        fs::write(ctx.chimera_config_path(), old).unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup {
            fail: true,
            ..Default::default()
        };
        let error = ensure_backup(&ctx, &mut snapshot, &mut port).unwrap_err();
        assert!(format!("{error:#}").contains("injected snapshot failure"));
        assert!(snapshot.is_none());
        assert_eq!(fs::read(ctx.chimera_config_path()).unwrap(), old);
        port.fail = false;
        assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(snapshot, Some(2));
        assert_eq!(port.calls, 2);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_storage_link_still_requires_backup() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
        let missing = root.path().join("not-created.redb");
        symlink(&missing, ctx.storage_path()).unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(port.calls, 1);
        assert_eq!(fs::read_link(ctx.storage_path()).unwrap(), missing);
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_configuration_ancestor_blocks_without_backup_callback() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
        let not_directory = ctx.paths().app_config_dir().to_path_buf();
        fs::write(&not_directory, b"do not delete").unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        let error = ensure_backup(&ctx, &mut snapshot, &mut port).unwrap_err();
        assert!(format!("{error:#}").contains("failed to list"));
        assert!(snapshot.is_none());
        assert_eq!(port.calls, 0);
        assert_eq!(fs::read(&not_directory).unwrap(), b"do not delete");
    }

    #[test]
    fn prerelease_matches_release_but_not_future_release() {
        assert!(introduced_in_reached(
            &Version::parse("0.28.3").unwrap(),
            &Version::parse("0.28.3-rc.1").unwrap()
        ));
        assert!(!introduced_in_reached(
            &Version::parse("0.28.4").unwrap(),
            &Version::parse("0.28.3").unwrap()
        ));
    }

    #[test]
    fn all_durable_step_states_follow_runner_retry_policy() {
        use crate::store::TaskRecord;
        let mut store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        for (state, expected) in [
            (MigrationState::NotStarted, MigrationAdvice::Pending),
            (MigrationState::InProgress, MigrationAdvice::Pending),
            (MigrationState::Failed, MigrationAdvice::Pending),
            (MigrationState::Skipped, MigrationAdvice::Done),
            (MigrationState::Completed, MigrationAdvice::Done),
        ] {
            store.tasks.insert(
                step.id.into(),
                TaskRecord {
                    module: step.module.into(),
                    revision: step.revision,
                    introduced_in: version.clone(),
                    state,
                    started_at: None,
                    finished_at: None,
                    error: None,
                },
            );
            assert_eq!(advice_step(&store, step, &version, false, true), expected);
            assert_eq!(
                advice_step(&store, step, &version, true, true),
                MigrationAdvice::Pending
            );
            assert_eq!(
                advice_step(&store, step, &version, true, false),
                MigrationAdvice::Ignored
            );
        }
    }

    #[test]
    fn a_future_step_is_ignored_until_target_release() {
        let store = MigrationStore::default();
        let introduced = Version::new(0, 29, 0);
        let older = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "future/step",
            module: "future",
            revision: 1,
            introduced_in: &introduced,
        };
        assert_eq!(
            advice_step(&store, step, &older, false, true),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &introduced, false, true),
            MigrationAdvice::Pending
        );
    }

    #[test]
    fn disabled_steps_and_completed_steps_are_not_rerun_unless_forced() {
        let store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        assert_eq!(
            advice_step(&store, step, &version, false, false),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &version, false, true),
            MigrationAdvice::Pending
        );
        assert_eq!(
            advice_step(&store, step, &version, true, true),
            MigrationAdvice::Pending
        );
    }
}
