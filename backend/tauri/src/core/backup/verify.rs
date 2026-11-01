//! Read-only verification of a committed migration backup.
//!
//! Ref v2.0.1 records sources and links in its manifest, but does not carry
//! file hashes. Chimera adds an inventory to *new* backups; old manifests are
//! kept untouched and never claimed as byte-verified. This module never writes
//! to a user's config or storage directory.

use super::{BackupInfo, CONFIG_DIR, DATA_DIR, MANIFEST_FILE, Manifest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

pub(super) const FORMAT_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub(crate) enum VerificationError {
    #[error("cannot read backup node {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("backup node is not the required regular file or directory: {}", path.display())]
    InvalidNode { path: PathBuf },
    #[error("unsafe or ambiguous backup manifest path: {path}")]
    UnsafePath { path: String },
    #[error("invalid backup manifest")]
    Manifest(#[source] serde_json::Error),
    #[error("unsupported backup integrity manifest version {version}")]
    Format { version: u32 },
    #[error("backup manifest contents do not match its stored files or links: {reason}")]
    Mismatch { reason: &'static str },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Inventory {
    pub files: Vec<ManifestFile>,
    pub directories: Vec<String>,
}

fn io_error(path: &Path, source: io::Error) -> VerificationError {
    VerificationError::Io {
        path: path.to_owned(),
        source,
    }
}

fn directory(path: &Path) -> Result<(), VerificationError> {
    let meta = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(VerificationError::InvalidNode {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<(), VerificationError> {
    let meta = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(VerificationError::InvalidNode {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn safe_relative(path: &str) -> Result<(), VerificationError> {
    let parts: Vec<_> = path.split('/').collect();
    if parts.is_empty()
        || !matches!(parts[0], CONFIG_DIR | DATA_DIR)
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || part.contains('\\')
                || part.contains(':')
        })
    {
        return Err(VerificationError::UnsafePath {
            path: path.to_owned(),
        });
    }
    Ok(())
}

fn relative_name(root: &Path, path: &Path) -> Result<String, VerificationError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| VerificationError::InvalidNode {
            path: path.to_owned(),
        })?;
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err(VerificationError::InvalidNode {
                path: path.to_owned(),
            });
        };
        let Some(text) = name.to_str() else {
            return Err(VerificationError::InvalidNode {
                path: path.to_owned(),
            });
        };
        parts.push(text);
    }
    let name = parts.join("/");
    safe_relative(&name)?;
    Ok(name)
}

fn hash_file(path: &Path) -> Result<(u64, String), VerificationError> {
    regular_file(path)?;
    let mut file = fs::File::open(path).map_err(|error| io_error(path, error))?;
    let before = file.metadata().map_err(|error| io_error(path, error))?;
    if !before.is_file() {
        return Err(VerificationError::InvalidNode {
            path: path.to_owned(),
        });
    }
    let mut hash = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| io_error(path, error))?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        size = size
            .checked_add(read as u64)
            .ok_or(VerificationError::Mismatch {
                reason: "file length overflow",
            })?;
    }
    let after = fs::symlink_metadata(path).map_err(|error| io_error(path, error))?;
    if !after.is_file()
        || after.file_type().is_symlink()
        || before.len() != size
        || after.len() != size
    {
        return Err(VerificationError::Mismatch {
            reason: "file changed while hashing",
        });
    }
    Ok((size, hex::encode(hash.finalize())))
}

fn scan_dir(
    root: &Path,
    at: &Path,
    inventory: &mut Inventory,
    allowed_links: Option<&HashSet<String>>,
) -> Result<(), VerificationError> {
    directory(at)?;
    inventory.directories.push(relative_name(root, at)?);
    for entry in fs::read_dir(at).map_err(|error| io_error(at, error))? {
        let entry = entry.map_err(|error| io_error(at, error))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| io_error(&path, error))?;
        if kind.is_dir() {
            scan_dir(root, &path, inventory, allowed_links)?;
        } else if kind.is_file() {
            let rel = relative_name(root, &path)?;
            let (size, sha256) = hash_file(&path)?;
            inventory.files.push(ManifestFile {
                path: rel,
                size,
                sha256,
            });
        } else if kind.is_symlink()
            && allowed_links.is_some_and(|links| {
                relative_name(root, &path).is_ok_and(|relative| links.contains(&relative))
            })
        {
            // Test-only extraction may reconstruct links present in a verified
            // manifest. The committed backup must never contain these nodes.
        } else {
            return Err(VerificationError::InvalidNode { path });
        }
    }
    Ok(())
}

