//! Ref-aligned IO boundaries for migration-state persistence.
//! Never follow a destination link or replace a non-file node.

use anyhow::{Context as _, bail, ensure};
use std::{io, path::Path};

pub fn config_node_exists(path: &Path) -> io::Result<bool> {
    match chimera_persistence::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn validate_destination(path: &Path) -> anyhow::Result<()> {
    ensure!(
        path.file_name().is_some(),
        "destination path has no file name: {}",
        path.display()
    );
    match chimera_persistence::retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(metadata) if chimera_persistence::is_link(&metadata) => bail!(
            "cannot atomically overwrite symbolic link {}",
            path.display()
        ),
        Ok(metadata) if !metadata.is_file() => bail!(
            "cannot atomically overwrite non-file node {}",
            path.display()
        ),
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect destination {}", path.display()));
        }
    }
    Ok(())
}

pub fn atomic_write(path: &Path, content: &[u8]) -> anyhow::Result<()> {
    validate_destination(path)?;
    chimera_persistence::write::atomic_write(path, content)
        .with_context(|| format!("failed to persist migration file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn dangling_link_is_present_and_rejected_as_destination() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.yaml");
        let link = dir.path().join("migration-state.yaml");
        symlink(&missing, &link).unwrap();
        assert!(config_node_exists(&link).unwrap());
        assert!(!missing.exists());
        let error = atomic_write(&link, b"nope").unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read_link(link).unwrap(), missing);
    }

    #[cfg(unix)]
    #[test]
    fn linked_target_is_never_overwritten() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let external = dir.path().join("target.yaml");
        let destination = dir.path().join("migration-state.yaml");
        std::fs::write(&external, b"original").unwrap();
        symlink(&external, &destination).unwrap();
        let error = atomic_write(&destination, b"new").unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read(&external).unwrap(), b"original");
        assert_eq!(std::fs::read_link(destination).unwrap(), external);
    }

    #[test]
    fn directory_destination_is_rejected_without_removing_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-state.yaml");
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("old"), b"data").unwrap();
        assert!(
            format!("{:#}", atomic_write(&path, b"new").unwrap_err()).contains("non-file node")
        );
        assert_eq!(std::fs::read(path.join("old")).unwrap(), b"data");
    }
}
