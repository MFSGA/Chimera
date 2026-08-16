use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::{
    config::profile::item_type::ProfileItemType,
    utils::{dirs, help},
};

pub fn generate_uid(kind: &ProfileItemType) -> String {
    match kind {
        ProfileItemType::Remote => help::get_uid("r"),
        ProfileItemType::Local => help::get_uid("l"),
        ProfileItemType::Merge => help::get_uid("m"),
        ProfileItemType::Script(_) => help::get_uid("s"),
    }
}

/// Resolve a managed profile filename inside the profiles directory.
///
/// Managed profiles are intentionally flat: accepting separators, parent
/// components, absolute paths, or links would allow a persisted profiles.yaml
/// entry to read, overwrite, open, or delete files outside application storage.
pub fn resolve_managed_profile_path(file: &str) -> Result<PathBuf> {
    resolve_managed_profile_path_in(&dirs::app_profiles_dir()?, file)
}

fn resolve_managed_profile_path_in(root: &Path, file: &str) -> Result<PathBuf> {
    let name = validate_managed_profile_name(file)?;
    validate_directory_if_present(root)?;
    let path = root.join(name);
    validate_file_if_present(&path)?;
    Ok(path)
}

fn validate_managed_profile_name(file: &str) -> Result<&std::ffi::OsStr> {
    if file.contains(['/', '\\']) {
        bail!("managed profile path must not contain directories");
    }

    let relative = Path::new(file);
    let mut components = relative.components();
    let Some(Component::Normal(name)) = components.next() else {
        bail!("managed profile path must be a file name");
    };
    if components.next().is_some() || name.is_empty() {
        bail!("managed profile path must not contain directories");
    }
    Ok(name)
}

fn validate_directory_if_present(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_symlink_or_reparse(&metadata) || !metadata.is_dir() {
                bail!("managed profiles root is not a real directory");
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

fn validate_file_if_present(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            if is_symlink_or_reparse(&metadata) || !metadata.is_file() {
                bail!("managed profile target is not a real file");
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect {}", path.display())),
    }
}

#[cfg(windows)]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    metadata.file_type().is_symlink()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_symlink_or_reparse(metadata: &std::fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_profile_path_rejects_escape_components() {
        let dir = tempfile::tempdir().unwrap();
        for path in [
            "",
            ".",
            "..",
            "../outside.yaml",
            "dir/file.yaml",
            "dir\\file.yaml",
            "/tmp/file.yaml",
            "C:\\outside.yaml",
            "\\\\server\\share\\outside.yaml",
        ] {
            assert!(
                resolve_managed_profile_path_in(dir.path(), path).is_err(),
                "accepted {path:?}"
            );
        }
    }

    #[test]
    fn managed_profile_path_accepts_a_flat_yaml_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = resolve_managed_profile_path_in(dir.path(), "profile.yaml").unwrap();
        assert_eq!(path.file_name().unwrap(), "profile.yaml");
    }

    #[test]
    fn managed_profile_root_rejects_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles");
        std::fs::write(&path, b"not a directory").unwrap();

        assert!(validate_directory_if_present(&path).is_err());
    }

    #[test]
    fn managed_profile_target_rejects_a_directory() {
        let dir = tempfile::tempdir().unwrap();

        assert!(validate_file_if_present(dir.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn managed_profile_root_rejects_a_junction() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let junction = dir.path().join("profiles");
        std::fs::create_dir(&target).unwrap();
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .status()
            .unwrap();
        assert!(status.success());
        assert!(resolve_managed_profile_path_in(&junction, "profile.yaml").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn managed_profile_target_rejects_a_symlink() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.yaml");
        let link = dir.path().join("profile.yaml");
        std::fs::write(&target, b"mode: rule\n").unwrap();
        symlink(&target, &link).unwrap();

        assert!(validate_file_if_present(&link).is_err());
    }
}
