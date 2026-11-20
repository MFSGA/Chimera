use crate::utils::path::PathResolver;
use chimera_contracts::storage::HotkeyStoreOpener;
use chimera_core::storage::RedbHotkeyStoreOpener;
use semver::Version;
use std::{path::PathBuf, sync::Arc};

pub(crate) mod fs;
pub mod modules;
pub mod registry;
pub mod runner;
pub mod store;

pub use runner::Runner;

pub use chimera_migration::{MigrationAdvice, MigrationId, MigrationState, ModuleId};

/// Tauri integration adapter: platform path discovery and injected shared context.
/// The migration itself uses the frontend-independent `chimera_migration::Ctx`.
#[derive(Debug, Clone)]
pub struct Ctx {
    paths: PathResolver,
    shared: chimera_migration::context::Ctx,
}

impl AsRef<chimera_migration::context::Ctx> for Ctx {
    fn as_ref(&self) -> &chimera_migration::context::Ctx {
        &self.shared
    }
}

impl std::ops::Deref for Ctx {
    type Target = chimera_migration::context::Ctx;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl Ctx {
    pub fn from_app_dirs() -> anyhow::Result<Self> {
        Ok(Self::from_paths(PathResolver::from_env()?))
    }

    pub fn from_paths(paths: PathResolver) -> Self {
        Self::with_hotkey_store_opener(paths, Arc::new(RedbHotkeyStoreOpener))
    }

    pub fn with_hotkey_store_opener(
        paths: PathResolver,
        hotkey_store_opener: Arc<dyn HotkeyStoreOpener>,
    ) -> Self {
        let migration_paths = chimera_migration::context::MigrationPaths::new(
            paths.app_config_dir().to_path_buf(),
            paths.app_data_dir().to_path_buf(),
        );
        Self {
            paths,
            shared: chimera_migration::context::Ctx::new(migration_paths, hotkey_store_opener),
        }
    }

    #[cfg(test)]
    pub fn new(app_config_dir: PathBuf, app_data_dir: PathBuf) -> Self {
        Self::from_paths(PathResolver::with_base_dirs(app_config_dir, app_data_dir))
    }

    /// Preserve the document-spec function-pointer signature during the move.
    pub fn profiles_path(&self) -> PathBuf {
        self.shared.profiles_path()
    }

    /// Keep the existing Tauri backup and legacy integrations on the same roots.
    pub fn paths(&self) -> &PathResolver {
        &self.paths
    }
}

pub use chimera_migration::module::{
    DocumentSpec, MigrationCheckError, MigrationStep, ModuleKind, ModuleMigrator, StepCheck,
};

pub fn current_version() -> anyhow::Result<Version> {
    Version::parse(crate::consts::BUILD_INFO.pkg_version).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_migration_paths_match_host_resolver_without_touching_disk() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config not yet created");
        let data = root.path().join("data not yet created");
        let host = PathResolver::with_base_dirs(config.clone(), data.clone());
        let ctx = Ctx::from_paths(host.clone());

        assert_eq!(ctx.profiles_path(), host.profiles_path());
        assert_eq!(ctx.chimera_config_path(), host.chimera_config_path());
        assert_eq!(
            ctx.application_config_path(),
            host.application_config_path()
        );
        assert_eq!(ctx.session_state_path(), host.session_state_path());
        assert_eq!(ctx.clash_config_path(), host.clash_config_path());
        assert_eq!(
            ctx.clash_guard_overrides_path(),
            host.clash_guard_overrides_path()
        );
        assert_eq!(ctx.storage_path(), host.storage_path());
        assert_eq!(ctx.state_path(), config.join("migration-state.yaml"));
        assert_eq!(ctx.paths().app_config_dir(), config);
        assert_eq!(ctx.paths().app_data_dir(), data);
        assert!(!config.exists());
        assert!(!data.exists());
    }
}
