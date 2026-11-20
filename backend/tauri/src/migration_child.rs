//! Migration-only entry point and parent/child protocol. The child never
//! constructs Tauri or client/runtime owners. All roots are injected by the
//! already-claimed primary instance, not rediscovered from process globals.
use std::{
    ffi::OsString,
    fs,
    io::Read as _,
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
};

use anyhow::{Context as _, anyhow, bail};

use crate::{
    core::{backup::BackupInfo, migration::Runner},
    startup_failure::{self, Report, StartupStage},
    utils::path::PathResolver,
};

#[cfg(test)]
use chimera_migration::child_result::MAX_REPORT_BYTES;
use chimera_migration::child_result::{self, validate_new_result_destination};

const CHILD_FLAG: &str = "--chimera-internal-migrate";
type ChildOutcome = child_result::ChildOutcome<Report>;

// The shell specializes the shared generic wire protocol to Tauri's existing
// startup report. Encoding, validation and IO are owned by chimera-migration.
fn write_outcome(result: &Path, outcome: &ChildOutcome) -> anyhow::Result<()> {
    child_result::write_outcome(result, outcome)
}

fn read_outcome(result: &Path) -> anyhow::Result<ChildOutcome> {
    child_result::read_outcome(result)
}

/// Called in `main` before `chimera_lib::run()` and before all Tauri plugins.
/// A normal desktop invocation is not interpreted as a migration command.
pub fn maybe_run_from_command_line() -> Option<ExitCode> {
    let mut arguments = std::env::args_os().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(CHILD_FLAG)) {
        return None;
    }
    Some(match parse_child_arguments(arguments.collect()) {
        Ok((paths, result)) => execute_to_file(&paths, &result),
        Err(error) => {
            eprintln!("invalid Chimera migration child invocation: {error:#}");
            ExitCode::FAILURE
        }
    })
}

fn parse_child_arguments(args: Vec<OsString>) -> anyhow::Result<(PathResolver, PathBuf)> {
    let [config, data, result] = <[OsString; 3]>::try_from(args)
        .map_err(|_| anyhow!("expected config root, data root and private result path"))?;
    let config = PathBuf::from(config);
    let data = PathBuf::from(data);
    let result = PathBuf::from(result);
    if !config.is_absolute() || !data.is_absolute() || !result.is_absolute() {
        bail!("child migration paths must be absolute");
    }
    Ok((PathResolver::with_base_dirs(config, data), result))
}

fn execute(paths: &PathResolver) -> ChildOutcome {
    let mut runner = match Runner::with_paths(paths.clone(), false)
        .context("failed to prepare migration child Runner")
    {
        Ok(runner) => runner,
        Err(error) => {
            return ChildOutcome::Failure {
                report: Report::from_error(error.as_ref(), StartupStage::MigrationPrepare, paths),
                backup_name: None,
            };
        }
    };
    if let Err(error) = runner.run_pending().context("migration child failed") {
        return ChildOutcome::Failure {
            report: Report::from_error_with_backup(
                error.as_ref(),
                StartupStage::MigrationRun,
                paths,
                runner.backup_info(),
            ),
            backup_name: runner.backup_info().map(|backup| backup.name.clone()),
        };
    }
    ChildOutcome::Success {
        backup_name: runner.backup_info().map(|backup| backup.name.clone()),
    }
}

