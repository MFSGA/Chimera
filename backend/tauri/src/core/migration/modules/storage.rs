//! Legacy hotkey migration, aligned with ref's two-step StorageMigrator.
//!
//! The typed application config is the final authority. A short-lived
//! HotkeyStore handle bridges old, unversioned KV data during migration only.

use super::super::{
    Ctx, MigrationCheckError, MigrationStep, ModuleMigrator, StepCheck,
    fs::{self, try_exists},
};
use chimera_contracts::BoxedCause;
use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml_ng::{Mapping, Value};
use std::{io, path::PathBuf};

pub static MIGRATOR: StorageMigrator = StorageMigrator;

// ref introduces these steps in 2.0.0; the Chimera release line is 0.28.x.
// Keep the two original revision numbers and migrate existing 0.28.3 data.
static VERSION_0_28_3: Lazy<Version> = Lazy::new(|| Version::parse("0.28.3").unwrap());
static HOTKEYS_TO_KV: MigrateHotkeysToKv = MigrateHotkeysToKv;
static HOTKEYS_TO_TYPED_CONFIG: MigrateHotkeysToTypedConfig = MigrateHotkeysToTypedConfig;
static STEPS: [&dyn MigrationStep; 2] = [&HOTKEYS_TO_KV, &HOTKEYS_TO_TYPED_CONFIG];

