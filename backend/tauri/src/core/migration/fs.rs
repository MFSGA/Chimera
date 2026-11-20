//! Tauri compatibility path for shared Migration document and YAML IO.
//! The single source of truth lives in chimera-migration::document.

#[cfg(test)]
use super::MigrationCheckError;
pub(crate) use chimera_migration::document::{
    DocumentFile, read_document, read_yaml_if_exists, try_exists, write_document,
};
#[cfg(test)]
use chimera_migration::document::{read_document_with, read_yaml_if_exists_with};
#[cfg(test)]
use serde_yaml_ng::Mapping;
use std::path::Path;

pub(crate) fn atomic_write(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
    chimera_migration::fs::atomic_write(path, contents)
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

    #[test]
    fn stamped_migration_write_uses_shared_document_format() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("profiles.yaml");
        let mut payload = Mapping::new();
        payload.insert("revision".into(), 7.into());
        write_document(&path, "profiles", 4, payload, Some("# migrated")).unwrap();
        let document = read_document(&path, "profiles").unwrap().unwrap();
        assert!(document.raw.starts_with("# migrated\n\n"));
        assert_eq!(document.schema_revision, Some(4));
        assert_eq!(document.payload["revision"].as_u64(), Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn stamped_migration_write_preserves_link_and_target() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("managed.yaml");
        let link = dir.path().join("profiles.yaml");
        std::fs::write(&target, b"external config").unwrap();
        symlink(&target, &link).unwrap();
        let mut payload = Mapping::new();
        payload.insert("revision".into(), 7.into());
        let error = write_document(&link, "profiles", 4, payload, None).unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read_link(link).unwrap(), target);
        assert_eq!(std::fs::read(target).unwrap(), b"external config");
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