fn execute_to_file(paths: &PathResolver, result: &Path) -> ExitCode {
    // Fail *before* running an irreversible migration if the expected new
    // result path is already occupied. Otherwise a write refusal after a
    // successful migration would leave no trustworthy child result.
    if let Err(error) = validate_new_result_destination(result) {
        eprintln!("invalid migration child result destination: {error:#}");
        return ExitCode::FAILURE;
    }
    let outcome = execute(paths);
    if let Err(error) = write_outcome(result, &outcome) {
        eprintln!("failed to persist migration child report: {error:#}");
        return ExitCode::FAILURE;
    }
    if matches!(outcome, ChildOutcome::Success { .. }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn verified_backup(
    paths: &PathResolver,
    name: Option<String>,
) -> anyhow::Result<Option<BackupInfo>> {
    name.map(|name| {
        child_result::validate_backup_name(&name, crate::core::backup::MIGRATION_PREFIX)?;
        let backup = BackupInfo {
            path: paths.backups_dir().join(&name),
            name,
        };
        if !backup.is_available() {
            bail!("migration child returned an unavailable or unverified snapshot");
        }
        Ok(backup)
    })
    .transpose()
}

/// Called only by the primary instance after Tauri's single-instance claim.
/// The subprocess never holds the main AppHandle and exits before Core/Jobs
/// open the user's redb databases.
pub(crate) fn run_subprocess(
    paths: &PathResolver,
    capture: &parking_lot::Mutex<Option<Report>>,
) -> anyhow::Result<Option<BackupInfo>> {
    run_with_spawn(paths, capture, |result_file, stderr_path| {
        let stderr = fs::File::create(stderr_path)?;
        let executable =
            std::env::current_exe().context("failed to locate migration executable")?;
        Command::new(executable)
            .arg(CHILD_FLAG)
            .arg(paths.app_config_dir())
            .arg(paths.app_data_dir())
            .arg(result_file)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .status()
            .context("failed to spawn or wait for migration child")
    })
}

/// Same parent/result path for production and an actual separate test process.
/// A failed child must never be converted into a successful startup merely
/// because it wrote an apparently valid JSON file.
fn run_with_spawn(
    paths: &PathResolver,
    capture: &parking_lot::Mutex<Option<Report>>,
    spawn: impl FnOnce(&Path, &Path) -> anyhow::Result<ExitStatus>,
) -> anyhow::Result<Option<BackupInfo>> {
    let result = (|| -> anyhow::Result<Option<BackupInfo>> {
        let temp =
            tempfile::tempdir().context("failed to prepare private migration result directory")?;
        let result_file = temp.path().join("result.json");
        let stderr_path = temp.path().join("child-stderr.log");
        let status = spawn(&result_file, &stderr_path)?;
        let outcome = read_outcome(&result_file).with_context(|| {
            // The child can crash without writing its result; keep the exit
            // status in the parent error instead of guessing a success.
            let mut diagnostics = Vec::new();
            if let Ok(file) = fs::File::open(&stderr_path) {
                let _ = file.take(4096).read_to_end(&mut diagnostics);
            }
            let excerpt = String::from_utf8_lossy(&diagnostics);
            format!("migration child exited {status} without a valid result: {excerpt}")
        })?;
        match (status.success(), outcome) {
            (true, ChildOutcome::Success { backup_name }) => verified_backup(paths, backup_name),
            (
                false,
                ChildOutcome::Failure {
                    mut report,
                    backup_name,
                },
            ) => {
                // Recheck the child-supplied snapshot from the primary process.
                // A path reported by a child isn't proof of current integrity.
                report.backups_dir = verified_backup(paths, backup_name)
                    .ok()
                    .flatten()
                    .and_then(|backup| backup.path.parent().map(Path::to_path_buf));
                *capture.lock() = Some(report.clone());
                Err(anyhow!(
                    "migration subprocess reported {:?} ({:?}): {}",
                    report.failure.stage,
                    report.failure.kind,
                    report.failure.detail
                ))
            }
            _ => bail!("migration child exit status disagrees with structured outcome: {status}"),
        }
    })();
    if result.is_err() && capture.lock().is_none() {
        return startup_failure::report_boundary_with_backup_and_capture(
            result,
            StartupStage::MigrationRun,
            paths,
            None,
            Some(capture),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unexpected_args_and_child_paths_outside_absolute_roots() {
        assert!(parse_child_arguments(vec![]).is_err());
        assert!(
            parse_child_arguments(vec![
                "relative".into(),
                "/tmp/data".into(),
                "/tmp/out".into()
            ])
            .is_err()
        );
        assert!(
            parse_child_arguments(vec![
                "/tmp/config".into(),
                "/tmp/data".into(),
                "/tmp/result".into()
            ])
            .is_ok()
        );
    }

    #[test]
    fn private_child_report_roundtrips_and_rejects_bad_protocol_and_symlinks() {
        let temp = tempfile::tempdir().unwrap();
        let result = temp.path().join("result.json");
        let outcome = ChildOutcome::Success { backup_name: None };
        write_outcome(&result, &outcome).unwrap();
        assert!(matches!(
            read_outcome(&result).unwrap(),
            ChildOutcome::Success { backup_name: None }
        ));
        let body = fs::read_to_string(&result)
            .unwrap()
            .replace("\"protocol_version\":1", "\"protocol_version\":999");
        fs::write(&result, body).unwrap();
        assert!(
            read_outcome(&result)
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
        fs::write(&result, vec![b'x'; MAX_REPORT_BYTES as usize + 1]).unwrap();
        assert!(
            read_outcome(&result)
                .unwrap_err()
                .to_string()
                .contains("bounded regular file")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            fs::remove_file(&result).unwrap();
            symlink(temp.path().join("missing"), &result).unwrap();
            assert!(read_outcome(&result).is_err());
        }
    }

    #[test]
    fn existing_result_file_or_missing_parent_rejects_migration_before_running() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let legacy = b"hotkeys: [clash_mode_rule,Control+Q]\n";
        fs::write(paths.chimera_config_path(), legacy).unwrap();
        let result = temp.path().join("child-result.json");
        fs::write(&result, b"unrelated previous result").unwrap();
        assert_eq!(execute_to_file(&paths, &result), ExitCode::FAILURE);
        assert_eq!(fs::read(&result).unwrap(), b"unrelated previous result");
        assert_eq!(
            execute_to_file(&paths, &temp.path().join("missing-parent/result.json")),
            ExitCode::FAILURE
        );
        assert!(!paths.app_config_dir().join("migration-state.yaml").exists());
        assert!(!paths.application_config_path().exists());
        assert_eq!(fs::read(paths.chimera_config_path()).unwrap(), legacy);
    }

    #[cfg(unix)]
    #[test]
    fn occupied_result_node_rejects_migration_before_any_user_data_writes() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let original = b"hotkeys: [clash_mode_rule,Control+Q]\n";
        fs::write(paths.chimera_config_path(), original).unwrap();
        let external = temp.path().join("external-result.txt");
        fs::write(&external, b"other user's bytes").unwrap();
        let result = temp.path().join("result.json");
        symlink(&external, &result).unwrap();
        assert_eq!(execute_to_file(&paths, &result), ExitCode::FAILURE);
        assert_eq!(fs::read_link(&result).unwrap(), external);
        assert_eq!(fs::read(&external).unwrap(), b"other user's bytes");
        assert_eq!(fs::read(paths.chimera_config_path()).unwrap(), original);
        assert!(!paths.app_config_dir().join("migration-state.yaml").exists());
        assert!(!paths.application_config_path().exists());
        fs::remove_file(&result).unwrap();
        fs::write(&result, b"existing child result").unwrap();
        assert_eq!(execute_to_file(&paths, &result), ExitCode::FAILURE);
        assert_eq!(fs::read(&result).unwrap(), b"existing child result");
        assert!(!paths.app_config_dir().join("migration-state.yaml").exists());
    }

    #[test]
    fn untrusted_backup_path_is_never_advertised() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        for name in [
            "../x",
            "migration-../../outside",
            "manual-2026",
            "migration-\\\\etc",
            "migration-🔒",
        ] {
            assert!(verified_backup(&paths, Some(name.to_owned())).is_err());
        }
        assert!(verified_backup(&paths, None).unwrap().is_none());
    }

    #[test]
    fn child_execution_reports_real_migration_failure_without_initializing_tauri() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let original = b"invalid: [";
        fs::write(paths.chimera_config_path(), original).unwrap();
        let result = temp.path().join("result.json");
        assert_eq!(execute_to_file(&paths, &result), ExitCode::FAILURE);
        match read_outcome(&result).unwrap() {
            ChildOutcome::Failure {
                report,
                backup_name,
            } => {
                assert!(backup_name.is_some());
                assert_eq!(report.failure.stage, StartupStage::MigrationRun);
                assert_eq!(report.backups_dir, Some(paths.backups_dir()));
                assert_eq!(fs::read(paths.chimera_config_path()).unwrap(), original);
            }
            other => panic!("expected migration failure: {other:?}"),
        }
    }
}

#[cfg(test)]
mod process_tests {
    use super::*;

    const FIXTURE_MARKER: &str = "CHIMERA_INTERNAL_CHILD_TEST_ONLY";
    const CHILD_TEST: &str = "migration_child::process_tests::child_fixture_entry";

    /// Executed inside a *new* test-harness process to exercise the same
    /// independent migration engine and result writer as desktop `main`.
    /// Normal `cargo test` invocations simply skip this fixture.
    #[test]
    fn child_fixture_entry() {
        let Some(mode) = std::env::var_os(FIXTURE_MARKER) else {
            return;
        };
        if mode == "crash" {
            std::process::exit(73);
        }
        let config = PathBuf::from(std::env::var_os("CHIMERA_CHILD_TEST_CONFIG").unwrap());
        let data = PathBuf::from(std::env::var_os("CHIMERA_CHILD_TEST_DATA").unwrap());
        let result = PathBuf::from(std::env::var_os("CHIMERA_CHILD_TEST_RESULT").unwrap());
        let paths = PathResolver::with_base_dirs(config, data);
        let exit = execute_to_file(&paths, &result);
        std::process::exit(if exit == ExitCode::SUCCESS { 0 } else { 1 });
    }

    fn child_spawn(
        paths: &PathResolver,
        result: &Path,
        stderr: &Path,
        mode: &str,
    ) -> anyhow::Result<ExitStatus> {
        let output = fs::File::create(stderr)?;
        Command::new(std::env::current_exe()?)
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env(FIXTURE_MARKER, mode)
            .env("CHIMERA_CHILD_TEST_CONFIG", paths.app_config_dir())
            .env("CHIMERA_CHILD_TEST_DATA", paths.app_data_dir())
            .env("CHIMERA_CHILD_TEST_RESULT", result)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(output))
            .status()
            .map_err(Into::into)
    }

    #[test]
    fn real_child_process_succeeds_without_gui_and_returns_verified_legacy_snapshot() {
        use redb::ReadableDatabase as _;
        const USER_TABLE: redb::TableDefinition<&str, &str> =
            redb::TableDefinition::new("user-owned-before-child");
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        const LEGACY: &str = include_str!(
            "../../../ref/backend/nyanpasu-migration/src/fixtures/v1_6_1/nyanpasu-config.yaml"
        );
        fs::write(paths.chimera_config_path(), LEGACY).unwrap();
        let storage = paths.storage_path();
        {
            let db = redb::Database::create(&storage).unwrap();
            let transaction = db.begin_write().unwrap();
            // Use a valid Chimera storage layout. The opener intentionally
            // preserves and rebuilds databases that lack its required table.
            transaction
                .open_table(chimera_core::storage::NYANPASU_TABLE)
                .unwrap();
            transaction
                .open_table(USER_TABLE)
                .unwrap()
                .insert("theme", "legacy-user-value")
                .unwrap();
            transaction.commit().unwrap();
        }
        let original_storage = fs::read(&storage).unwrap();
        let capture = parking_lot::Mutex::new(None);
        let snapshot = run_with_spawn(&paths, &capture, |result, stderr| {
            child_spawn(&paths, result, stderr, "success")
        })
        .unwrap()
        .expect("the real child must create a verified pre-migration backup");
        assert!(snapshot.is_available());
        assert_eq!(
            fs::read(snapshot.path.join("config/chimera-config.yaml")).unwrap(),
            LEGACY.as_bytes()
        );
        assert_eq!(
            fs::read(snapshot.path.join("data/storage.db")).unwrap(),
            original_storage
        );
        assert!(paths.application_config_path().exists());
        assert!(capture.lock().is_none());
        // The parent must be able to reopen the original DB after the child
        // exits, proving migration does not retain the storage owner/lock.
        let db = redb::Database::open(&storage).expect("child released redb lock");
        let tx = db.begin_read().unwrap();
        assert_eq!(
            tx.open_table(USER_TABLE)
                .unwrap()
                .get("theme")
                .unwrap()
                .unwrap()
                .value(),
            "legacy-user-value"
        );
    }

    #[test]
    fn real_child_failure_preserves_original_files_and_reports_checked_backup() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let original = b"invalid: [";
        fs::write(paths.chimera_config_path(), original).unwrap();
        let capture = parking_lot::Mutex::new(None);
        let error = run_with_spawn(&paths, &capture, |result, stderr| {
            child_spawn(&paths, result, stderr, "failure")
        })
        .unwrap_err();
        assert!(error.to_string().contains("migration subprocess reported"));
        let report = capture.lock().clone().unwrap();
        assert_eq!(report.failure.stage, StartupStage::MigrationRun);
        assert_eq!(report.backups_dir, Some(paths.backups_dir()));
        assert_eq!(fs::read(paths.chimera_config_path()).unwrap(), original);
        assert!(!paths.application_config_path().exists());
        assert!(!paths.profiles_path().exists());
    }

    #[test]
    fn child_preserves_future_schema_revision_and_aborts_without_writes() {
        use crate::startup_failure::{FailureKind, RevisionFailure};
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let state_path = paths.app_config_dir().join("migration-state.yaml");
        let original = b"store_schema_version: 99\napp:\n  last_succeeded: 9.9.9\n";
        fs::write(&state_path, original).unwrap();
        let capture = parking_lot::Mutex::new(None);
        let err = run_with_spawn(&paths, &capture, |result, stderr| {
            child_spawn(&paths, result, stderr, "failure")
        })
        .unwrap_err();
        assert!(err.to_string().contains("NewerVersion"));
        let report = capture.lock().clone().unwrap();
        assert_eq!(report.failure.stage, StartupStage::MigrationPrepare);
        assert_eq!(report.failure.kind, FailureKind::NewerVersion);
        assert!(matches!(
            report.failure.revision,
            Some(RevisionFailure::NewerStoreSchema {
                read: 99,
                supported: 2,
                ..
            })
        ));
        assert!(report.backups_dir.is_none());
        assert_eq!(fs::read(&state_path).unwrap(), original);
        assert!(!paths.profiles_path().exists());
    }

    #[test]
    fn parent_drops_recovery_claim_when_child_snapshot_is_tampered() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let original = b"invalid: [";
        fs::write(paths.chimera_config_path(), original).unwrap();
        let capture = parking_lot::Mutex::new(None);
        let _ = run_with_spawn(&paths, &capture, |result, stderr| {
            let status = child_spawn(&paths, result, stderr, "failure")?;
            let committed = fs::read_dir(paths.backups_dir())?.next().unwrap()?.path();
            fs::write(
                committed.join("config/chimera-config.yaml"),
                b"tampered post-child",
            )?;
            Ok(status)
        })
        .unwrap_err();
        let report = capture.lock().clone().unwrap();
        assert_eq!(report.failure.stage, StartupStage::MigrationRun);
        assert!(
            report.backups_dir.is_none(),
            "snapshot must be reverified after child exit"
        );
        assert_eq!(fs::read(paths.chimera_config_path()).unwrap(), original);
        assert!(!paths.application_config_path().exists());
    }

    #[test]
    fn real_child_abnormal_exit_fails_closed_without_any_backup_claim() {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        fs::create_dir_all(paths.app_config_dir()).unwrap();
        fs::create_dir_all(paths.app_data_dir()).unwrap();
        let capture = parking_lot::Mutex::new(None);
        let error = run_with_spawn(&paths, &capture, |result, stderr| {
            child_spawn(&paths, result, stderr, "crash")
        })
        .unwrap_err();
        assert!(format!("{error:#}").contains("without a valid result"));
        let report = capture.lock().clone().unwrap();
        assert_eq!(report.failure.stage, StartupStage::MigrationRun);
        assert!(report.backups_dir.is_none());
        assert!(!paths.application_config_path().exists());
    }
}