const HOTKEYS_KEY: &str = "hotkeys";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to read {path:?}")]
    ReadFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to parse YAML")]
    ParseYaml(#[source] serde_yaml_ng::Error),
    #[error("failed to serialize YAML")]
    SerializeYaml(#[source] serde_yaml_ng::Error),
    #[error("failed to inspect migration input")]
    InspectInput(#[source] MigrationCheckError),
    #[error("failed to write migration output")]
    WriteOutput(#[source] anyhow::Error),
    #[error("failed to open storage")]
    OpenStorage(#[source] BoxedCause),
    #[error("failed to save hotkeys")]
    SaveHotkeys(#[source] BoxedCause),
    #[error("failed to remove hotkeys")]
    RemoveHotkeys(#[source] BoxedCause),
    #[error("hotkeys is not a list in the legacy config")]
    ReadHotkeyList,
    #[error("no hotkeys in the key-value storage")]
    FindHotkeys,
    #[error("failed to create storage parent {path:?}")]
    CreateStorageDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

pub struct StorageMigrator;

impl ModuleMigrator for StorageMigrator {
    fn module(&self) -> &'static str {
        "storage"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        // Baseline detection runs BEFORE Runner::ensure_backup. Do not open
        // redb here: the production opener may recover an unusable database,
        // and no user data may be reset before the pre-migration snapshot.
        let legacy_path = ctx.chimera_config_path();
        if try_exists(&legacy_path)? {
            // A malformed or unreadable legacy input must be checked again
            // after the runner has preserved a snapshot. A conservative
            // baseline schedules step 1 instead of failing before backup.
            match std::fs::read_to_string(&legacy_path) {
                Ok(raw) => match serde_yaml_ng::from_str::<Mapping>(&raw) {
                    Ok(config) => {
                        if config
                            .get(HOTKEYS_KEY)
                            .is_some_and(|value| value.as_sequence().is_some())
                        {
                            return Ok(0);
                        }
                    }
                    Err(_) => return Ok(0),
                },
                Err(_) => return Ok(0),
            }
        }

        // Existing storage may contain the hotkey key. Conservatively check it
        // after backup instead of opening (and potentially recovering) redb.
        if try_exists(&ctx.storage_path())? {
            return Ok(1);
        }
        Ok(current_revision())
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToKv;

impl MigrationStep for MigrateHotkeysToKv {
    fn id(&self) -> &'static str {
        "storage/hotkeys_to_kv"
    }
    fn module(&self) -> &'static str {
        "storage"
    }
    fn revision(&self) -> u64 {
        1
    }
    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_28_3
    }
    fn name(&self) -> &'static str {
        "MigrateHotkeysToKv"
    }
    fn check(&self, ctx: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
        let config: Option<Mapping> = fs::read_yaml_if_exists(&ctx.chimera_config_path())?;
        Ok(Some(StepCheck::from_needed(config.is_some_and(|config| {
            config
                .get(HOTKEYS_KEY)
                .is_some_and(|value| value.as_sequence().is_some())
        }))))
    }
    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        (|| -> Result<(), Error> {
            let config_path = ctx.chimera_config_path();
            let raw = std::fs::read_to_string(&config_path).map_err(|source| Error::ReadFile {
                path: config_path.clone(),
                source,
            })?;
            let mut config: Mapping = serde_yaml_ng::from_str(&raw).map_err(Error::ParseYaml)?;
            let hotkeys_key = Value::String(HOTKEYS_KEY.to_string());
            let hotkeys = config
                .get(&hotkeys_key)
                .and_then(Value::as_sequence)
                .ok_or(Error::ReadHotkeyList)?;
            let hotkey_strings: Vec<String> = hotkeys
                .iter()
                .filter_map(|value| value.as_str().map(ToString::to_string))
                .collect();
            if !hotkey_strings.is_empty() {
                let storage_path = ctx.storage_path();
                if let Some(parent) = storage_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|source| {
                        Error::CreateStorageDirectory {
                            path: parent.to_path_buf(),
                            source,
                        }
                    })?;
                }
                let storage = ctx
                    .hotkey_store_opener()
                    .open(&storage_path)
                    .map_err(Error::OpenStorage)?;
                storage.write(&hotkey_strings).map_err(Error::SaveHotkeys)?;
                tracing::info!(
                    count = hotkey_strings.len(),
                    "migrated legacy hotkeys to KV storage"
                );
            }
            config.remove(&hotkeys_key);
            let new_config = serde_yaml_ng::to_string(&config).map_err(Error::SerializeYaml)?;
            fs::atomic_write(&config_path, new_config.as_bytes()).map_err(Error::WriteOutput)?;
            Ok(())
        })()
        .map_err(Into::into)
    }
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

/// Hotkeys in the unversioned KV store cannot join the typed application's
/// revision/degradation protocol. Move the complete list to application.yaml.
#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToTypedConfig;

impl MigrationStep for MigrateHotkeysToTypedConfig {
    fn id(&self) -> &'static str {
        "storage/hotkeys_to_typed_config"
    }
    fn module(&self) -> &'static str {
        "storage"
    }
    fn revision(&self) -> u64 {
        2
    }
    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_28_3
    }
    fn name(&self) -> &'static str {
        "MigrateHotkeysToTypedConfig"
    }
    fn check(&self, ctx: &Ctx) -> Result<Option<StepCheck>, MigrationCheckError> {
        if read_kv_hotkeys(ctx)?.is_none() {
            return Ok(Some(StepCheck::Satisfied));
        }
        if !try_exists(&ctx.application_config_path())? {
            return Err(MigrationCheckError::Unrecognized(
                "cannot move hotkeys into the typed application config before it exists"
                    .to_string(),
            ));
        }
        Ok(Some(StepCheck::Needed))
    }
    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        (|| -> Result<(), Error> {
            let mut hotkeys = read_kv_hotkeys(ctx)
                .map_err(Error::InspectInput)?
                .ok_or(Error::FindHotkeys)?;
            hotkeys.retain(|hotkey| !is_script_hotkey(hotkey));
            let application_path = ctx.application_config_path();
            let raw =
                std::fs::read_to_string(&application_path).map_err(|source| Error::ReadFile {
                    path: application_path.clone(),
                    source,
                })?;
            let mut application: Mapping =
                serde_yaml_ng::from_str(&raw).map_err(Error::ParseYaml)?;
            // The KV list is authoritative until the key is removed. This
            // includes an explicitly empty list, which must not resurrect hotkeys.
            application.insert(
                Value::String(HOTKEYS_KEY.to_string()),
                Value::Sequence(hotkeys.iter().cloned().map(Value::String).collect()),
            );
            let serialized =
                serde_yaml_ng::to_string(&application).map_err(Error::SerializeYaml)?;
            fs::atomic_write(&application_path, serialized.as_bytes())
                .map_err(Error::WriteOutput)?;
            tracing::info!(
                count = hotkeys.len(),
                "migrated KV hotkeys into typed application config"
            );
            let storage = open_storage(ctx)
                .map_err(Error::InspectInput)?
                .ok_or(Error::FindHotkeys)?;
            storage.remove().map_err(Error::RemoveHotkeys)?;
            Ok(())
        })()
        .map_err(Into::into)
    }
}

