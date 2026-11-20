//! Versioned migration-child result, exchanged only between the isolated
//! migration process and its parent. Ref: nyanpasu-migration/child_result.rs.
//!
//! The application shell supplies its diagnostic `Report` and the verified
//! backup producer. This shared crate owns the wire format and safe file IO.
use std::{
    error::Error as StdError,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use chimera_utils::io::IoFailure;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
    module::MigrationCheckError,
    runner::{ExecutionError, RevisionError},
    store::SchemaError,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RevisionFailure {
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
    DocumentBehindState {
        path: PathBuf,
        state_path: PathBuf,
        module: String,
        read: u64,
        applied: u64,
    },
    MissingDocumentStamp {
        path: PathBuf,
        state_path: PathBuf,
    },
    FilesBehindState {
        state_path: PathBuf,
        module: String,
        applied: u64,
        reason: String,
    },
}

impl RevisionFailure {
    pub fn path(&self) -> &PathBuf {
        match self {
            Self::NewerStoreSchema { path, .. }
            | Self::OlderStoreSchema { path, .. }
            | Self::NewerDocument { path, .. }
            | Self::NewerAppliedModule { path, .. }
            | Self::DocumentBehindState { path, .. }
            | Self::MissingDocumentStamp { path, .. } => path,
            Self::FilesBehindState { state_path, .. } => state_path,
        }
    }

    pub fn is_newer(&self) -> bool {
        matches!(
            self,
            Self::NewerStoreSchema { .. }
                | Self::NewerDocument { .. }
                | Self::NewerAppliedModule { .. }
        )
    }
}

/// Upstream-compatible stage in the migration-only subprocess. Unlike the
/// broader Tauri StartupStage, this is independent of windows and runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    RunnerConstruction,
    Backup,
    Migration,
}

/// Information collected from the original error chain before crossing the
/// child/parent boundary. The application shell may enrich it with platform
/// paths and classify it for its existing startup UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("migration child failed during {stage:?}: {}", .chain.join(": "))]
pub struct Failure {
    pub stage: FailureStage,
    pub config_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub paths: Vec<PathBuf>,
    pub revision: Option<RevisionFailure>,
    pub chain: Vec<String>,
    pub backup_available: bool,
    #[serde(default)]
    pub io_failure: Option<IoFailure>,
}

