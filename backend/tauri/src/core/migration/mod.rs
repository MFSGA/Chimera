use crate::utils::path::PathResolver;
use chimera_contracts::{BoxedCause, storage::HotkeyStoreOpener};
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

/// What a step's check found on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepCheck {
    Needed,
    Satisfied,
}

impl StepCheck {
    pub fn from_needed(needed: bool) -> Self {
        if needed {
            Self::Needed
        } else {
            Self::Satisfied
        }
    }
}

/// Why a check could not tell whether its step is needed. These are problems
/// with the environment or the data; a defect in the check itself is not
/// reported here (see [`MigrationStep::check`]).
#[derive(Debug, thiserror::Error)]
pub enum MigrationCheckError {
    #[error("failed to access {}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse {}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },
    #[error("failed to read the key-value storage at {}", path.display())]
    Storage {
        path: PathBuf,
        #[source]
        source: BoxedCause,
    },
    #[error("failed to read the stamp of {}", path.display())]
    Stamp {
        path: PathBuf,
        #[source]
        source: chimera_core::format::StampError,
    },
    /// Readable and well-formed, but in no shape the step knows.
    #[error("{0}")]
    Unrecognized(String),
}

pub trait MigrationStep: Send + Sync {
    fn id(&self) -> MigrationId;
    fn module(&self) -> ModuleId;
    fn revision(&self) -> u64;
    fn introduced_in(&self) -> &'static Version;
    fn name(&self) -> &'static str;
    /// Whether the files on disk still need this step; `None` means the step
    /// has no check and always runs. The runner records `Skipped` on `Satisfied`
    /// and asks again after `run`, so `run` may assume `Needed`, and a check
    /// that still says `Needed` afterwards is reported as a defect.
    fn check(&self, _ctx: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
        Ok(None)
    }
    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()>;
    fn rollback(&self, _: &mut Ctx) -> anyhow::Result<()> {
        Ok(())
    }
}

/// How the runner learns which revision a module's files are at.
///
/// New config migrations must go into a `Document` module; `Heuristic`
/// modules are frozen and must not gain steps, since every step would need
/// yet another shape probe in `detect_baseline` and `files_behind`.
#[derive(Debug, Clone, Copy)]
pub enum ModuleKind {
    /// The revision is guessed from the shape of the files.
    Heuristic,
    /// Every step rewrites one file only, and the module revision is the
    /// schema revision stamped into that file.
    Document(DocumentSpec),
}

#[derive(Debug, Clone, Copy)]
pub struct DocumentSpec {
    pub document: &'static str,
    pub path: fn(&Ctx) -> PathBuf,
    /// The last revision written before the module adopted stamps. An
    /// unstamped file is at most this revision, and `detect_baseline` only
    /// has to tell revisions up to it apart.
    pub unstamped_ceiling: u64,
}

pub trait ModuleMigrator: Send + Sync {
    fn module(&self) -> ModuleId;
    fn kind(&self) -> ModuleKind {
        ModuleKind::Heuristic
    }
    /// The revision of files that no state records. For a `Document` module
    /// it is only asked about an existing, unstamped file.
    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64>;
    fn steps(&self) -> &'static [&'static dyn MigrationStep];
    /// What the files on disk lack when they are behind `applied`, i.e. the
    /// state file claims migrations whose results are missing or were moved
    /// away; `None` when they match. Errors are reserved for files that cannot
    /// be read or parsed. Only modules whose on-disk shape reliably reveals
    /// their revision override this; the rest cannot tell and accept any
    /// `applied`.
    fn files_behind(&self, _ctx: &Ctx, _applied: u64) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

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
