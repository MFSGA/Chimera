use super::{
    Ctx, DocumentSpec, MigrationAdvice, MigrationCheckError, MigrationStep, ModuleKind,
    ModuleMigrator, StepCheck, current_version, fs, registry,
    store::{MigrationStore, ModuleState},
};
use crate::{
    core::backup::{
        self, BackupInfo, BackupKind, BackupRequest, KEEP_MIGRATION_BACKUPS, MIGRATION_PREFIX,
        StorageSource,
    },
    utils::path::PathResolver,
};
use anyhow::{Context, bail};
use chimera_migration::{StepSpec, runner as core_runner};
use semver::Version;
use time::OffsetDateTime;

fn step_spec(step: &dyn MigrationStep<Ctx>) -> StepSpec<'static> {
    StepSpec {
        id: step.id(),
        module: step.module(),
        revision: step.revision(),
        introduced_in: step.introduced_in(),
    }
}

/// The shell-only verified backup producer, injected into the shared Runner.
struct TauriBackupPort<'a> {
    paths: &'a PathResolver,
    from: Option<&'a Version>,
    target: &'a Version,
}

impl core_runner::BackupPort for TauriBackupPort<'_> {
    type Snapshot = BackupInfo;

    fn create_snapshot(
        &mut self,
        ctx: &chimera_migration::context::Ctx,
    ) -> anyhow::Result<BackupInfo> {
        let storage_path = ctx.storage_path();
        let snapshot = backup::create_backup(&BackupRequest {
            paths: self.paths,
            storage: StorageSource::File(&storage_path),
            kind: BackupKind::Migration {
                from: self.from,
                target: self.target,
            },
            now: OffsetDateTime::now_utc(),
        })
        .context("failed to back up the config before migrating")?;
        tracing::info!(path = %snapshot.path.display(), "migration backup created before config rewrite");
        Ok(snapshot)
    }
}

pub(crate) use chimera_migration::runner::RevisionError;

#[derive(Debug)]
pub struct Runner {
    target: Version,
    force: bool,
    profile_migrations_enabled: bool,
    ctx: Ctx,
    store: MigrationStore,
    backup: Option<BackupInfo>,
}

impl Default for Runner {
    fn default() -> Self {
        Self::new(false).expect("failed to create migration runner")
    }
}

impl Runner {
    pub fn new(force: bool) -> anyhow::Result<Self> {
        Self::with_target(current_version()?, force)
    }

    pub fn with_target(target: Version, force: bool) -> anyhow::Result<Self> {
        Self::with_context(target, force, Ctx::from_app_dirs()?)
    }

    pub fn with_paths(paths: PathResolver, force: bool) -> anyhow::Result<Self> {
        Self::with_context(current_version()?, force, Ctx::from_paths(paths))
    }

    /// Runs the migration set that remains compatible with the currently
    /// wired legacy Profile client. This is retained only for migration tests;
    /// production startup uses the ref-aligned Profile document migration.
    #[cfg(test)]
    pub fn with_paths_before_profile_client_migration(
        paths: PathResolver,
        force: bool,
    ) -> anyhow::Result<Self> {
        Self::with_context_and_profile_migrations(
            current_version()?,
            force,
            Ctx::from_paths(paths),
            false,
        )
    }

    pub fn advice_step(&self, step: &dyn MigrationStep<Ctx>) -> MigrationAdvice {
        core_runner::advice_step(
            &self.store,
            step_spec(step),
            &self.target,
            self.force,
            self.profile_migrations_enabled || step.module() != "profiles",
        )
    }