impl Failure {
    /// Collect original sources, revision details and migration-owned paths;
    /// do not flatten errors to strings before examining their types.
    /// Tauri supplies platform IO classification and verified backup status.
    pub fn from_error(
        error: &(dyn StdError + 'static),
        stage: FailureStage,
        config_dir: PathBuf,
        backups_dir: PathBuf,
        backup_available: bool,
        io_failure: Option<IoFailure>,
    ) -> Self {
        let mut chain = Vec::new();
        let mut paths = Vec::new();
        let mut revision = None;
        let mut current = Some(error);
        while let Some(cause) = current {
            chain.push(cause.to_string());
            if let Some(schema) = cause.downcast_ref::<SchemaError>() {
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
            if let Some(revision_error) = cause.downcast_ref::<RevisionError>() {
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
                    RevisionError::DocumentBehindState {
                        path,
                        state_path,
                        module,
                        read,
                        applied,
                    } => RevisionFailure::DocumentBehindState {
                        path: path.clone(),
                        state_path: state_path.clone(),
                        module: module.clone(),
                        read: *read,
                        applied: *applied,
                    },
                    RevisionError::MissingDocumentStamp { path, state_path } => {
                        RevisionFailure::MissingDocumentStamp {
                            path: path.clone(),
                            state_path: state_path.clone(),
                        }
                    }
                    RevisionError::FilesBehindState {
                        state_path,
                        module,
                        applied,
                        reason,
                    } => RevisionFailure::FilesBehindState {
                        state_path: state_path.clone(),
                        module: module.clone(),
                        applied: *applied,
                        reason: reason.clone(),
                    },
                });
            }
            let found_path = if let Some(check) = cause.downcast_ref::<MigrationCheckError>() {
                match check {
                    MigrationCheckError::Io { path, .. }
                    | MigrationCheckError::Parse { path, .. }
                    | MigrationCheckError::Storage { path, .. }
                    | MigrationCheckError::Stamp { path, .. } => Some(path),
                    MigrationCheckError::Unrecognized(_) => None,
                }
            } else if let Some(execution) = cause.downcast_ref::<ExecutionError>() {
                match execution {
                    ExecutionError::PersistState { path, .. }
                    | ExecutionError::StampDocument { path, .. }
                    | ExecutionError::LoadState { path, .. }
                    | ExecutionError::ReadConfigDirectory { path, .. } => Some(path),
                    _ => None,
                }
            } else if let Some(error) = cause.downcast_ref::<chimera_persistence::write::Error>() {
                match error {
                    chimera_persistence::write::Error::StampDocument { path, .. }
                    | chimera_persistence::write::Error::SerializeDocument { path, .. }
                    | chimera_persistence::write::Error::ValidateDestination { path, .. }
                    | chimera_persistence::write::Error::CreateDirectory { path, .. }
                    | chimera_persistence::write::Error::WriteFile { path, .. } => Some(path),
                }
            } else if let Some(chimera_persistence::Error::PreserveEvidence { path, .. }) =
                cause.downcast_ref::<chimera_persistence::Error>()
            {
                Some(path)
            } else {
                None
            };
            if let Some(path) = found_path
                && !paths.contains(path)
            {
                paths.push(path.clone());
            }
            current = cause.source();
        }
        if let Some(revision) = &revision
            && !paths.contains(revision.path())
        {
            paths.push(revision.path().clone());
        }
        Self {
            stage,
            config_dir,
            backups_dir,
            paths,
            revision,
            chain,
            backup_available,
            io_failure,
        }
    }
}

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_REPORT_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ChildOutcome<R> {
    Success {
        backup_name: Option<String>,
    },
    Failure {
        report: R,
        backup_name: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ChildEnvelope<Outcome> {
    protocol_version: u32,
    outcome: Outcome,
}

/// An internal child never overwrites an existing result node. The parent
/// creates a private temporary directory and passes a name that is absent.
/// This is deliberately stricter than ordinary atomic config-file writes.
pub fn validate_new_result_destination(result: &Path) -> anyhow::Result<()> {
    let parent = result
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("migration result has no private parent directory"))?;
    let parent_metadata = chimera_persistence::retry_io(|| fs::symlink_metadata(parent))
        .with_context(|| {
            format!(
                "migration result parent is not accessible {}",
                parent.display()
            )
        })?;
    if !parent_metadata.is_dir() || chimera_persistence::is_link(&parent_metadata) {
        bail!(
            "migration result parent is not a real directory: {}",
            parent.display()
        );
    }
    match chimera_persistence::retry_io(|| fs::symlink_metadata(result)) {
        Ok(_) => bail!(
            "refusing to overwrite existing migration result node {}",
            result.display()
        ),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("failed to inspect migration result {}", result.display())),
    }
}

/// Encode and commit a result only to a new node in the parent's private
/// directory. Both the preflight and the commit use the same destination
/// validation policy. The actual write is atomic and crash-resistant.
pub fn write_outcome<R: Serialize>(result: &Path, outcome: &ChildOutcome<R>) -> anyhow::Result<()> {
    validate_new_result_destination(result)?;
    let body = serde_json::to_vec(&ChildEnvelope {
        protocol_version: PROTOCOL_VERSION,
        outcome,
    })?;
    crate::fs::atomic_write(result, &body)
        .with_context(|| format!("failed to save migration result {}", result.display()))
}

/// The parent fails closed on missing, linked, oversized, malformed or newer
/// results. Never treat a written result as proof of backup availability.
pub fn read_outcome<R: DeserializeOwned>(path: &Path) -> anyhow::Result<ChildOutcome<R>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("migration result was not written at {}", path.display()))?;
    if !metadata.is_file()
        || chimera_persistence::is_link(&metadata)
        || metadata.len() > MAX_REPORT_BYTES
    {
        bail!(
            "migration result is not a valid bounded regular file: {}",
            path.display()
        );
    }
    let envelope: ChildEnvelope<ChildOutcome<R>> = serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("invalid migration result {}", path.display()))?;
    if envelope.protocol_version != PROTOCOL_VERSION {
        bail!(
            "unsupported migration result protocol {}",
            envelope.protocol_version
        );
    }
    Ok(envelope.outcome)
}

/// Only an expected migration snapshot basename can be returned from a child;
/// the parent separately checks the actual manifest and all file hashes.
pub fn validate_backup_name(name: &str, migration_prefix: &str) -> anyhow::Result<()> {
    if !name.starts_with(migration_prefix)
        || name.len() > 200
        || name.contains("..")
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        bail!("unsafe migration backup name in child result");
    }
    Ok(())
}