/// Match the upstream removed Script proxy mode exactly, excluding lookalikes.
fn is_script_hotkey(hotkey: &str) -> bool {
    hotkey.split(',').next() == Some("clash_mode_script")
}

fn read_kv_hotkeys(ctx: &Ctx) -> Result<Option<Vec<String>>, MigrationCheckError> {
    let Some(storage) = open_storage(ctx)? else {
        return Ok(None);
    };
    storage
        .read()
        .map_err(|source| MigrationCheckError::Storage {
            path: ctx.storage_path(),
            source,
        })
}

fn open_storage(
    ctx: &Ctx,
) -> Result<Option<Box<dyn chimera_contracts::storage::HotkeyStore>>, MigrationCheckError> {
    let storage_path = ctx.storage_path();
    if !try_exists(&storage_path)? {
        return Ok(None);
    }
    ctx.hotkey_store_opener()
        .open(&storage_path)
        .map(Some)
        .map_err(|source| MigrationCheckError::Storage {
            path: storage_path,
            source,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::path::PathResolver;
    use chimera_contracts::storage::{HotkeyStore, HotkeyStoreOpener};
    use std::{path::Path, sync::Arc};

    struct Fixture {
        _temp: tempfile::TempDir,
        ctx: Ctx,
    }

    impl Fixture {
        fn new(application_yaml: Option<&str>, kv_hotkeys: Option<&[&str]>) -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config = temp.path().join("config");
            let data = temp.path().join("data");
            std::fs::create_dir_all(&config).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let ctx = Ctx::new(config, data);
            if let Some(application_yaml) = application_yaml {
                std::fs::write(ctx.application_config_path(), application_yaml).unwrap();
            }
            if let Some(hotkeys) = kv_hotkeys {
                let store = ctx.hotkey_store_opener().open(&ctx.storage_path()).unwrap();
                store
                    .write(&hotkeys.iter().map(ToString::to_string).collect::<Vec<_>>())
                    .unwrap();
            }
            Self { _temp: temp, ctx }
        }

        fn typed_hotkeys(&self) -> Option<Vec<String>> {
            let raw = std::fs::read_to_string(self.ctx.application_config_path()).ok()?;
            let config: Mapping = serde_yaml_ng::from_str(&raw).unwrap();
            config.get(HOTKEYS_KEY)?.as_sequence().map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str().map(ToString::to_string))
                    .collect()
            })
        }

        fn kv_hotkeys(&self) -> Option<Vec<String>> {
            read_kv_hotkeys(&self.ctx).unwrap()
        }

        fn run_typed_hotkey_step(&mut self) {
            assert_eq!(
                HOTKEYS_TO_TYPED_CONFIG.check(&self.ctx).unwrap(),
                Some(StepCheck::Needed)
            );
            HOTKEYS_TO_TYPED_CONFIG.run(&mut self.ctx).unwrap();
            assert_eq!(
                HOTKEYS_TO_TYPED_CONFIG.check(&self.ctx).unwrap(),
                Some(StepCheck::Satisfied)
            );
        }
    }

    #[test]
    fn migrates_kv_hotkeys_to_typed_config_without_losing_other_fields() {
        let mut fixture = Fixture::new(
            Some("language: en\nhotkeys: [clash_mode_rule,Control+Q]\n"),
            Some(&["toggle_tun_mode,Control+T", "clash_mode_rule,Control+R"]),
        );
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 1);
        fixture.run_typed_hotkey_step();
        assert_eq!(
            fixture.typed_hotkeys().unwrap(),
            ["toggle_tun_mode,Control+T", "clash_mode_rule,Control+R"]
        );
        let content = std::fs::read_to_string(fixture.ctx.application_config_path()).unwrap();
        let config: Mapping = serde_yaml_ng::from_str(&content).unwrap();
        assert_eq!(config.get("language").and_then(Value::as_str), Some("en"));
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn empty_kv_hotkeys_explicitly_clears_typed_list() {
        let mut fixture = Fixture::new(
            Some("hotkeys: ['clash_mode_rule,Control+Q']\nlanguage: en\n"),
            Some(&[]),
        );
        fixture.run_typed_hotkey_step();
        assert_eq!(fixture.typed_hotkeys(), Some(vec![]));
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn absent_kv_skips_without_creating_or_rewriting_application_file() {
        let mut fixture = Fixture::new(Some("hotkeys: ['keep,Control+K']\n"), None);
        let before = std::fs::read(fixture.ctx.application_config_path()).unwrap();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 2);
        assert_eq!(
            HOTKEYS_TO_TYPED_CONFIG.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Satisfied)
        );
        assert_eq!(
            std::fs::read(fixture.ctx.application_config_path()).unwrap(),
            before
        );
        assert!(!fixture.ctx.storage_path().exists());
        HOTKEYS_TO_TYPED_CONFIG
            .run(&mut fixture.ctx)
            .expect_err("an unnecessary direct run must fail");
        assert_eq!(
            std::fs::read(fixture.ctx.application_config_path()).unwrap(),
            before
        );
    }

    #[test]
    fn kv_hotkeys_are_not_removed_when_typed_application_is_missing() {
        let fixture = Fixture::new(None, Some(&["clash_mode_rule,Control+Q"]));
        let error = HOTKEYS_TO_TYPED_CONFIG.check(&fixture.ctx).unwrap_err();
        assert!(matches!(error, MigrationCheckError::Unrecognized(_)));
        assert_eq!(
            fixture.kv_hotkeys(),
            Some(vec!["clash_mode_rule,Control+Q".into()])
        );
        assert!(!fixture.ctx.application_config_path().exists());
    }

    #[test]
    fn moves_legacy_yaml_through_kv_to_typed_config() {
        let mut fixture = Fixture::new(Some("language: en\n"), None);
        std::fs::write(
            fixture.ctx.chimera_config_path(),
            "hotkeys: ['toggle_tun_mode,Control+T', 'clash_mode_rule,Control+R']\ntheme_color: blue\n",
        )
        .unwrap();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 0);
        assert_eq!(
            HOTKEYS_TO_KV.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Needed)
        );
        HOTKEYS_TO_KV.run(&mut fixture.ctx).unwrap();
        assert_eq!(
            HOTKEYS_TO_KV.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Satisfied)
        );
        assert_eq!(fixture.kv_hotkeys().unwrap().len(), 2);
        let legacy: Mapping = serde_yaml_ng::from_str(
            &std::fs::read_to_string(fixture.ctx.chimera_config_path()).unwrap(),
        )
        .unwrap();
        assert!(!legacy.contains_key(HOTKEYS_KEY));
        assert_eq!(
            legacy.get("theme_color").and_then(Value::as_str),
            Some("blue")
        );
        fixture.run_typed_hotkey_step();
        assert_eq!(fixture.typed_hotkeys().unwrap().len(), 2);
        assert_eq!(fixture.kv_hotkeys(), None);
        // The baseline intentionally does not open redb before backup. When
        // a store file remains but the key is gone it conservatively returns
        // 1, then the step check skips without rewriting typed configuration.
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 1);
        assert_eq!(
            HOTKEYS_TO_TYPED_CONFIG.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Satisfied)
        );
    }

    #[test]
    fn empty_legacy_list_is_removed_without_creating_kv() {
        let mut fixture = Fixture::new(Some("language: en\n"), None);
        std::fs::write(
            fixture.ctx.chimera_config_path(),
            "hotkeys: []\nother: true\n",
        )
        .unwrap();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 0);
        HOTKEYS_TO_KV.run(&mut fixture.ctx).unwrap();
        assert_eq!(
            HOTKEYS_TO_KV.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Satisfied)
        );
        assert_eq!(fixture.kv_hotkeys(), None);
        assert!(!fixture.ctx.storage_path().exists());
    }

    #[test]
    fn invalid_typed_yaml_keeps_kv_and_can_retry_after_repair() {
        let mut fixture = Fixture::new(
            Some("hotkeys: [invalid yaml\n"),
            Some(&["toggle_tun_mode,Control+T"]),
        );
        assert_eq!(
            HOTKEYS_TO_TYPED_CONFIG.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Needed)
        );
        let error = HOTKEYS_TO_TYPED_CONFIG.run(&mut fixture.ctx).unwrap_err();
        assert!(
            error
                .downcast_ref::<Error>()
                .is_some_and(|error| matches!(error, Error::ParseYaml(_)))
        );
        assert_eq!(
            fixture.kv_hotkeys(),
            Some(vec!["toggle_tun_mode,Control+T".into()])
        );
        assert_eq!(
            std::fs::read_to_string(fixture.ctx.application_config_path()).unwrap(),
            "hotkeys: [invalid yaml\n"
        );
        std::fs::write(fixture.ctx.application_config_path(), "language: en\n").unwrap();
        fixture.run_typed_hotkey_step();
        assert_eq!(
            fixture.typed_hotkeys(),
            Some(vec!["toggle_tun_mode,Control+T".into()])
        );
        assert_eq!(fixture.kv_hotkeys(), None);
    }

    #[test]
    fn filters_only_exact_removed_script_hotkey_function() {
        let mut fixture = Fixture::new(
            Some("language: en\n"),
            Some(&[
                "clash_mode_script,Control+S",
                "clash_mode_rule,Control+R",
                "clash_mode_script_extra,Control+E",
                " clash_mode_script,Control+X",
            ]),
        );
        fixture.run_typed_hotkey_step();
        assert_eq!(
            fixture.typed_hotkeys(),
            Some(vec![
                "clash_mode_rule,Control+R".into(),
                "clash_mode_script_extra,Control+E".into(),
                " clash_mode_script,Control+X".into(),
            ])
        );
    }

    #[cfg(unix)]
    #[test]
    fn typed_output_symlink_is_rejected_without_deleting_kv_or_target() {
        use std::os::unix::fs::symlink;
        let mut fixture = Fixture::new(None, Some(&["clash_mode_rule,Control+Q"]));
        let external = fixture.ctx.paths().app_config_dir().join("external.yaml");
        std::fs::write(&external, "language: en\n").unwrap();
        symlink(&external, fixture.ctx.application_config_path()).unwrap();
        let error = HOTKEYS_TO_TYPED_CONFIG.run(&mut fixture.ctx).unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(
            std::fs::read_to_string(&external).unwrap(),
            "language: en\n"
        );
        assert_eq!(
            fixture.kv_hotkeys(),
            Some(vec!["clash_mode_rule,Control+Q".into()])
        );
    }

    #[test]
    fn runner_migrates_legacy_hotkeys_with_backup_and_restarts_idempotently() {
        use crate::{
            config::chimera::IVerge,
            core::migration::{runner::Runner, store::MigrationStore},
        };
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let mut old_config: Mapping =
            serde_yaml_ng::from_str(&serde_yaml_ng::to_string(&IVerge::template()).unwrap())
                .unwrap();
        old_config.insert(
            Value::String(HOTKEYS_KEY.into()),
            Value::Sequence(vec![
                Value::String("toggle_tun_mode,Control+T".into()),
                Value::String("clash_mode_rule,Control+R".into()),
            ]),
        );
        let legacy = serde_yaml_ng::to_string(&old_config).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        runner.run_pending().unwrap();
        let snapshot = paths.backups_dir();
        let backups: Vec<_> = std::fs::read_dir(&snapshot)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_dir())
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read_to_string(backups[0].join("config/chimera-config.yaml")).unwrap(),
            legacy
        );
        let state =
            MigrationStore::load(&paths.app_config_dir().join("migration-state.yaml")).unwrap();
        assert_eq!(state.module_state("storage").applied_revision, 2);
        let app_before = std::fs::read(paths.application_config_path()).unwrap();
        let application: Mapping = serde_yaml_ng::from_slice(&app_before).unwrap();
        let migrated: Vec<_> = application
            .get(HOTKEYS_KEY)
            .unwrap()
            .as_sequence()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(
            migrated,
            ["toggle_tun_mode,Control+T", "clash_mode_rule,Control+R"]
        );
        let typed: chimera_config::application::ChimeraAppConfig =
            serde_yaml_ng::from_slice(&app_before).unwrap();
        assert_eq!(typed.hotkeys.len(), 2);
        let opener = chimera_core::storage::RedbHotkeyStoreOpener;
        let db = opener.open(&paths.storage_path()).unwrap();
        assert_eq!(db.read().unwrap(), None);
        drop(db);

        let mut restarted = Runner::with_paths(paths.clone(), false).unwrap();
        restarted.run_pending().unwrap();
        assert_eq!(
            std::fs::read(paths.application_config_path()).unwrap(),
            app_before
        );
        assert_eq!(std::fs::read_dir(snapshot).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn runner_does_not_move_legacy_hotkeys_without_verified_backup() {
        use crate::{config::chimera::IVerge, core::migration::runner::Runner};
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let paths = PathResolver::with_base_dirs(config_dir, data_dir);
        let mut config: Mapping =
            serde_yaml_ng::from_str(&serde_yaml_ng::to_string(&IVerge::template()).unwrap())
                .unwrap();
        config.insert(
            Value::String(HOTKEYS_KEY.into()),
            Value::Sequence(vec![Value::String("toggle_tun_mode,Control+T".into())]),
        );
        let legacy = serde_yaml_ng::to_string(&config).unwrap();
        std::fs::write(paths.chimera_config_path(), &legacy).unwrap();
        let external = temp.path().join("external-backups");
        std::fs::create_dir_all(&external).unwrap();
        symlink(&external, paths.backups_dir()).unwrap();

        let mut runner = Runner::with_paths(paths.clone(), false).unwrap();
        let error = runner.run_pending().unwrap_err();
        assert!(format!("{error:#}").contains("failed to back up"));
        assert_eq!(
            std::fs::read_to_string(paths.chimera_config_path()).unwrap(),
            legacy
        );
        assert!(!paths.storage_path().exists());
        assert!(!paths.application_config_path().exists());
        assert_eq!(std::fs::read_dir(external).unwrap().count(), 0);
    }

    #[derive(Debug)]
    struct FailingHotkeyStoreOpener;
    impl HotkeyStoreOpener for FailingHotkeyStoreOpener {
        fn open(&self, _path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
            Err(Box::new(io::Error::from(io::ErrorKind::PermissionDenied)))
        }
    }

    #[test]
    fn opener_failure_keeps_original_storage_error_and_path() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let paths = PathResolver::with_base_dirs(config, data);
        let store_path = paths.storage_path();
        std::fs::write(&store_path, b"unmodified").unwrap();
        let ctx = Ctx::with_hotkey_store_opener(paths, Arc::new(FailingHotkeyStoreOpener));
        let error = read_kv_hotkeys(&ctx).unwrap_err();
        assert!(
            matches!(error, MigrationCheckError::Storage { path, source }
            if path == store_path && source.downcast_ref::<io::Error>().is_some_and(|e| e.kind() == io::ErrorKind::PermissionDenied))
        );
        assert_eq!(std::fs::read(store_path).unwrap(), b"unmodified");
    }
}
