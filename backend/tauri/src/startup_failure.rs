//! Structured, source-preserving startup diagnostics.
//!
//! Converts the shared migration child failure diagnostics into Chimera's
//! existing startup report, while retaining platform-owned Core/Jobs, backup
//! and user-config classifications. This module is deliberately read-only:
//! it does not create backups, reset configuration or spawn a reporter.

use std::{error::Error as StdError, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    core::{
        backup::{BackupError, BackupInfo},
        corrupt,
        migration::MigrationCheckError,
        session_state,
    },
    utils::{path::PathResolver, recovery_io::IoFailure},
};
use chimera_migration::{
    child_result::{Failure as MigrationFailure, FailureStage},
    runner::ExecutionError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FailureKind {
    Directories,
    MigrationFailed,
    NewerVersion,
    UserConfig,
    JobsDatabase,
    Permission,
    Busy,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StartupFailure {
    pub stage: StartupStage,
    pub kind: FailureKind,
    pub path: Option<PathBuf>,
    pub detail: String,
    pub revision: Option<RevisionFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartupStage {
    Setup,
    Jobs,
    MigrationPrepare,
    MigrationRun,
    Storage,
    CoreHost,
    TypedClient,
}

pub(crate) use chimera_migration::child_result::RevisionFailure;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Report {
    pub failure: StartupFailure,
    pub version: String,
    pub logs_dir: PathBuf,
    /// None until a real, verified migration backup exists. A config directory
    /// is not a backup simply because a plausible backup path can be named.
    pub backups_dir: Option<PathBuf>,
}

impl StartupFailure {
    pub(crate) fn from_error(
        error: &(dyn StdError + 'static),
        stage: StartupStage,
        paths: &PathResolver,
    ) -> Self {
        let default_kind = match stage {
            StartupStage::Setup => FailureKind::Internal,
            StartupStage::Jobs => FailureKind::JobsDatabase,
            StartupStage::MigrationPrepare | StartupStage::MigrationRun => {
                FailureKind::MigrationFailed
            }
            StartupStage::Storage | StartupStage::CoreHost => FailureKind::Internal,
            StartupStage::TypedClient => FailureKind::UserConfig,
        };
        // The shared migration failure collector owns the original error
        // chain, typed revision and migration-owned path extraction. Tauri
        // only augments it with host/backup paths and UI classification.
        // Revision and path extraction is useful to every startup stage; the
        // shared FailureStage is internal here and only reaches the child
        // protocol for MigrationPrepare/MigrationRun reports.
        let shared = MigrationFailure::from_error(
            error,
            if stage == StartupStage::MigrationPrepare {
                FailureStage::RunnerConstruction
            } else {
                FailureStage::Migration
            },
            paths.app_config_dir().to_owned(),
            paths.backups_dir(),
            false,
            corrupt::error_io(error),
        );
        let mut path = shared.paths.last().cloned();
        let revision = shared.revision;

        let mut host_user_config = false;
        // RecordFailure exposes its independent persistence error as a field,
        // not a source (the original step failure remains the source).
        let chain = shared.chain;
        let mut current = Some(error);
        while let Some(error) = current {
            if let Some(execution) = error.downcast_ref::<ExecutionError>() {
                match execution {
                    ExecutionError::PersistState { path: actual, .. }
                    | ExecutionError::StampDocument { path: actual, .. }
                    | ExecutionError::LoadState { path: actual, .. }
                    | ExecutionError::ReadConfigDirectory { path: actual, .. } => {
                        path = Some(actual.clone());
                    }
                    ExecutionError::RecordFailure { persistence, .. } => {
                        if let Some(ExecutionError::PersistState { path: actual, .. }) =
                            persistence.downcast_ref::<ExecutionError>()
                        {
                            // A later source may reveal a more precise path to
                            // the original step failure; use this as fallback.
                            path.get_or_insert_with(|| actual.clone());
                        }
                    }
                    _ => {}
                }
            }
            if let Some(check) = error.downcast_ref::<MigrationCheckError>() {
                match check {
                    MigrationCheckError::Io { path: actual, .. }
                    | MigrationCheckError::Parse { path: actual, .. }
                    | MigrationCheckError::Storage { path: actual, .. }
                    | MigrationCheckError::Stamp { path: actual, .. } => {
                        path = Some(actual.clone());
                    }
                    MigrationCheckError::Unrecognized(_) => {}
                }
            }
            if let Some(error) = error.downcast_ref::<BackupError>() {
                match error {
                    BackupError::Io { path: actual, .. }
                    | BackupError::Snapshot { path: actual, .. }
                    | BackupError::InvalidNode { path: actual } => path = Some(actual.clone()),
                    BackupError::Manifest(_) | BackupError::Verification(_) => {}
                }
            }
            if let Some(session_state::Error::ReadState { path: actual, .. }) =
                error.downcast_ref::<session_state::Error>()
            {
                path = Some(actual.clone());
            }
            if let Some(corrupt::Error::PreserveEvidence { path: actual, .. }) =
                error.downcast_ref::<corrupt::Error>()
            {
                path = Some(actual.clone());
            }
            if let Some(host_error) =
                error.downcast_ref::<crate::core::actor_v2::local_host::Error>()
            {
                match host_error {
                    crate::core::actor_v2::local_host::Error::InspectConfig {
                        path: actual,
                        ..
                    } => {
                        path = Some(actual.clone());
                    }
                    crate::core::actor_v2::local_host::Error::ReadCoreKind {
                        path: actual, ..
                    } => {
                        path = Some(actual.clone());
                        host_user_config = true;
                    }
                    crate::core::actor_v2::local_host::Error::BuildManager { .. } => {}
                    crate::core::actor_v2::local_host::Error::InvalidPath { path: actual } => {
                        path = Some(actual.clone());
                    }
                }
            }
            current = error.source();
        }

        if let Some(revision) = &revision {
            path = Some(revision.path().clone());
        }
        // Classify the first IO failure in the primary error chain. If the
        // original step failed without an IO cause, the independently failed
        // state flush still needs to surface permission/busy diagnostics.
        let kind = match migration_io_failure(error) {
            Some(IoFailure::PermissionDenied) => FailureKind::Permission,
            Some(IoFailure::Transient) => FailureKind::Busy,
            _ if revision.as_ref().is_some_and(RevisionFailure::is_newer) => {
                FailureKind::NewerVersion
            }
            _ if host_user_config && stage == StartupStage::CoreHost => FailureKind::UserConfig,
            _ => default_kind,
        };
        let path = path.or_else(|| match stage {
            StartupStage::Setup => None,
            StartupStage::Jobs => Some(paths.jobs_path()),
            StartupStage::Storage => Some(paths.storage_path()),
            StartupStage::MigrationPrepare
            | StartupStage::MigrationRun
            | StartupStage::CoreHost
            | StartupStage::TypedClient => Some(paths.app_config_dir().to_owned()),
        });
        Self {
            stage,
            kind,
            path,
            detail: chain.join("\nCaused by: "),
            revision,
        }
    }
}

/// Preserve the original step IO classification when a later attempt to
/// persist `Failed` also errors. The second failure is held outside `source()`
/// by `ExecutionError::RecordFailure`, so normal error-chain scanning alone
/// cannot detect its permission or lock status.
pub(crate) fn migration_io_failure(error: &(dyn StdError + 'static)) -> Option<IoFailure> {
    let primary = corrupt::error_io(error);
    if matches!(
        primary,
        Some(IoFailure::PermissionDenied | IoFailure::Transient)
    ) {
        return primary;
    }
    let mut current = Some(error);
    while let Some(cause) = current {
        if let Some(ExecutionError::RecordFailure { persistence, .. }) =
            cause.downcast_ref::<ExecutionError>()
        {
            if let Some(secondary) = corrupt::error_io(persistence.as_ref()) {
                return Some(secondary);
            }
        }
        current = cause.source();
    }
    primary
}

impl Report {
    /// Convert a v2 migration-only Failure into the unchanged host-facing
    /// startup report. Never trust the child-provided root or backup hint as
    /// proof of recovery: the caller validates roots and the backup manifest.
    pub(crate) fn from_migration_failure(
        failure: MigrationFailure,
        paths: &PathResolver,
        backup: Option<&BackupInfo>,
    ) -> Self {
        let stage = match failure.stage {
            FailureStage::RunnerConstruction => StartupStage::MigrationPrepare,
            FailureStage::Backup | FailureStage::Migration => StartupStage::MigrationRun,
        };
        let kind = match failure.io_failure {
            Some(IoFailure::PermissionDenied) => FailureKind::Permission,
            Some(IoFailure::Transient) => FailureKind::Busy,
            _ if failure
                .revision
                .as_ref()
                .is_some_and(RevisionFailure::is_newer) =>
            {
                FailureKind::NewerVersion
            }
            _ => FailureKind::MigrationFailed,
        };
        let path = failure
            .revision
            .as_ref()
            .map(|revision| revision.path().clone())
            .or_else(|| failure.paths.last().cloned())
            .or_else(|| Some(paths.app_config_dir().to_owned()));
        let detail = failure.chain.join("\nCaused by: ");
        Self {
            failure: StartupFailure {
                stage,
                kind,
                path,
                detail,
                revision: failure.revision,
            },
            version: env!("CARGO_PKG_VERSION").to_owned(),
            logs_dir: paths.app_logs_dir(),
            backups_dir: backup
                .filter(|backup| backup.is_available())
                .and_then(|backup| backup.path.parent().map(|path| path.to_owned())),
        }
    }

    pub(crate) fn from_error(
        error: &(dyn StdError + 'static),
        stage: StartupStage,
        paths: &PathResolver,
    ) -> Self {
        Self {
            failure: StartupFailure::from_error(error, stage, paths),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            logs_dir: paths.app_logs_dir(),
            backups_dir: None,
        }
    }

    pub(crate) fn from_error_with_backup(
        error: &(dyn StdError + 'static),
        stage: StartupStage,
        paths: &PathResolver,
        backup: Option<&BackupInfo>,
    ) -> Self {
        let mut report = Self::from_error(error, stage, paths);
        // Report the root only after the migration Runner has a committed,
        // still-present snapshot with a manifest. Never infer availability
        // from an existing backups directory alone.
        report.backups_dir = backup
            .filter(|backup| backup.is_available())
            .and_then(|backup| backup.path.parent().map(|path| path.to_owned()));
        report
    }
}

/// Annotate only the currently identifiable startup boundaries. The original
/// source error remains in the anyhow chain for the existing Tauri caller.
/// A dedicated crash-reporter window and pre-Tauri paths are *not* implemented.
pub(crate) fn report_boundary<T>(
    result: anyhow::Result<T>,
    stage: StartupStage,
    paths: &PathResolver,
) -> anyhow::Result<T> {
    report_boundary_with_backup(result, stage, paths, None)
}

pub(crate) fn report_boundary_with_backup<T>(
    result: anyhow::Result<T>,
    stage: StartupStage,
    paths: &PathResolver,
    backup: Option<&BackupInfo>,
) -> anyhow::Result<T> {
    report_boundary_with_backup_and_capture(result, stage, paths, backup, None)
}

pub(crate) fn report_boundary_with_backup_and_capture<T>(
    result: anyhow::Result<T>,
    stage: StartupStage,
    paths: &PathResolver,
    backup: Option<&BackupInfo>,
    capture: Option<&parking_lot::Mutex<Option<Report>>>,
) -> anyhow::Result<T> {
    result.map_err(|source| {
        let report = Report::from_error_with_backup(source.as_ref(), stage, paths, backup);
        if let Some(capture) = capture {
            *capture.lock() = Some(report.clone());
        }
        tracing::error!(
            stage = ?report.failure.stage,
            kind = ?report.failure.kind,
            path = ?report.failure.path,
            revision = ?report.failure.revision,
            backups_dir = ?report.backups_dir,
            detail = %report.failure.detail,
            "startup boundary failed"
        );
        eprintln!(
            "Chimera startup failed at {:?} ({:?}){}: {}",
            report.failure.stage,
            report.failure.kind,
            report
                .failure
                .path
                .as_ref()
                .map(|path| format!(" [{}]", path.display()))
                .unwrap_or_default(),
            report.failure.detail
        );
        source.context(format!(
            "startup {:?} failed ({:?})",
            report.failure.stage, report.failure.kind
        ))
    })
}

/// Presents a structured setup failure after Tauri has returned its setup error.
/// The builder error does not retain the stage/path classification in a form
/// the entry point can use, so callers retain this report across that boundary.
pub(crate) fn show_report(report: &Report) {
    let message = report_message(report);
    tracing::error!(
        stage = ?report.failure.stage,
        kind = ?report.failure.kind,
        path = ?report.failure.path,
        revision = ?report.failure.revision,
        backups_dir = ?report.backups_dir,
        detail = %report.failure.detail,
        "application startup failed during Tauri setup"
    );
    eprintln!("{message}");
    crate::utils::dialog::error_dialog(message);
}

fn report_message(report: &Report) -> String {
    let failure = &report.failure;
    let location = failure
        .path
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unresolved path".to_owned());
    let recovery = report
        .backups_dir
        .as_ref()
        .map(|path| format!("Verified migration backup: {}", path.display()))
        .unwrap_or_else(|| {
            "Check the reported path and permissions; do not remove existing configuration files."
                .to_owned()
        });
    format!(
        "Clash Chimera could not start during {:?} ({:?}).\n\nPath: {location}\n\n{recovery}\n\n{}",
        failure.stage, failure.kind, failure.detail
    )
}

/// Presents pre-Tauri failures without starting a reporter or event loop.
pub(crate) fn show_one_shot(
    kind: FailureKind,
    path: Option<PathBuf>,
    error: &(dyn StdError + 'static),
    recovery_key: Option<&str>,
) {
    let detail = snafu::Report::from_error(error).to_string();
    tracing::error!(?kind, ?path, %detail, "application startup failed before Tauri setup");
    let message = one_shot_message(kind, path.as_deref(), &detail, recovery_key);
    eprintln!("{message}");
    crate::utils::dialog::error_dialog(message);
}

/// The pre-Tauri discovery stage has no Setup owner or migration revision.
/// Keep it distinct from the structured reports created at actual Setup boundaries.
fn one_shot_message(
    kind: FailureKind,
    path: Option<&std::path::Path>,
    detail: &str,
    recovery_key: Option<&str>,
) -> String {
    let location = path
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unresolved path".to_owned());
    let recovery = recovery_key
        .map(|key| format!("Recovery action: {key}"))
        .unwrap_or_else(|| {
            "Check the reported path and permissions; do not remove existing configuration files."
                .to_owned()
        });
    format!(
        "Clash Chimera could not start ({kind:?}).\n\nPath: {location}\n\n{recovery}\n\n{detail}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::{
        runner::RevisionError,
        store::{MigrationStore, SchemaError},
    };

    fn fixture() -> (tempfile::TempDir, PathResolver) {
        let temp = tempfile::tempdir().unwrap();
        let paths =
            PathResolver::with_base_dirs(temp.path().join("config"), temp.path().join("data"));
        std::fs::create_dir_all(paths.app_config_dir()).unwrap();
        std::fs::create_dir_all(paths.app_data_dir()).unwrap();
        (temp, paths)
    }

    #[test]
    fn newer_and_older_schema_reports_keep_exact_revision_and_original_file() {
        for (value, newer) in [(99, true), (1, false)] {
            let (temp, paths) = fixture();
            let path = paths.app_config_dir().join("migration-state.yaml");
            let original = format!("store_schema_version: {value}\n");
            std::fs::write(&path, &original).unwrap();
            let error = MigrationStore::load(&path).unwrap_err();
            let report = Report::from_error(error.as_ref(), StartupStage::MigrationPrepare, &paths);
            assert_eq!(
                report.failure.kind,
                if newer {
                    FailureKind::NewerVersion
                } else {
                    FailureKind::MigrationFailed
                }
            );
            assert_eq!(report.failure.path, Some(path.clone()));
            match report.failure.revision.as_ref().unwrap() {
                RevisionFailure::NewerStoreSchema {
                    path: p,
                    read,
                    supported,
                } if newer => assert_eq!((p, *read, *supported), (&path, value, 2)),
                RevisionFailure::OlderStoreSchema {
                    path: p,
                    read,
                    supported,
                } if !newer => assert_eq!((p, *read, *supported), (&path, value, 2)),
                other => panic!("unexpected revision {other:?}"),
            }
            assert_eq!(report.backups_dir, None);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
            assert_eq!(
                std::fs::read_dir(paths.app_config_dir()).unwrap().count(),
                1
            );
            let json = serde_json::to_vec(&report).unwrap();
            assert_eq!(serde_json::from_slice::<Report>(&json).unwrap(), report);
            drop(temp);
        }
    }

    #[test]
    fn future_document_and_applied_module_are_reported_without_string_heuristics() {
        let (_temp, paths) = fixture();
        let document = paths.profiles_path();
        let state = paths.app_config_dir().join("migration-state.yaml");
        let cases = [
            (
                RevisionError::NewerDocument {
                    path: document.clone(),
                    read: 42,
                    supported: 4,
                },
                document.clone(),
            ),
            (
                RevisionError::NewerAppliedModule {
                    path: state.clone(),
                    module: "profiles".into(),
                    read: 42,
                    supported: 4,
                },
                state.clone(),
            ),
        ];
        for (error, path) in cases {
            let report = Report::from_error(&error, StartupStage::MigrationRun, &paths);
            assert_eq!(report.failure.kind, FailureKind::NewerVersion);
            assert_eq!(report.failure.path, Some(path));
            assert!(report.failure.revision.is_some());
            assert!(report.backups_dir.is_none());
        }
    }

    #[test]
    fn migration_consistency_errors_report_structured_paths_without_claiming_newer_version() {
        let (_temp, paths) = fixture();
        let document = paths.profiles_path();
        let state = paths.app_config_dir().join("migration-state.yaml");
        let cases = [
            (
                RevisionError::DocumentBehindState {
                    path: document.clone(),
                    state_path: state.clone(),
                    module: "profiles".into(),
                    read: 2,
                    applied: 4,
                },
                document.clone(),
                "document_behind_state",
            ),
            (
                RevisionError::MissingDocumentStamp {
                    path: document.clone(),
                    state_path: state.clone(),
                },
                document.clone(),
                "missing_document_stamp",
            ),
            (
                RevisionError::FilesBehindState {
                    state_path: state.clone(),
                    module: "profiles".into(),
                    applied: 4,
                    reason: "profiles missing expected field".into(),
                },
                state.clone(),
                "files_behind_state",
            ),
        ];
        for (error, expected_path, expected_kind) in cases {
            // Keep the source reachable when Tauri decorates the error at setup.
            let wrapped = anyhow::Error::new(error).context("migration startup boundary");
            let report = Report::from_error(wrapped.as_ref(), StartupStage::MigrationRun, &paths);
            assert_eq!(report.failure.kind, FailureKind::MigrationFailed);
            assert_eq!(report.failure.path, Some(expected_path));
            let revision = report
                .failure
                .revision
                .as_ref()
                .expect("typed revision mismatch");
            let json = serde_json::to_value(revision).unwrap();
            assert_eq!(json["kind"], expected_kind);
            assert_eq!(
                serde_json::from_value::<Report>(serde_json::to_value(&report).unwrap()).unwrap(),
                report
            );
            assert_eq!(report.backups_dir, None);
        }
    }

    #[test]
    fn migration_io_permission_and_busy_take_priority_over_generic_failure() {
        for (kind, expected) in [
            (
                std::io::ErrorKind::PermissionDenied,
                FailureKind::Permission,
            ),
            (std::io::ErrorKind::WouldBlock, FailureKind::Busy),
        ] {
            let (_temp, paths) = fixture();
            let path = paths.app_config_dir().join("application.yaml");
            let error = MigrationCheckError::Io {
                path: path.clone(),
                source: std::io::Error::from(kind),
            };
            let report = Report::from_error(&error, StartupStage::MigrationPrepare, &paths);
            assert_eq!(report.failure.kind, expected);
            assert_eq!(report.failure.path, Some(path));
            assert_eq!(report.backups_dir, None);
        }
    }

    #[test]
    fn typed_migration_execution_errors_reach_startup_report_with_original_sources() {
        use chimera_migration::runner::ExecutionError;
        let (_temp, paths) = fixture();
        let config = paths.application_config_path();
        let source = MigrationCheckError::Io {
            path: config.clone(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        let error = anyhow::Error::new(ExecutionError::CheckStep {
            step: "typed_config/split_legacy_config".into(),
            source,
        })
        .context("failed at migration boundary");
        let report = Report::from_error(error.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::Permission);
        assert_eq!(report.failure.path, Some(config));
        assert!(
            report
                .failure
                .detail
                .contains("failed to check migration typed_config/split_legacy_config")
        );
        assert!(
            report
                .failure
                .detail
                .contains("failed at migration boundary")
        );
        assert!(report.failure.revision.is_none());
        assert!(report.backups_dir.is_none());

        // The original runtime failure remains reachable through the source
        // chain even when writing the failed state *also* fails.
        let error = anyhow::Error::new(ExecutionError::RecordFailure {
            step: "storage/hotkeys_to_kv".into(),
            source: ExecutionError::RunStep {
                step: "storage/hotkeys_to_kv".into(),
                source: anyhow::anyhow!("original KV write denied"),
            }
            .into(),
            persistence: anyhow::anyhow!("migration-state atomic write denied"),
        })
        .context("failed at migration boundary");
        let report = Report::from_error(error.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::MigrationFailed);
        assert!(report.failure.detail.contains("original KV write denied"));
        assert!(
            report
                .failure
                .detail
                .contains("migration-state atomic write denied")
        );
        assert!(report.failure.detail.contains("storage/hotkeys_to_kv"));
        assert_eq!(report.backups_dir, None);
    }

    #[test]
    fn migration_load_state_keeps_newer_schema_and_original_state_file() {
        let (_temp, paths) = fixture();
        let state_path = paths.app_config_dir().join("migration-state.yaml");
        let original = b"store_schema_version: 99\n";
        std::fs::write(&state_path, original).unwrap();
        let error = crate::core::migration::Runner::with_paths(paths.clone(), false)
            .expect_err("future migration state cannot be used by this release");
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::LoadState { path, source })
                if path == &state_path && matches!(source.downcast_ref::<SchemaError>(),
                    Some(SchemaError::Newer { read: 99, supported: 2, .. }))));
        let report = Report::from_error(error.as_ref(), StartupStage::MigrationPrepare, &paths);
        assert_eq!(report.failure.kind, FailureKind::NewerVersion);
        assert_eq!(report.failure.path, Some(state_path.clone()));
        assert!(matches!(
            report.failure.revision,
            Some(RevisionFailure::NewerStoreSchema {
                read: 99,
                supported: 2,
                ..
            })
        ));
        assert!(report.backups_dir.is_none());
        assert_eq!(std::fs::read(&state_path).unwrap(), original);
    }

    #[test]
    fn read_config_directory_failure_is_reported_with_exact_path_and_io_kind() {
        let (_temp, paths) = fixture();
        let config_path = paths.app_config_dir().to_owned();
        let error = anyhow::Error::new(ExecutionError::ReadConfigDirectory {
            path: config_path.clone(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        })
        .context("migration snapshot inspection");
        let report = Report::from_error(error.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::Permission);
        assert_eq!(report.failure.path, Some(config_path));
        assert!(
            report
                .failure
                .detail
                .contains("migration snapshot inspection")
        );
        assert!(report.backups_dir.is_none());
    }

    #[test]
    fn typed_migration_persistence_backup_and_stamp_failures_report_exact_paths() {
        let (_temp, paths) = fixture();
        let state_path = paths.app_config_dir().join("migration-state.yaml");
        let persist = anyhow::Error::new(ExecutionError::PersistState {
            operation: "successful migration state".to_owned(),
            path: state_path.clone(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied).into(),
        })
        .context("migration startup boundary");
        let report = Report::from_error(persist.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::Permission);
        assert_eq!(report.failure.path, Some(state_path.clone()));
        assert!(report.failure.detail.contains("successful migration state"));
        assert!(report.backups_dir.is_none());

        let unsafe_backup = paths.backups_dir();
        let create = anyhow::Error::new(ExecutionError::CreateBackup {
            source: BackupError::InvalidNode {
                path: unsafe_backup.clone(),
            }
            .into(),
        });
        let report = Report::from_error(create.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::MigrationFailed);
        assert_eq!(report.failure.path, Some(unsafe_backup));
        assert!(
            report
                .failure
                .detail
                .contains("failed to back up the config")
        );
        assert!(
            report.backups_dir.is_none(),
            "a failed snapshot is never advertised"
        );

        let profiles = paths.profiles_path();
        let stamp = anyhow::Error::new(ExecutionError::StampDocument {
            module: "profiles".to_owned(),
            path: profiles.clone(),
            source: anyhow::anyhow!("refused to replace a symbolic link"),
        });
        let report = Report::from_error(stamp.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::MigrationFailed);
        assert_eq!(report.failure.path, Some(profiles));
        assert!(report.failure.detail.contains("symbolic link"));
        assert!(report.backups_dir.is_none());
    }

    #[test]
    fn secondary_persistence_error_preserves_original_failure_and_permission_diagnosis() {
        let (_temp, paths) = fixture();
        let state_path = paths.app_config_dir().join("migration-state.yaml");
        let error = anyhow::Error::new(ExecutionError::RecordFailure {
            step: "storage/hotkeys_to_kv".to_owned(),
            source: ExecutionError::RunStep {
                step: "storage/hotkeys_to_kv".to_owned(),
                source: anyhow::anyhow!("original malformed hotkey data"),
            }
            .into(),
            persistence: ExecutionError::PersistState {
                operation: "storage/hotkeys_to_kv failed state".to_owned(),
                path: state_path.clone(),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied).into(),
            }
            .into(),
        })
        .context("migration startup boundary");
        let report = Report::from_error(error.as_ref(), StartupStage::MigrationRun, &paths);
        assert_eq!(report.failure.kind, FailureKind::Permission);
        assert_eq!(report.failure.path, Some(state_path));
        assert!(
            report
                .failure
                .detail
                .contains("original malformed hotkey data")
        );
        assert!(
            report
                .failure
                .detail
                .contains("failed to persist storage/hotkeys_to_kv failed state")
        );
        assert!(report.failure.revision.is_none());
        assert!(report.backups_dir.is_none());
    }

    #[test]
    fn jobs_and_user_config_have_stage_specific_safe_fallback_paths() {
        let (_temp, paths) = fixture();
        let error = std::io::Error::from(std::io::ErrorKind::InvalidData);
        let jobs = Report::from_error(&error, StartupStage::Jobs, &paths);
        assert_eq!(jobs.failure.kind, FailureKind::JobsDatabase);
        assert_eq!(jobs.failure.path, Some(paths.jobs_path()));
        let user = Report::from_error(&error, StartupStage::TypedClient, &paths);
        assert_eq!(user.failure.kind, FailureKind::UserConfig);
        assert_eq!(user.failure.path, Some(paths.app_config_dir().to_owned()));
        assert!(!user.failure.detail.is_empty());

        let setup = Report::from_error(&error, StartupStage::Setup, &paths);
        assert_eq!(setup.failure.kind, FailureKind::Internal);
        assert_eq!(setup.failure.path, None);
    }

    #[tokio::test]
    async fn malformed_typed_core_config_is_reported_at_the_host_boundary() {
        let (temp, paths) = fixture();
        let typed_config = paths.application_config_path();
        std::fs::write(&typed_config, "invalid: [\n").unwrap();
        std::fs::write(paths.chimera_config_path(), "core: mihomo\n").unwrap();
        let result = crate::core::actor_v2::local_host::build(
            &paths,
            &temp.path().join("helper.exe"),
            std::sync::Arc::new(chimera_utils::process::AtomicStopStrategy::new(
                chimera_utils::process::StopStrategy::default(),
            )),
        )
        .await;
        let Err(error) = result else {
            panic!("the malformed typed config must be rejected");
        };

        let report = Report::from_error(&error, StartupStage::CoreHost, &paths);
        assert_eq!(report.failure.kind, FailureKind::UserConfig);
        assert_eq!(report.failure.path, Some(typed_config));
        assert!(report.failure.detail.contains("failed to read core kind"));
    }

    #[test]
    fn invalid_host_path_reports_the_exact_path() {
        let (_temp, paths) = fixture();
        let actual = paths.app_data_dir().join("unrepresentable-native-path");
        let error = crate::core::actor_v2::local_host::Error::InvalidPath {
            path: actual.clone(),
        };

        let report = Report::from_error(&error, StartupStage::CoreHost, &paths);
        assert_eq!(report.failure.kind, FailureKind::Internal);
        assert_eq!(report.failure.path, Some(actual));
    }

    #[test]
    fn backup_dir_is_advertised_only_for_a_committed_existing_snapshot() {
        use crate::core::backup::{self, BackupKind, BackupRequest, StorageSource};
        let (_temp, paths) = fixture();
        let legacy = paths.chimera_config_path();
        std::fs::write(&legacy, b"legacy yaml").unwrap();
        let storage = paths.storage_path();
        let target = semver::Version::new(2, 0, 0);
        let backup = backup::create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(&storage),
            kind: BackupKind::Migration {
                from: None,
                target: &target,
            },
            now: time::OffsetDateTime::now_utc(),
        })
        .unwrap();
        let error = std::io::Error::from(std::io::ErrorKind::InvalidData);
        let report = Report::from_error_with_backup(
            &error,
            StartupStage::MigrationRun,
            &paths,
            Some(&backup),
        );
        assert_eq!(report.backups_dir, Some(paths.backups_dir()));
        let backed_up = backup.path.join("config/chimera-config.yaml");
        assert_eq!(std::fs::read(&backed_up).unwrap(), b"legacy yaml");
        std::fs::write(&backed_up, b"tampered!!!").unwrap();
        let corrupt = Report::from_error_with_backup(
            &error,
            StartupStage::MigrationRun,
            &paths,
            Some(&backup),
        );
        assert!(
            corrupt.backups_dir.is_none(),
            "a damaged snapshot must not be advertised"
        );
        std::fs::write(&backed_up, b"legacy yaml").unwrap();
        assert_eq!(
            Report::from_error_with_backup(
                &error,
                StartupStage::MigrationRun,
                &paths,
                Some(&backup)
            )
            .backups_dir,
            Some(paths.backups_dir())
        );
        std::fs::remove_dir_all(&backup.path).unwrap();
        let stale = Report::from_error_with_backup(
            &error,
            StartupStage::MigrationRun,
            &paths,
            Some(&backup),
        );
        assert!(
            stale.backups_dir.is_none(),
            "missing snapshot is not a valid recovery claim"
        );
        let absent =
            Report::from_error_with_backup(&error, StartupStage::MigrationRun, &paths, None);
        assert!(absent.backups_dir.is_none());
    }

    #[test]
    fn boundary_preserves_source_error_chain_and_does_not_write_report_files() {
        let (_temp, paths) = fixture();
        let path = paths.app_config_dir().join("application.yaml");
        let root = MigrationCheckError::Io {
            path: path.clone(),
            source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        };
        let error = anyhow::Error::new(root).context("migration boundary");
        let annotated =
            report_boundary::<()>(Err(error), StartupStage::MigrationRun, &paths).unwrap_err();
        assert!(
            annotated
                .to_string()
                .contains("startup MigrationRun failed")
        );
        assert!(annotated.downcast_ref::<MigrationCheckError>().is_some());
        assert!(
            std::fs::read_dir(paths.app_config_dir())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn captured_storage_report_keeps_its_path_and_verified_backup_for_the_setup_dialog() {
        use crate::core::backup::{self, BackupKind, BackupRequest, StorageSource};

        let (_temp, paths) = fixture();
        std::fs::write(paths.chimera_config_path(), b"legacy yaml").unwrap();
        let storage = paths.storage_path();
        let target = semver::Version::new(2, 0, 0);
        let backup = backup::create_backup(&BackupRequest {
            paths: &paths,
            storage: StorageSource::File(&storage),
            kind: BackupKind::Migration {
                from: None,
                target: &target,
            },
            now: time::OffsetDateTime::now_utc(),
        })
        .unwrap();
        let capture = parking_lot::Mutex::new(None);
        let error = anyhow::anyhow!("storage database is malformed");

        let annotated = report_boundary_with_backup_and_capture::<()>(
            Err(error),
            StartupStage::Storage,
            &paths,
            Some(&backup),
            Some(&capture),
        )
        .unwrap_err();

        assert!(annotated.to_string().contains("startup Storage failed"));
        let report = capture.lock().clone().expect("startup report was captured");
        assert_eq!(report.failure.stage, StartupStage::Storage);
        assert_eq!(report.failure.kind, FailureKind::Internal);
        assert_eq!(report.failure.path, Some(paths.storage_path()));
        assert_eq!(report.backups_dir, Some(paths.backups_dir()));

        let message = report_message(&report);
        assert!(message.contains("during Storage (Internal)"));
        assert!(message.contains(&format!("Path: {}", paths.storage_path().display())));
        assert!(message.contains(&format!(
            "Verified migration backup: {}",
            paths.backups_dir().display()
        )));
        assert!(!message.contains("Path: unresolved path"));
    }
}