fn scan_snapshot_with_links(
    root: &Path,
    allowed_links: Option<&HashSet<String>>,
) -> Result<Inventory, VerificationError> {
    directory(root)?;
    let mut result = Inventory::default();
    scan_dir(root, &root.join(CONFIG_DIR), &mut result, allowed_links)?;
    let data = root.join(DATA_DIR);
    match fs::symlink_metadata(&data) {
        Ok(_) => scan_dir(root, &data, &mut result, allowed_links)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(io_error(&data, error)),
    }
    result.files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    result.directories.sort_unstable();
    Ok(result)
}

/// Committed backups never contain links, even ones named in the manifest.
pub(super) fn scan_snapshot(root: &Path) -> Result<Inventory, VerificationError> {
    scan_snapshot_with_links(root, None)
}

fn assert_manifest_safety(manifest: &Manifest) -> Result<(), VerificationError> {
    let mut claimed = HashSet::new();
    for path in manifest
        .directories
        .iter()
        .chain(manifest.files.iter().map(|entry| &entry.path))
        .chain(manifest.symlinks.iter().map(|entry| &entry.path))
    {
        safe_relative(path)?;
        if !claimed.insert(path.as_str()) {
            return Err(VerificationError::Mismatch {
                reason: "duplicate file, directory or link path",
            });
        }
    }

    let dirs: HashSet<&str> = manifest.directories.iter().map(String::as_str).collect();
    if !dirs.contains(CONFIG_DIR) {
        return Err(VerificationError::Mismatch {
            reason: "missing config root",
        });
    }
    for path in &claimed {
        let Some((parent, _)) = path.rsplit_once('/') else {
            // The only root nodes allowed in the inventory are directories.
            if !manifest.directories.iter().any(|entry| entry == path) {
                return Err(VerificationError::Mismatch {
                    reason: "file or link cannot replace an inventory root",
                });
            }
            continue;
        };
        if !dirs.contains(parent) {
            return Err(VerificationError::Mismatch {
                reason: "entry parent is not a recorded directory",
            });
        }
    }
    for link in &manifest.symlinks {
        if !link.path.starts_with("config/") || link.target.as_os_str().is_empty() {
            return Err(VerificationError::UnsafePath {
                path: link.path.clone(),
            });
        }
    }
    Ok(())
}

/// Verify a staged or committed snapshot, including the exact file set and
/// SHA-256 of each stored file. Ref-only manifests lack the new inventory and
/// correctly fail closed instead of being reported as verified.
pub(super) fn verify_at(root: &Path) -> Result<Manifest, VerificationError> {
    directory(root)?;
    for entry in fs::read_dir(root).map_err(|error| io_error(root, error))? {
        let entry = entry.map_err(|error| io_error(root, error))?;
        if !matches!(
            entry.file_name().to_str(),
            Some(MANIFEST_FILE | CONFIG_DIR | DATA_DIR)
        ) {
            return Err(VerificationError::Mismatch {
                reason: "unexpected snapshot-root entry",
            });
        }
    }
    let file = root.join(MANIFEST_FILE);
    regular_file(&file)?;
    let data = fs::read(&file).map_err(|error| io_error(&file, error))?;
    let manifest: Manifest = serde_json::from_slice(&data).map_err(VerificationError::Manifest)?;
    if manifest.format_version != FORMAT_VERSION {
        return Err(VerificationError::Format {
            version: manifest.format_version,
        });
    }
    if !matches!(manifest.kind.as_str(), "migration" | "manual") {
        return Err(VerificationError::Mismatch {
            reason: "invalid backup kind",
        });
    }
    assert_manifest_safety(&manifest)?;
    let actual = scan_snapshot(root)?;
    if actual.files != manifest.files || actual.directories != manifest.directories {
        return Err(VerificationError::Mismatch {
            reason: "file names, lengths, hashes or directories differ",
        });
    }
    for link in &manifest.symlinks {
        let path = root.join(&link.path);
        match fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(&path, error)),
            Ok(_) => {
                return Err(VerificationError::Mismatch {
                    reason: "recorded symlink unexpectedly exists as a copied node",
                });
            }
        }
    }
    Ok(manifest)
}

