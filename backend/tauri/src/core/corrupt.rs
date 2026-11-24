//! Startup storage error classification and compatibility entry points.
//! The shared persistence crate owns retries, evidence preservation and IO safety.

use std::{io, path::Path};

pub(crate) use chimera_core::corrupt::error_io;
pub(crate) use chimera_persistence::{Error, is_link, preserve, retry, retry_io, retry_with_delay};

/// Check whether a configuration node exists without following symlinks.
/// Unlike `Path::exists`, a dangling symlink is still present and must not
/// silently fall back to a seed/default; inaccessible paths retain their IO error.
/// Existing valid symlinks remain loadable by callers for compatibility.
pub(crate) fn config_node_exists(path: &Path) -> io::Result<bool> {
    match retry_io(|| std::fs::symlink_metadata(path)) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_node_existence_distinguishes_missing_and_invalid_parent() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.yaml");
        assert!(!config_node_exists(&missing).unwrap());
        let regular = temp.path().join("regular.yaml");
        std::fs::write(&regular, b"keep").unwrap();
        assert!(config_node_exists(&regular).unwrap());
        // Windows maps a child of a non-directory to platform-specific errors;
        // the exact NotADirectory mapping is only asserted on Unix.
        #[cfg(unix)]
        {
            let impossible = regular.join("child.yaml");
            assert_eq!(
                config_node_exists(&impossible).unwrap_err().kind(),
                io::ErrorKind::NotADirectory,
            );
        }
        assert_eq!(std::fs::read(&regular).unwrap(), b"keep");
    }

    #[cfg(unix)]
    #[test]
    fn dangling_link_is_a_present_config_node() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let missing_target = temp.path().join("not-created.yaml");
        let link = temp.path().join("application.yaml");
        symlink(&missing_target, &link).unwrap();
        assert!(!link.exists(), "Path::exists follows a dangling link");
        assert!(config_node_exists(&link).unwrap());
        assert_eq!(std::fs::read_link(&link).unwrap(), missing_target);
        assert!(!missing_target.exists());
    }
}
