//! Crash-safe filesystem writes for the migration subsystem.
//!
//! Wraps [`atomicwrites`] (already a workspace dependency via `chimera-core`)
//! behind a single helper so the migration store and every config rewrite share
//! one durable write path. Keeping the third-party type in one place means a
//! future swap only touches this file.

use super::MigrationCheckError;
use anyhow::{Context, bail, ensure};
use atomicwrites::{AllowOverwrite, AtomicFile};
use chimera_core::format::{DocumentStamp, Inspected, StampError};
use serde::de::DeserializeOwned;
use serde_yaml_ng::Mapping;
use std::{io::Write, path::Path};

/// Whether `path` exists, reporting an inaccessible parent as an error
/// instead of the `false` that [`Path::exists`] would return.
pub(crate) fn try_exists(path: &Path) -> Result<bool, MigrationCheckError> {
    crate::core::corrupt::config_node_exists(path).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })
}

/// Read optional migration input without discarding a present dangling link.
/// NotFound from the read is only absence if the node itself is also absent.
fn read_string_if_exists_with(
    path: &Path,
    read: impl FnMut() -> std::io::Result<String>,
) -> Result<Option<String>, MigrationCheckError> {
    match crate::core::corrupt::retry_io(read) {
        Ok(raw) => Ok(Some(raw)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound && !try_exists(path)? => {
            Ok(None)
        }
        Err(source) => Err(MigrationCheckError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// The YAML document at `path`, or `None` when the file does not exist.
pub(crate) fn read_yaml_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, MigrationCheckError> {
    read_yaml_if_exists_with(path, || std::fs::read_to_string(path))
}

fn read_yaml_if_exists_with<T: DeserializeOwned>(
    path: &Path,
    read: impl FnMut() -> std::io::Result<String>,
) -> Result<Option<T>, MigrationCheckError> {
    let Some(raw) = read_string_if_exists_with(path, read)? else {
        return Ok(None);
    };
    serde_yaml_ng::from_str(&raw)
        .map(Some)
        .map_err(|source| MigrationCheckError::Parse {
            path: path.to_path_buf(),
            source,
        })
}

/// A document module's file, split into its stamp and content.
pub(crate) struct DocumentFile {
    pub raw: String,
    /// `None` for a file written before its module adopted stamps.
    pub schema_revision: Option<u64>,
    pub payload: Mapping,
}

/// The file at `path` holding `document`, or `None` when it does not exist.
/// A malformed stamp, or one naming another document, is an error.
pub(crate) fn read_document(
    path: &Path,
    document: &str,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    read_document_with(path, document, || std::fs::read_to_string(path))
}

fn read_document_with(
    path: &Path,
    document: &str,
    read: impl FnMut() -> std::io::Result<String>,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    let Some(raw) = read_string_if_exists_with(path, read)? else {
        return Ok(None);
    };
    let stamp_error = |source| MigrationCheckError::Stamp {
        path: path.to_path_buf(),
        source,
    };
    let (schema_revision, payload) =
        match chimera_core::format::inspect(&raw).map_err(stamp_error)? {
            Inspected::Unstamped(payload) => (None, payload),
            Inspected::Stamped { stamp, payload } => {
                if stamp.document != document {
                    return Err(stamp_error(StampError::WrongDocument {
                        expected: document.to_owned(),
                        found: stamp.document,
                    }));
                }
                (Some(stamp.schema_revision), payload)
            }
        };
    Ok(Some(DocumentFile {
        raw,
        schema_revision,
        payload,
    }))
}

/// Atomically write `payload` to `path`, stamped as `document` at
/// `schema_revision`, so the content and its revision always change together.
pub(crate) fn write_document(
    path: &Path,
    document: &str,
    schema_revision: u64,
    payload: Mapping,
    prefix: Option<&str>,
) -> anyhow::Result<()> {
    let stamp = DocumentStamp {
        document: document.to_owned(),
        schema_revision,
    };
    let stamped = chimera_core::format::stamp(payload, &stamp)
        .with_context(|| format!("failed to stamp {}", path.display()))?;
    let body = serde_yaml_ng::to_string(&stamped)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    let content = match prefix {
        Some(prefix) => format!("{prefix}\n\n{body}"),
        None => body,
    };
    atomic_write(path, content.as_bytes())
}

/// Atomically write `contents` to `path`.
///
/// The destination is never left half-written: `atomicwrites` writes the bytes
/// into a temp file under a randomized `.atomicwrite` subdirectory of the
/// target's parent, fsyncs it, then atomically replaces `path`. On Unix it also
/// fsyncs the parent directories so the rename survives a crash; on Windows it
/// replaces via `MoveFileExW` with write-through semantics. Missing parent
/// directories are created first.
pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    ensure!(
        path.file_name().is_some(),
        "destination path has no file name: {}",
        path.display()
    );
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        crate::core::corrupt::retry_io(|| std::fs::create_dir_all(parent))
            .with_context(|| format!("failed to create dir {}", parent.display()))?;
    }
    // An atomic rename would replace a symbolic link itself rather than
    // updating its target, silently breaking externally managed config paths.
    // A directory or other non-file node must never be replaced either.
    match crate::core::corrupt::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(metadata) if crate::core::corrupt::is_link(&metadata) => {
            bail!(
                "cannot atomically overwrite symbolic link {}",
                path.display()
            );
        }
        Ok(metadata) if !metadata.is_file() => {
            bail!(
                "cannot atomically overwrite non-file node {}",
                path.display()
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect destination {}", path.display()));
        }
    }
    crate::core::corrupt::retry(
        || AtomicFile::new(path, AllowOverwrite).write(|file| file.write_all(contents)),
        |error| {
            let (atomicwrites::Error::Internal(error) | atomicwrites::Error::User(error)) = error;
            crate::utils::recovery_io::classify(error)
                == crate::utils::recovery_io::IoFailure::Transient
        },
    )
    .with_context(|| format!("failed to atomically write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[cfg(unix)]
    #[test]
    fn atomic_write_preserves_existing_symlink_and_its_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing-data.yaml");
        let destination = dir.path().join("profiles.yaml");
        std::fs::write(&target, b"original profiles").unwrap();
        symlink(&target, &destination).unwrap();

        let error = atomic_write(&destination, b"replacement")
            .expect_err("migration must not silently replace a linked document");
        assert!(format!("{error:#}").contains("symbolic link"), "{error:#}");
        assert_eq!(std::fs::read_link(&destination).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), b"original profiles");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn atomic_write_rejects_directory_destination_without_touching_children() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("migration-state.yaml");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("original"), b"keep").unwrap();

        let error = atomic_write(&destination, b"replacement").unwrap_err();
        assert!(format!("{error:#}").contains("non-file node"), "{error:#}");
        assert!(destination.is_dir());
        assert_eq!(
            std::fs::read(destination.join("original")).unwrap(),
            b"keep"
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn broken_document_link_must_not_be_mistaken_for_missing_file() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");
        let missing_target = dir.path().join("not-created.yaml");
        symlink(&missing_target, &path).unwrap();

        assert!(try_exists(&path).unwrap(), "the link node exists");
        let yaml_error = read_yaml_if_exists::<Mapping>(&path)
            .err()
            .expect("YAML reader must not treat a dangling link as missing");
        let document_error = read_document(&path, "profiles")
            .err()
            .expect("document reader must not treat a dangling link as missing");
        for error in [yaml_error, document_error] {
            assert!(
                matches!(error, MigrationCheckError::Io { source, .. }
                    if source.kind() == io::ErrorKind::NotFound),
                "expected original NotFound IO source on present broken link"
            );
        }
        assert_eq!(std::fs::read_link(&path).unwrap(), missing_target);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn yaml_reader_retries_transient_read_before_parsing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        std::fs::write(&path, "value: 7").unwrap();
        let mut attempts = 0;
        let value: Mapping = read_yaml_if_exists_with(&path, || {
            attempts += 1;
            if attempts == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                std::fs::read_to_string(&path)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(value["value"].as_u64(), Some(7));
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn yaml_reader_reports_exhausted_and_permission_errors_without_renaming() {
        for (kind, expected_attempts) in [
            (io::ErrorKind::WouldBlock, 4),
            (io::ErrorKind::PermissionDenied, 1),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.yaml");
            std::fs::write(&path, "value: 7").unwrap();
            let mut attempts = 0;
            let result = read_yaml_if_exists_with::<Mapping>(&path, || {
                attempts += 1;
                Err(io::Error::from(kind))
            });
            assert!(
                matches!(result, Err(MigrationCheckError::Io { path: actual, source }) if actual == path && source.kind() == kind)
            );
            assert_eq!(attempts, expected_attempts);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "value: 7");
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn document_reader_retries_transient_read_before_inspection() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.yaml");
        let raw = "value: 7";
        std::fs::write(&path, raw).unwrap();
        let mut attempts = 0;
        let document = read_document_with(&path, "test.document", || {
            attempts += 1;
            if attempts == 1 {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                std::fs::read_to_string(&path)
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(document.raw, raw);
        assert_eq!(document.schema_revision, None);
        assert_eq!(document.payload["value"].as_u64(), Some(7));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn document_reader_reports_exhausted_and_permission_errors_without_renaming() {
        for (kind, expected_attempts) in [
            (io::ErrorKind::WouldBlock, 4),
            (io::ErrorKind::PermissionDenied, 1),
        ] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join("config.yaml");
            let raw = "value: 7";
            std::fs::write(&path, raw).unwrap();
            let mut attempts = 0;
            let result = read_document_with(&path, "test.document", || {
                attempts += 1;
                Err(io::Error::new(kind, std::fmt::Error))
            });
            let Err(MigrationCheckError::Io {
                path: actual,
                source,
            }) = result
            else {
                panic!("expected a typed document read error");
            };
            assert_eq!(actual, path);
            assert_eq!(source.kind(), kind);
            assert!(source.get_ref().unwrap().is::<std::fmt::Error>());
            assert_eq!(attempts, expected_attempts);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), raw);
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }
}
