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
            config_dir: dirs::app_config_dir()?,
            data_dir: dirs::app_data_dir()?,
        })
    }

    pub fn with_base_dirs(config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            config_dir,
            data_dir,
        }
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
        assert_eq!(r.clash_pid_path(), Path::new("/data").join("clash.pid"));
        assert_eq!(r.app_logs_dir(), Path::new("/data").join("logs"));
        assert_eq!(r.cache_dir(), Path::new("/data").join("cache"));
    }
}