    pub fn run_migration(&mut self, step: &dyn MigrationStep<Ctx>) -> anyhow::Result<()> {
        let modules = self.modules();
        // Snapshot metadata is borrowed independently of the mutable Context
        // and Store passed to the shared Runner for the entire transaction.
        let paths = self.ctx.paths().clone();
        let from = self.store.app.last_succeeded.clone();
        let mut backup_port = TauriBackupPort {
            paths: &paths,
            from: from.as_ref(),
            target: &self.target,
        };
        core_runner::run_selected_migration(
            step,
            &modules,
            &registry::MODULES,
            &mut self.ctx,
            &mut self.store,
            &mut self.backup,
            &mut backup_port,
            &self.target,
            self.force,
            self.profile_migrations_enabled || step.module() != "profiles",
        )
    }

    pub fn run_pending(&mut self) -> anyhow::Result<()> {
        println!("Running migrations up to version: {}", self.target);
        let modules = self.modules();
        // Snapshot metadata is borrowed independently of the mutable Context
        // and Store passed to the shared Runner for the entire transaction.
        let paths = self.ctx.paths().clone();
        let from = self.store.app.last_succeeded.clone();
        let mut backup_port = TauriBackupPort {
            paths: &paths,
            from: from.as_ref(),
            target: &self.target,
        };
        core_runner::run_pending(
            &modules,
            &registry::MODULES,
            &mut self.ctx,
            &mut self.store,
            &mut self.backup,
            &mut backup_port,
            &self.target,
            self.force,
        )?;
        // Only the host can prune platform backups, and only after the shared
        // Runner has durably committed the successful migration version.
        if let Err(error) = backup::prune_backups(
            &self.ctx.paths().backups_dir(),
            MIGRATION_PREFIX,
            KEEP_MIGRATION_BACKUPS,
        ) {
            eprintln!("Failed to prune old migration backups: {error:#}");
        }
        Ok(())
    }

    /// A committed snapshot is available for failure diagnostics only after
    /// create_backup has successfully renamed its completed directory.
    pub(crate) fn backup_info(&self) -> Option<&BackupInfo> {
        self.backup.as_ref()
    }

    fn with_context(target: Version, force: bool, ctx: Ctx) -> anyhow::Result<Self> {
        Self::with_context_and_profile_migrations(target, force, ctx, true)
    }

    fn with_context_and_profile_migrations(
        target: Version,
        force: bool,
        ctx: Ctx,
        profile_migrations_enabled: bool,
    ) -> anyhow::Result<Self> {
        let state_path = ctx.state_path();
        let store = core_runner::load_state(&state_path)?;
        let mut runner = Self {
            target,
            force,
            profile_migrations_enabled,
            ctx,
            store,
            backup: None,
        };
        runner.ensure_baselines()?;
        Ok(runner)
    }

    fn modules(&self) -> Vec<&'static dyn ModuleMigrator<Ctx>> {
        registry::modules()
            .filter(|module| self.profile_migrations_enabled || module.module() != "profiles")
            .collect()
    }

    fn ensure_baselines(&mut self) -> anyhow::Result<()> {
        core_runner::ensure_baselines(
            self.modules(),
            &self.ctx,
            &mut self.store,
            &self.ctx.state_path(),
        )
    }

    fn run_step(&mut self, step: &dyn MigrationStep<Ctx>) -> anyhow::Result<()> {
        let state_path = self.ctx.state_path();
        core_runner::execute_step(
            step,
            &mut self.ctx,
            &mut self.store,
            &state_path,
            &registry::MODULES,
        )
    }
}

#[cfg(test)]
use super::MigrationState;
#[cfg(test)]
use chimera_migration::runner::introduced_in_reached;

#[cfg(test)]
mod tests {
    use super::*;

    use crate::core::migration::{
        MigrationCheckError,
        store::{ModuleState, STORE_FILE_NAME},
    };
    use anyhow::bail;
    use once_cell::sync::Lazy;

    fn runner_with_store(store: MigrationStore, force: bool) -> Runner {
        let temp = tempfile::tempdir().unwrap().keep();
        let config_dir = temp.join("config");
        let data_dir = temp.join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        Runner {
            target: TEST_VERSION.clone(),
            force,
            profile_migrations_enabled: true,
            ctx: Ctx::new(config_dir, data_dir),
            store,
            backup: None,
        }
    }

