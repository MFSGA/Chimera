//! Versioned migration-child result, exchanged only between the isolated
//! migration process and its parent. Ref: nyanpasu-migration/child_result.rs.
//!
//! The application shell supplies its diagnostic `Report` and the verified
//! backup producer. This shared crate owns the wire format and safe file IO.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

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
