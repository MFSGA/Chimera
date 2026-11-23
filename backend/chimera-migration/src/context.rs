//! Reference-aligned migration context with resolved roots and injected storage.
//! Constructing a context has no filesystem side effects; the desktop shell
//! remains responsible for platform-specific directory discovery.

use chimera_contracts::storage::HotkeyStoreOpener;
use chimera_paths::{CHIMERA_CONFIG, CLASH_CFG_GUARD_OVERRIDES, PROFILE_YAML, STORAGE_DB};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Immutable roots used by migration steps, without a frontend dependency.
#[derive(Debug, Clone)]
pub struct MigrationPaths {
    config: PathBuf,
    data: PathBuf,
}

impl MigrationPaths {
    pub fn new(config: PathBuf, data: PathBuf) -> Self {
        Self { config, data }
    }

    pub fn app_config_dir(&self) -> &Path {
        &self.config
    }
    pub fn app_data_dir(&self) -> &Path {
        &self.data
    }
    pub fn profiles_path(&self) -> PathBuf {
        self.config.join(PROFILE_YAML)
    }
    pub fn chimera_config_path(&self) -> PathBuf {
        self.config.join(CHIMERA_CONFIG)
    }
    pub fn application_config_path(&self) -> PathBuf {
        self.config.join("application.yaml")
    }
    pub fn session_state_path(&self) -> PathBuf {
        self.config.join("session-state.yaml")
    }
    pub fn clash_config_path(&self) -> PathBuf {
        self.config.join("clash-config.yaml")
    }
    pub fn clash_guard_overrides_path(&self) -> PathBuf {
        self.config.join(CLASH_CFG_GUARD_OVERRIDES)
    }
    pub fn storage_path(&self) -> PathBuf {
        self.data.join(STORAGE_DB)
    }
    pub fn state_path(&self) -> PathBuf {
        self.config.join(crate::store::STORE_FILE_NAME)
    }
}

#[derive(Debug, Clone)]
pub struct Ctx {
    paths: MigrationPaths,
    hotkey_store_opener: Arc<dyn HotkeyStoreOpener>,
}

impl AsRef<Ctx> for Ctx {
    fn as_ref(&self) -> &Ctx {
        self
    }
}

impl Ctx {
    pub fn new(paths: MigrationPaths, hotkey_store_opener: Arc<dyn HotkeyStoreOpener>) -> Self {
        Self {
            paths,
            hotkey_store_opener,
        }
    }

    pub fn paths(&self) -> &MigrationPaths {
        &self.paths
    }
    pub fn hotkey_store_opener(&self) -> &dyn HotkeyStoreOpener {
        self.hotkey_store_opener.as_ref()
    }
    pub fn profiles_path(&self) -> PathBuf {
        self.paths.profiles_path()
    }
    pub fn chimera_config_path(&self) -> PathBuf {
        self.paths.chimera_config_path()
    }
    pub fn application_config_path(&self) -> PathBuf {
        self.paths.application_config_path()
    }
    pub fn session_state_path(&self) -> PathBuf {
        self.paths.session_state_path()
    }
    pub fn clash_config_path(&self) -> PathBuf {
        self.paths.clash_config_path()
    }
    pub fn clash_guard_overrides_path(&self) -> PathBuf {
        self.paths.clash_guard_overrides_path()
    }
    pub fn storage_path(&self) -> PathBuf {
        self.paths.storage_path()
    }
    pub fn state_path(&self) -> PathBuf {
        self.paths.state_path()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_paths_without_touching_disk() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("nonexistent-config");
        let data = temp.path().join("nonexistent-data");
        let paths = MigrationPaths::new(config.clone(), data.clone());
        assert_eq!(paths.profiles_path(), config.join("profiles.yaml"));
        assert_eq!(
            paths.application_config_path(),
            config.join("application.yaml")
        );
        assert_eq!(paths.storage_path(), data.join("storage.db"));
        assert_eq!(paths.state_path(), config.join("migration-state.yaml"));
        assert!(!config.exists());
        assert!(!data.exists());
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_path_components_without_conversion() {
        use std::os::unix::ffi::OsStringExt;
        let root = PathBuf::from(std::ffi::OsString::from_vec(vec![
            b'/', b't', b'm', b'p', b'/', 255,
        ]));
        let paths = MigrationPaths::new(root.clone(), root.clone());
        assert_eq!(paths.storage_path(), root.join("storage.db"));
        assert_eq!(paths.state_path(), root.join("migration-state.yaml"));
    }
}