    static TEST_VERSION: Lazy<Version> = Lazy::new(|| Version::parse("0.24.1").unwrap());

    struct TestStep {
        id: &'static str,
        module: &'static str,
        revision: u64,
        fail: bool,
    }

    impl MigrationStep<Ctx> for TestStep {
        fn id(&self) -> &'static str {
            self.id
        }

        fn module(&self) -> &'static str {
            self.module
        }

        fn revision(&self) -> u64 {
            self.revision
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            self.id
        }

        fn run(&self, _: &mut Ctx) -> anyhow::Result<()> {
            if self.fail {
                bail!("boom")
            }
            Ok(())
        }
    }

    const CLEAN_PROFILES: &str = "items:\n- uid: a\n  name: A\n  type: config\n  config:\n    \
                                  type: file\n    source:\n      type: local\n      binding:\n        \
                                  type: managed\n        file: a.yaml\n";

    fn stamped_profiles(revision: u64, body: &str) -> String {
        format!("_nyanpasu:\n  document: profiles\n  schema_revision: {revision}\n{body}")
    }

    fn profiles_at(applied: u64, stamped: bool) -> MigrationStore {
        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_string(),
            ModuleState {
                applied_revision: applied,
                baseline_revision: applied,
                stamped,
            },
        );
        store
    }

    struct DocumentCase {
        _temp: tempfile::TempDir,
        ctx: Ctx,
    }

    impl DocumentCase {
        fn new(store: Option<MigrationStore>, profiles: &str) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config_dir = temp.path().join("config");
            let data_dir = temp.path().join("data");
            std::fs::create_dir_all(&config_dir).unwrap();
            std::fs::create_dir_all(&data_dir).unwrap();
            if let Some(store) = store {
                store
                    .flush_atomic(&config_dir.join(STORE_FILE_NAME))
                    .unwrap();
            }
            std::fs::write(config_dir.join("profiles.yaml"), profiles).unwrap();
            Self {
                _temp: temp,
                ctx: Ctx::new(config_dir, data_dir),
            }
        }

        fn runner(&self) -> anyhow::Result<Runner> {
            Runner::with_context(TEST_VERSION.clone(), false, self.ctx.clone())
        }

        fn profiles(&self) -> String {
            std::fs::read_to_string(self.ctx.profiles_path()).unwrap()
        }

        fn state(&self) -> Option<String> {
            std::fs::read_to_string(self.ctx.state_path()).ok()
        }

        fn stamp(&self) -> Option<u64> {
            chimera_core::format::inspect(&self.profiles())
                .unwrap()
                .stamp()
                .map(|stamp| stamp.schema_revision)
        }
    }

    #[test]
    fn stamp_replaces_detection_when_the_state_is_lost() {
        // Shape detection would put this legacy file at revision 0.
        let case = DocumentCase::new(
            None,
            &stamped_profiles(4, include_str!("fixtures/v1_6_1/profiles.yaml")),
        );

        let runner = case.runner().unwrap();

        assert_eq!(
            runner.store.module_state("profiles"),
            ModuleState {
                applied_revision: 4,
                baseline_revision: 4,
                stamped: true,
            }
        );
    }

    #[test]
    fn unstamped_profiles_are_detected_then_stamped_once_migrated() {
        let case = DocumentCase::new(None, include_str!("fixtures/v1_6_1/profiles.yaml"));

        let mut runner = case.runner().unwrap();
        assert_eq!(runner.store.module_state("profiles").baseline_revision, 0);
        runner.run_pending().unwrap();

        assert_eq!(case.stamp(), Some(4));
        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert_eq!(state.module_state("profiles").applied_revision, 4);
        assert!(state.module_state("profiles").stamped);
    }

    #[test]
    fn refuses_a_lost_stamp() {
        let case = DocumentCase::new(Some(profiles_at(4, true)), CLEAN_PROFILES);
        let state = case.state();

        let cause = case.runner().unwrap_err();
        assert!(matches!(cause.downcast_ref::<RevisionError>(),
            Some(RevisionError::MissingDocumentStamp { path, state_path })
                if *path == case.ctx.profiles_path() && *state_path == case.ctx.state_path()));
        let error = format!("{cause:#}");

        assert!(error.contains("lost the `_nyanpasu` stamp"), "{error}");
        assert!(error.contains(STORE_FILE_NAME), "{error}");
        assert_eq!(case.state(), state);
        assert_eq!(case.profiles(), CLEAN_PROFILES);
    }

    #[test]
    fn refuses_a_stamp_behind_the_state() {
        let profiles = stamped_profiles(2, CLEAN_PROFILES);
        let case = DocumentCase::new(Some(profiles_at(4, true)), &profiles);
        let state = case.state();

        let cause = case.runner().unwrap_err();
        assert!(matches!(cause.downcast_ref::<RevisionError>(),
            Some(RevisionError::DocumentBehindState { path, state_path, module, read: 2, applied: 4 })
                if *path == case.ctx.profiles_path() && *state_path == case.ctx.state_path() && module == "profiles"));
        let error = format!("{cause:#}");

        assert!(error.contains("stamped at revision 2"), "{error}");
        assert!(error.contains("revision 4"), "{error}");
        assert_eq!(case.state(), state);
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn refuses_a_stamp_newer_than_this_build() {
        let profiles = stamped_profiles(5, CLEAN_PROFILES);
        let case = DocumentCase::new(None, &profiles);

        let error = format!("{:#}", case.runner().unwrap_err());
        assert!(error.contains("newer version"), "{error}");
        // `--migration <id>` builds the same runner, forced or not.
        let forced = Runner::with_context(TEST_VERSION.clone(), true, case.ctx.clone());
        assert!(forced.is_err());

        assert_eq!(case.state(), None);
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn refuses_malformed_or_foreign_stamps_before_any_write() {
        for profiles in [
            "_nyanpasu: 4\nitems: []\n".to_string(),
            "_nyanpasu:\n  document: application\n  schema_revision: 4\nitems: []\n".to_string(),
        ] {
            let case = DocumentCase::new(None, &profiles);

            let source = case.runner().unwrap_err();
            assert!(
                matches!(source.downcast_ref::<core_runner::ExecutionError>(),
                Some(core_runner::ExecutionError::InspectDocument { module, .. })
                    if module == "profiles")
            );
            let error = format!("{source:#}");

            assert!(
                error.contains("failed to inspect the profiles files"),
                "{error}"
            );
            assert_eq!(case.state(), None);
            assert_eq!(case.profiles(), profiles);
        }
    }

    #[test]
    fn stamp_ahead_of_the_state_skips_the_recorded_steps_without_rewriting() {
        // The file reached revision 4 but the state was not updated after.
        let profiles = stamped_profiles(4, CLEAN_PROFILES);
        let case = DocumentCase::new(Some(profiles_at(2, true)), &profiles);

        let mut runner = case.runner().unwrap();
        runner.run_pending().unwrap();

        assert_eq!(runner.store.module_state("profiles").applied_revision, 4);
        assert_eq!(
            runner.store.task_state("profiles/repair_schema"),
            Some(MigrationState::Skipped)
        );
        assert_eq!(case.profiles(), profiles);
    }

    #[test]
    fn document_step_that_does_not_stamp_is_a_defect() {
        let case = DocumentCase::new(Some(profiles_at(0, false)), CLEAN_PROFILES);
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: false,
        };
        let mut runner = case.runner().unwrap();

        let cause = runner.run_step(&step).unwrap_err();
        assert!(
            matches!(cause.downcast_ref::<core_runner::ExecutionError>(),
            Some(core_runner::ExecutionError::VerifyDocumentStamp { step })
                if step == "profiles/example")
        );
        let error = format!("{cause:#}");

        assert!(
            error.contains("without leaving its document stamped"),
            "{error}"
        );
        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Failed)
        );
    }

    struct RemovesProfiles;

    impl MigrationStep<Ctx> for RemovesProfiles {
        fn id(&self) -> &'static str {
            "profiles/removes"
        }

        fn module(&self) -> &'static str {
            "profiles"
        }

        fn revision(&self) -> u64 {
            1
        }

        fn introduced_in(&self) -> &'static Version {
            &TEST_VERSION
        }

        fn name(&self) -> &'static str {
            "RemovesProfiles"
        }

        fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
            std::fs::remove_file(ctx.profiles_path())?;
            Ok(())
        }
    }

    #[test]
    fn document_step_that_removes_its_document_is_a_defect() {
        let case = DocumentCase::new(Some(profiles_at(0, false)), CLEAN_PROFILES);
        let mut runner = case.runner().unwrap();

        let error = format!("{:#}", runner.run_step(&RemovesProfiles).unwrap_err());

        assert!(
            error.contains("without leaving its document stamped"),
            "{error}"
        );
        assert_eq!(runner.store.module_state("profiles").applied_revision, 0);
    }

    #[test]
    fn document_steps_do_not_run_without_their_document() {
        let mut runner = runner_with_store(profiles_at(0, false), false);
        let step = TestStep {
            id: "profiles/example",
            module: "profiles",
            revision: 1,
            fail: true,
        };

        runner.run_step(&step).unwrap();

        assert_eq!(
            runner.store.task_state(step.id()),
            Some(MigrationState::Skipped)
        );
    }

    #[test]
    fn a_completed_step_records_the_stamp_it_wrote() {
        let legacy = "current: [a]\nitems:\n- {uid: a, type: local, name: A, file: a.yaml}\n";
        let case = DocumentCase::new(Some(profiles_at(3, false)), legacy);
        let mut runner = case.runner().unwrap();
        let step = registry::find_migration("profiles/repair_schema").unwrap();

        runner.run_migration(step).unwrap();

        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert!(state.module_state("profiles").stamped);
        assert_eq!(state.task_state(step.id()), Some(MigrationState::Completed));
        assert!(state.tasks[step.id()].started_at.is_some());
        // So losing the stamp afterwards is refused rather than taken for a
        // file that never had one.
        let payload = chimera_core::format::inspect(&case.profiles())
            .unwrap()
            .into_payload();
        std::fs::write(
            case.ctx.profiles_path(),
            serde_yaml_ng::to_string(&payload).unwrap(),
        )
        .unwrap();
        let error = format!("{:#}", case.runner().unwrap_err());
        assert!(error.contains("lost the `_nyanpasu` stamp"), "{error}");
    }

    #[test]
    fn a_stamp_found_at_startup_is_recorded() {
        let case = DocumentCase::new(
            Some(profiles_at(4, false)),
            &stamped_profiles(4, CLEAN_PROFILES),
        );

        case.runner().unwrap();

        let state = MigrationStore::load(&case.ctx.state_path()).unwrap();
        assert!(state.module_state("profiles").stamped);
    }

    #[test]
    fn prerelease_target_reaches_same_release_migration() {
        assert!(introduced_in_reached(
            &Version::parse("0.23.0").unwrap(),
            &Version::parse("0.23.0-rc.1").unwrap()
        ));
    }

    #[test]
    fn runner_migrates_once_and_persists_completion() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let paths = PathResolver::with_base_dirs(config.clone(), data);
        let mut runner = Runner::with_context(
            Version::parse("0.24.1").unwrap(),
            false,
            Ctx::from_paths(paths.clone()),
        )
        .unwrap();
        runner.run_pending().unwrap();

        assert!(config.join("application.yaml").exists());
        assert!(config.join("session-state.yaml").exists());
        assert!(config.join("clash-config.yaml").exists());
        assert!(config.join("migration-state.yaml").exists());

        let before = std::fs::read(config.join("clash-config.yaml")).unwrap();
        let mut rerun = Runner::with_context(
            Version::parse("0.24.1").unwrap(),
            false,
            Ctx::from_paths(paths),
        )
        .unwrap();
        rerun.run_pending().unwrap();
        assert_eq!(
            std::fs::read(config.join("clash-config.yaml")).unwrap(),
            before
        );
    }

    #[test]
    fn pending_migration_snapshots_legacy_config_and_storage_before_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let legacy = serde_yaml_ng::to_string(&crate::config::chimera::IVerge::template()).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();
        let storage = b"opaque closed storage file before migration";
        std::fs::write(paths.storage_path(), storage).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        runner.run_pending().unwrap();

        let snapshot = runner
            .backup
            .as_ref()
            .expect("existing config must be snapshotted");
        assert_eq!(
            std::fs::read_to_string(snapshot.path.join("config/chimera-config.yaml")).unwrap(),
            legacy
        );
        assert_eq!(
            std::fs::read(snapshot.path.join("data/storage.db")).unwrap(),
            storage
        );
        assert!(
            !snapshot.path.join("config/application.yaml").exists(),
            "backup must predate typed writes"
        );
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(snapshot.path.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["kind"], "migration");
        assert_eq!(
            manifest["sources"]["config"],
            paths.app_config_dir().to_str().unwrap()
        );
        assert!(paths.application_config_path().exists());
        assert!(paths.session_state_path().exists());
        assert!(paths.clash_config_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn backup_failure_blocks_rewrites_and_retry_creates_a_snapshot() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let legacy = serde_yaml_ng::to_string(&crate::config::chimera::IVerge::template()).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();
        let external = temp.path().join("outside-backups");
        std::fs::create_dir_all(&external).unwrap();
        symlink(&external, paths.backups_dir()).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        let state_before = std::fs::read(paths.app_config_dir().join(STORE_FILE_NAME)).unwrap();
        let error = runner
            .run_pending()
            .expect_err("unsafe backup destination must block migration");
        assert!(
            matches!(error.downcast_ref::<core_runner::ExecutionError>(),
            Some(core_runner::ExecutionError::CreateBackup { source })
                if format!("{source:#}").contains("failed to back up the config"))
        );
        assert!(format!("{error:#}").contains("failed to back up the config"));
        assert_eq!(
            std::fs::read_to_string(paths.chimera_config_path()).unwrap(),
            legacy
        );
        assert_eq!(
            std::fs::read(paths.app_config_dir().join(STORE_FILE_NAME)).unwrap(),
            state_before
        );
        assert!(!paths.application_config_path().exists());
        assert!(!paths.session_state_path().exists());
        assert!(!paths.clash_config_path().exists());
        assert!(std::fs::read_dir(&external).unwrap().next().is_none());
        assert_eq!(std::fs::read_link(paths.backups_dir()).unwrap(), external);

        std::fs::remove_file(paths.backups_dir()).unwrap();
        runner.run_pending().unwrap();
        let snapshot = runner.backup.as_ref().expect("retry needs a real backup");
        assert_eq!(
            std::fs::read_to_string(snapshot.path.join("config/chimera-config.yaml")).unwrap(),
            legacy
        );
        assert!(!snapshot.path.join("config/application.yaml").exists());
        assert!(paths.application_config_path().exists());
    }

    #[test]
    fn a_storage_snapshot_failure_leaves_no_partial_and_blocks_typed_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let legacy = serde_yaml_ng::to_string(&crate::config::chimera::IVerge::template()).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();
        std::fs::create_dir(paths.storage_path()).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        let error = runner.run_pending().unwrap_err();
        assert!(format!("{error:#}").contains("failed to back up the config"));
        assert!(runner.backup.is_none());
        assert_eq!(
            std::fs::read_to_string(paths.chimera_config_path()).unwrap(),
            legacy
        );
        assert!(!paths.application_config_path().exists());
        assert!(!paths.session_state_path().exists());
        assert!(!paths.clash_config_path().exists());
        assert!(paths.storage_path().is_dir());
        assert_eq!(
            std::fs::read_dir(paths.backups_dir()).unwrap().count(),
            0,
            "no completed or partial backup survives"
        );
    }

    #[test]
    fn a_failed_migration_keeps_the_pre_step_backup_and_never_marks_success() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let malformed = "enable_system_proxy: [broken yaml";
        std::fs::write(paths.chimera_config_path(), malformed).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        let error = runner
            .run_pending()
            .expect_err("invalid legacy input must fail migration");
        assert!(
            format!("{error:#}").contains("failed to read legacy config"),
            "{error:#}"
        );
        let backup = runner
            .backup
            .as_ref()
            .expect("snapshot must survive a later step failure");
        assert_eq!(
            std::fs::read_to_string(backup.path.join("config/chimera-config.yaml")).unwrap(),
            malformed
        );
        assert_eq!(
            std::fs::read_to_string(paths.chimera_config_path()).unwrap(),
            malformed
        );
        assert!(backup.path.join("manifest.json").is_file());
        let state = MigrationStore::load(&paths.app_config_dir().join(STORE_FILE_NAME)).unwrap();
        assert!(
            state.app.last_succeeded.is_none(),
            "failed migration cannot claim success"
        );
        assert!(
            state
                .tasks
                .values()
                .any(|entry| entry.state == MigrationState::Failed)
        );
        assert!(!paths.application_config_path().exists());
        assert!(!paths.session_state_path().exists());
        assert!(!paths.clash_config_path().exists());
    }

    #[test]
    fn a_fresh_install_with_only_generated_baselines_needs_no_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let paths = PathResolver::with_base_dirs(config, data);
        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        runner.run_pending().unwrap();
        assert!(runner.backup.is_none());
        assert!(!paths.backups_dir().exists());
        assert!(paths.application_config_path().exists());
    }

    #[test]
    fn a_second_start_does_not_create_a_backup_without_pending_steps() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let legacy = serde_yaml_ng::to_string(&crate::config::chimera::IVerge::template()).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();
        let mut first = Runner::with_paths(paths.clone(), false).unwrap();
        first.run_pending().unwrap();
        assert!(first.backup.is_some());
        let count = std::fs::read_dir(paths.backups_dir()).unwrap().count();
        let mut second = Runner::with_paths(paths.clone(), false).unwrap();
        second.run_pending().unwrap();
        assert!(
            second.backup.is_none(),
            "a healthy restart should not create another snapshot"
        );
        assert_eq!(
            std::fs::read_dir(paths.backups_dir()).unwrap().count(),
            count
        );
    }

    #[test]
    fn startup_runner_keeps_legacy_profiles_untouched_until_client_migration() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let legacy_profiles = "current: [legacy]\nitems:\n- {uid: legacy, type: local, name: Legacy, file: legacy.yaml}\n";
        std::fs::write(config.join("profiles.yaml"), legacy_profiles).unwrap();

        let paths = PathResolver::with_base_dirs(config.clone(), data);
        let mut runner = Runner::with_paths_before_profile_client_migration(paths, false).unwrap();
        runner.run_pending().unwrap();

        assert_eq!(
            std::fs::read_to_string(config.join("profiles.yaml")).unwrap(),
            legacy_profiles
        );
        let state = MigrationStore::load(&config.join(STORE_FILE_NAME)).unwrap();
        assert!(!state.modules.contains_key("profiles"));
    }
}
