//! Structured, source-preserving startup diagnostics.
//!
//! Maps the same failure kinds/revision cases as ref startup_failure.rs and
//! core/migration/child_result.rs, but Chimera has not migrated the independent
//! crash-reporter process or subprocess migration protocol. This layer is
//! deliberately read-only: it does not create backups, reset configurations,
//! launch another process or display a GUI.

use std::{error::Error as StdError, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    core::{
        backup::{BackupError, BackupInfo},
        corrupt,
        migration::{MigrationCheckError, runner::RevisionError, store::SchemaError},
        session_state,
    },
    utils::{path::PathResolver, recovery_io::IoFailure},
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
    Jobs,
    MigrationPrepare,
    MigrationRun,
    CoreHost,
    TypedClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RevisionFailure {
    NewerStoreSchema {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    OlderStoreSchema {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    NewerDocument {
        path: PathBuf,
        read: u64,
        supported: u64,
    },
    NewerAppliedModule {
        path: PathBuf,
        module: String,
        read: u64,
        supported: u64,
    },
}

impl RevisionFailure {
    fn path(&self) -> &PathBuf {
        match self {
            Self::NewerStoreSchema { path, .. }
            | Self::OlderStoreSchema { path, .. }
            | Self::NewerDocument { path, .. }
            | Self::NewerAppliedModule { path, .. } => path,
        }
    }

    fn is_newer(&self) -> bool {
        !matches!(self, Self::OlderStoreSchema { .. })
    }
}

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
            StartupStage::Jobs => FailureKind::JobsDatabase,
            StartupStage::MigrationPrepare | StartupStage::MigrationRun => {
                FailureKind::MigrationFailed
            }
            StartupStage::CoreHost => FailureKind::Internal,
            StartupStage::TypedClient => FailureKind::UserConfig,
        };
        let mut path = None;
        let mut revision = None;
        let mut host_user_config = false;
        let mut chain = Vec::new();
        let mut current = Some(error);
        while let Some(error) = current {
            chain.push(error.to_string());

            if let Some(schema) = error.downcast_ref::<SchemaError>() {
                revision = Some(match schema {
                    SchemaError::Newer {
                        path,
                        read,
                        supported,
                    } => RevisionFailure::NewerStoreSchema {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    },
                    SchemaError::Older {
                        path,
                        read,
                        supported,
                    } => RevisionFailure::OlderStoreSchema {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    },
                });
            }
            if let Some(revision_error) = error.downcast_ref::<RevisionError>() {
                revision = Some(match revision_error {
                    RevisionError::NewerDocument {
                        path,
                        read,
                        supported,
                    } => RevisionFailure::NewerDocument {
                        path: path.clone(),
                        read: *read,
                        supported: *supported,
                    },
                    RevisionError::NewerAppliedModule {
                        path,
                        module,
                        read,
                        supported,
                    } => RevisionFailure::NewerAppliedModule {
                        path: path.clone(),
                        module: module.clone(),
                        read: *read,
                        supported: *supported,
                    },
                });
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
        let kind = match corrupt::error_io(error) {
            Some(IoFailure::PermissionDenied) => FailureKind::Permission,
            Some(IoFailure::Transient) => FailureKind::Busy,
            _ if revision.as_ref().is_some_and(RevisionFailure::is_newer) => {
                FailureKind::NewerVersion
            }
            _ if host_user_config && stage == StartupStage::CoreHost => FailureKind::UserConfig,
            _ => default_kind,
        };
        let path = path.or_else(|| {
            Some(match stage {
                StartupStage::Jobs => paths.jobs_path(),
                StartupStage::MigrationPrepare
                | StartupStage::MigrationRun
                | StartupStage::CoreHost
                | StartupStage::TypedClient => paths.app_config_dir().to_owned(),
            })
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

impl Report {
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
    result.map_err(|source| {
        let report = Report::from_error_with_backup(source.as_ref(), stage, paths, backup);
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
    use crate::core::migration::{runner::RevisionError, store::MigrationStore};

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
}