pub(super) fn verify_backup(backup: &BackupInfo) -> Result<(), VerificationError> {
    let parent = backup.path.parent().ok_or(VerificationError::InvalidNode {
        path: backup.path.clone(),
    })?;
    directory(parent)?;
    if backup.path.file_name() != Some(backup.name.as_ref()) {
        return Err(VerificationError::Mismatch {
            reason: "snapshot name and directory differ",
        });
    }
    let manifest = verify_at(&backup.path)?;
    let expected_prefix = match manifest.kind.as_str() {
        "migration" => super::MIGRATION_PREFIX,
        _ => super::MANUAL_PREFIX,
    };
    if !backup.name.starts_with(expected_prefix) {
        return Err(VerificationError::Mismatch {
            reason: "backup directory does not match the manifest kind",
        });
    }
    Ok(())
}

/// Test-only isolated extraction, intentionally not exposed to the application.
/// The caller must supply a NEW destination; no user config or storage is ever
/// overwritten. In production a full recovery needs offline ownership and UI
/// confirmation, which are not part of this module.
#[cfg(test)]
pub(super) fn restore_to_empty_sandbox(
    backup: &BackupInfo,
    destination: &Path,
) -> Result<(), VerificationError> {
    verify_backup(backup)?;
    // Fail without modifying an existing target. This makes the helper safe
    // for explicit tempfile-based round-trip tests.
    fs::create_dir(destination).map_err(|error| io_error(destination, error))?;
    let manifest = verify_at(&backup.path)?;
    for name in &manifest.directories {
        let path = destination.join(name);
        fs::create_dir(&path).map_err(|error| io_error(&path, error))?;
    }
    for item in &manifest.files {
        let src = backup.path.join(&item.path);
        let dst = destination.join(&item.path);
        fs::copy(&src, &dst).map_err(|error| io_error(&dst, error))?;
    }
    for link in &manifest.symlinks {
        let dst = destination.join(&link.path);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link.target, &dst).map_err(|error| io_error(&dst, error))?;
        #[cfg(not(unix))]
        return Err(VerificationError::InvalidNode { path: dst });
    }
    let links: HashSet<String> = manifest
        .symlinks
        .iter()
        .map(|link| link.path.clone())
        .collect();
    let extracted = scan_snapshot_with_links(destination, Some(&links))?;
    for link in &manifest.symlinks {
        let restored = destination.join(&link.path);
        let metadata =
            fs::symlink_metadata(&restored).map_err(|error| io_error(&restored, error))?;
        if !metadata.file_type().is_symlink()
            || fs::read_link(&restored).map_err(|error| io_error(&restored, error))? != link.target
        {
            return Err(VerificationError::Mismatch {
                reason: "restored symlink node or target differs from manifest",
            });
        }
    }
    if extracted.files != manifest.files || extracted.directories != manifest.directories {
        return Err(VerificationError::Mismatch {
            reason: "extracted bytes differ from committed snapshot",
        });
    }
    Ok(())
}
