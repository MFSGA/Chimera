//! Centralized filesystem path resolution.
//!
//! Ref keeps migration and client composition on one injected path resolver so
//! persisted-state ownership cannot accidentally drift across helpers.

use crate::utils::dirs;
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PathResolver {
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl PathResolver {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            config_dir: dirs::discover_config_dir()?,
            data_dir: dirs::discover_data_dir()?,
        })
    }

    pub fn with_base_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
        }
    }

    /// As in ref, only the two base roots are required. Leaf directories are
    /// best-effort and can be recreated by their individual consumers.
    pub fn create_base_dirs(&self) -> Result<()> {
        // Unlike the legacy directory helper, never delete a non-directory
        // occupying a required root. Startup must fail without losing data.
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        for path in [
            self.app_profiles_dir(),
            self.app_logs_dir(),
            self.cache_dir(),
        ] {
            // Do not remove a user's file if an optional leaf is obstructed.
            if let Err(error) = std::fs::create_dir_all(&path) {
                tracing::error!(%error, path = %path.display(), "failed to prepare optional startup directory");
            }
        }
        Ok(())
    }

    pub fn app_config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn app_data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn app_profiles_dir(&self) -> PathBuf {
        self.config_dir.join("profiles")
    }

    pub fn profiles_path(&self) -> PathBuf {
        self.config_dir.join(dirs::PROFILE_YAML)
    }

    pub fn chimera_config_path(&self) -> PathBuf {
        self.config_dir.join(dirs::CHIMERA_CONFIG)
    }

    pub fn application_config_path(&self) -> PathBuf {
        self.config_dir.join("application.yaml")
    }

    pub fn session_state_path(&self) -> PathBuf {
        self.config_dir.join("session-state.yaml")
    }

    pub fn clash_config_path(&self) -> PathBuf {
        self.config_dir.join("clash-config.yaml")
    }

    pub fn clash_guard_overrides_path(&self) -> PathBuf {
        self.config_dir.join(dirs::CLASH_CFG_GUARD_OVERRIDES)
    }

    pub fn storage_path(&self) -> PathBuf {
        self.data_dir.join(dirs::STORAGE_DB)
    }

    pub fn jobs_path(&self) -> PathBuf {
        self.data_dir.join("jobs.redb")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.data_dir.join("backups")
    }

    pub fn scripts_dir(&self) -> PathBuf {
        self.data_dir.join("scripts")
    }

    pub fn clash_pid_path(&self) -> PathBuf {
        self.data_dir.join("clash.pid")
    }

    pub fn app_logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.data_dir.join("cache")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> PathResolver {
        PathResolver::with_base_dirs(PathBuf::from("/cfg"), PathBuf::from("/data"))
    }

    #[test]
    fn base_directory_creation_is_explicit_and_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("configuration");
        let data = temp.path().join("application-data");
        let r = PathResolver::with_base_dirs(config.clone(), data.clone());

        assert!(
            !config.exists(),
            "resolving paths must not create configuration"
        );
        assert!(
            !data.exists(),
            "resolving paths must not create application data"
        );
        r.create_base_dirs().unwrap();
        r.create_base_dirs().unwrap();

        assert!(config.is_dir());
        assert!(data.is_dir());
        assert!(r.app_profiles_dir().is_dir());
        assert!(r.app_logs_dir().is_dir());
        assert!(r.cache_dir().is_dir());
    }

    #[test]
    fn required_root_failure_does_not_create_other_root() {
        let temp = tempfile::tempdir().unwrap();
        let blocker = temp.path().join("not-a-directory");
        std::fs::write(&blocker, b"preserve me").unwrap();
        let data = temp.path().join("new-data");
        let r = PathResolver::with_base_dirs(blocker.join("config"), data.clone());

        assert!(r.create_base_dirs().is_err());
        assert_eq!(std::fs::read(&blocker).unwrap(), b"preserve me");
        assert!(!data.exists());
    }

    #[test]
    fn existing_required_root_files_are_not_removed() {
        for blocked_root in ["config", "data"] {
            let temp = tempfile::tempdir().unwrap();
            let config = temp.path().join("config");
            let data = temp.path().join("data");
            let blocked = temp.path().join(blocked_root);
            std::fs::write(&blocked, b"preserve original user bytes").unwrap();
            let r = PathResolver::with_base_dirs(config, data);

            assert!(r.create_base_dirs().is_err());
            assert_eq!(
                std::fs::read(&blocked).unwrap(),
                b"preserve original user bytes"
            );
        }
    }

    #[test]
    fn blocked_optional_leaf_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("cache"), b"original cache file").unwrap();
        let r = PathResolver::with_base_dirs(config, data.clone());

        r.create_base_dirs().unwrap();
        assert_eq!(
            std::fs::read(data.join("cache")).unwrap(),
            b"original cache file"
        );
        assert!(r.app_profiles_dir().is_dir());
        assert!(r.app_logs_dir().is_dir());
    }

    #[test]
    fn config_derived_paths_join_config_dir() {
        let r = resolver();
        assert_eq!(
            r.profiles_path(),
            Path::new("/cfg").join(dirs::PROFILE_YAML)
        );
        assert_eq!(
            r.chimera_config_path(),
            Path::new("/cfg").join(dirs::CHIMERA_CONFIG)
        );
        assert_eq!(
            r.clash_guard_overrides_path(),
            Path::new("/cfg").join(dirs::CLASH_CFG_GUARD_OVERRIDES)
        );
        assert_eq!(
            r.application_config_path(),
            Path::new("/cfg").join("application.yaml")
        );
        assert_eq!(
            r.session_state_path(),
            Path::new("/cfg").join("session-state.yaml")
        );
        assert_eq!(
            r.clash_config_path(),
            Path::new("/cfg").join("clash-config.yaml")
        );
        assert_eq!(r.app_profiles_dir(), Path::new("/cfg").join("profiles"));
    }

    #[test]
    fn data_derived_paths_join_data_dir() {
        let r = resolver();
        assert_eq!(r.storage_path(), Path::new("/data").join(dirs::STORAGE_DB));
        assert_eq!(r.jobs_path(), Path::new("/data").join("jobs.redb"));
        assert_eq!(r.backups_dir(), Path::new("/data").join("backups"));
        assert_eq!(r.scripts_dir(), Path::new("/data").join("scripts"));
        assert_eq!(r.clash_pid_path(), Path::new("/data").join("clash.pid"));
        assert_eq!(r.app_logs_dir(), Path::new("/data").join("logs"));
        assert_eq!(r.cache_dir(), Path::new("/data").join("cache"));
    }
}
