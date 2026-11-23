//! Document IO for ref-style stamped migration modules.
//! Reads never discard malformed stamps or assume a dangling symlink is absent;
//! writes reject linked or non-file destinations before atomic replacement.

use crate::{fs, module::MigrationCheckError};
use chimera_persistence::{
    format::{Inspected, StampError},
    write as persistence_write,
};
use serde::de::DeserializeOwned;
use serde_yaml_ng::Mapping;
use std::{io, path::Path};

/// A module document split into its text, revision stamp and payload.
pub struct DocumentFile {
    pub raw: String,
    pub schema_revision: Option<u64>,
    pub payload: Mapping,
}

/// Whether the on-disk node exists (including a dangling link).
pub fn try_exists(path: &Path) -> Result<bool, MigrationCheckError> {
    fs::config_node_exists(path).map_err(|source| MigrationCheckError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_string_if_exists_with(
    path: &Path,
    read: impl FnMut() -> io::Result<String>,
) -> Result<Option<String>, MigrationCheckError> {
    match chimera_persistence::retry_io(read) {
        Ok(raw) => Ok(Some(raw)),
        Err(source) if source.kind() == io::ErrorKind::NotFound && !try_exists(path)? => Ok(None),
        Err(source) => Err(MigrationCheckError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Read an optional YAML input, preserving the original parse/IO sources.
pub fn read_yaml_if_exists<T: DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, MigrationCheckError> {
    read_yaml_if_exists_with(path, || std::fs::read_to_string(path))
}

/// Injected reader used for bounded retry and transient error verification.
pub fn read_yaml_if_exists_with<T: DeserializeOwned>(
    path: &Path,
    read: impl FnMut() -> io::Result<String>,
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

/// Read a stamped or legacy unstamped migration document.
pub fn read_document(
    path: &Path,
    document: &str,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    read_document_with(path, document, || std::fs::read_to_string(path))
}

/// Injected reader: the entire file is read before inspecting its stamp.
pub fn read_document_with(
    path: &Path,
    document: &str,
    read: impl FnMut() -> io::Result<String>,
) -> Result<Option<DocumentFile>, MigrationCheckError> {
    let Some(raw) = read_string_if_exists_with(path, read)? else {
        return Ok(None);
    };
    let stamp_error = |source| MigrationCheckError::Stamp {
        path: path.to_path_buf(),
        source,
    };
    let (schema_revision, payload) =
        match chimera_persistence::format::inspect(&raw).map_err(stamp_error)? {
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

/// Atomically persist a stamped migration document, including a custom prefix.
pub fn write_document(
    path: &Path,
    document: &str,
    schema_revision: u64,
    payload: Mapping,
    prefix: Option<&str>,
) -> anyhow::Result<()> {
    fs::validate_destination(path)?;
    persistence_write::write_document(path, document, schema_revision, payload, prefix)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamped_write_and_read_preserves_format_and_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("profiles.yaml");
        let mut payload = Mapping::new();
        payload.insert("mode".into(), "rule".into());
        write_document(&path, "profiles", 4, payload, Some("# migrated")).unwrap();
        let read = read_document(&path, "profiles").unwrap().unwrap();
        assert_eq!(read.schema_revision, Some(4));
        assert!(read.raw.starts_with("# migrated\n\n"));
        assert_eq!(read.payload["mode"].as_str(), Some("rule"));
    }

    #[test]
    fn mismatched_stamp_preserves_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");
        write_document(&path, "application", 2, Mapping::new(), None).unwrap();
        let original = std::fs::read(&path).unwrap();
        let error = read_document(&path, "profiles").err().unwrap();
        assert!(matches!(
            error,
            MigrationCheckError::Stamp {
                source: StampError::WrongDocument { .. },
                ..
            }
        ));
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_link_is_not_treated_as_missing_document() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");
        let target = dir.path().join("missing.yaml");
        symlink(&target, &path).unwrap();
        let error = read_document(&path, "profiles").err().unwrap();
        assert!(
            matches!(error, MigrationCheckError::Io { source, .. } if source.kind() == io::ErrorKind::NotFound)
        );
        assert_eq!(std::fs::read_link(path).unwrap(), target);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_document_output_symlink_without_touching_external_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external.yaml");
        let path = dir.path().join("profiles.yaml");
        std::fs::write(&target, b"external").unwrap();
        symlink(&target, &path).unwrap();
        let error = write_document(&path, "profiles", 4, Mapping::new(), None).unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read(target).unwrap(), b"external");
    }
}
