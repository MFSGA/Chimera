//! Legacy hotkey migration, aligned with ref's two-step StorageMigrator.
//!
//! The typed application config is the final authority. A short-lived
//! HotkeyStore handle bridges old, unversioned KV data during migration only.

use crate::{
    context::Ctx,
    document::{self, try_exists},
    module::{MigrationCheckError, MigrationStep, ModuleMigrator, StepCheck},
};
use chimera_contracts::BoxedCause;
use semver::Version;
use serde_yaml_ng::{Mapping, Value};
use std::sync::LazyLock;
use std::{io, path::PathBuf};

pub static MIGRATOR: StorageMigrator = StorageMigrator;

// ref introduces these steps in 2.0.0; the Chimera release line is 0.28.x.
// Keep the two original revision numbers and migrate existing 0.28.3 data.
static VERSION_0_28_3: LazyLock<Version> = LazyLock::new(|| Version::parse("0.28.3").unwrap());
pub static HOTKEYS_TO_KV: MigrateHotkeysToKv = MigrateHotkeysToKv;
pub static HOTKEYS_TO_TYPED_CONFIG: MigrateHotkeysToTypedConfig = MigrateHotkeysToTypedConfig;

pub const HOTKEYS_KEY: &str = "hotkeys";

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

struct StorageSteps<C>(std::marker::PhantomData<C>);
impl<C: AsRef<Ctx> + 'static> StorageSteps<C> {
    const ALL: [&'static dyn MigrationStep<C>; 2] = [&HOTKEYS_TO_KV, &HOTKEYS_TO_TYPED_CONFIG];
}

impl<C: AsRef<Ctx> + 'static> ModuleMigrator<C> for StorageMigrator {
    fn module(&self) -> &'static str {
        "storage"
    }

    fn detect_baseline(&self, ctx: &C) -> anyhow::Result<u64> {
        let ctx = ctx.as_ref();
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

    fn steps(&self) -> &'static [&'static dyn MigrationStep<C>] {
        &StorageSteps::<C>::ALL
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToKv;

impl<C: AsRef<Ctx> + 'static> MigrationStep<C> for MigrateHotkeysToKv {
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
    fn check(&self, ctx: &C) -> Result<Option<StepCheck>, MigrationCheckError> {
        let ctx = ctx.as_ref();
        let config: Option<Mapping> = document::read_yaml_if_exists(&ctx.chimera_config_path())?;
        Ok(Some(StepCheck::from_needed(config.is_some_and(|config| {
            config
                .get(HOTKEYS_KEY)
                .is_some_and(|value| value.as_sequence().is_some())
        }))))
    }
    fn run(&self, ctx: &mut C) -> anyhow::Result<()> {
        let ctx = ctx.as_ref();
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
            crate::fs::atomic_write(&config_path, new_config.as_bytes())
                .map_err(Error::WriteOutput)?;
            Ok(())
        })()
        .map_err(Into::into)
    }
}

fn current_revision() -> u64 {
    StorageSteps::<Ctx>::ALL
        .last()
        .map_or(0, |step| step.revision())
}

/// Hotkeys in the unversioned KV store cannot join the typed application's
/// revision/degradation protocol. Move the complete list to application.yaml.
#[derive(Debug, Clone, Copy)]
pub struct MigrateHotkeysToTypedConfig;

impl<C: AsRef<Ctx> + 'static> MigrationStep<C> for MigrateHotkeysToTypedConfig {
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
    fn check(&self, ctx: &C) -> Result<Option<StepCheck>, MigrationCheckError> {
        let ctx = ctx.as_ref();
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
    fn run(&self, ctx: &mut C) -> anyhow::Result<()> {
        let ctx = ctx.as_ref();
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
            crate::fs::atomic_write(&application_path, serialized.as_bytes())
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

pub fn read_kv_hotkeys<C: AsRef<Ctx>>(ctx: &C) -> Result<Option<Vec<String>>, MigrationCheckError> {
    let Some(storage) = open_storage(ctx.as_ref())? else {
        return Ok(None);
    };
    storage
        .read()
        .map_err(|source| MigrationCheckError::Storage {
            path: ctx.as_ref().storage_path(),
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
    use crate::context::MigrationPaths;
    use chimera_contracts::storage::{HotkeyStore, HotkeyStoreOpener};
    use std::{
        path::Path,
        sync::{Arc, Mutex},
    };

    #[derive(Debug, Default)]
    struct FakeState {
        value: Option<Vec<String>>,
        opens: usize,
        fail_write: bool,
    }

    #[derive(Debug, Clone)]
    struct FakeHotkeyStore {
        state: Arc<Mutex<FakeState>>,
    }

    impl HotkeyStore for FakeHotkeyStore {
        fn read(&self) -> Result<Option<Vec<String>>, BoxedCause> {
            Ok(self.state.lock().unwrap().value.clone())
        }
        fn write(&self, hotkeys: &[String]) -> Result<(), BoxedCause> {
            let mut state = self.state.lock().unwrap();
            if state.fail_write {
                return Err(Box::new(io::Error::from(io::ErrorKind::PermissionDenied)));
            }
            state.value = Some(hotkeys.to_vec());
            Ok(())
        }
        fn remove(&self) -> Result<(), BoxedCause> {
            self.state.lock().unwrap().value = None;
            Ok(())
        }
    }

    impl HotkeyStoreOpener for FakeHotkeyStore {
        fn open(&self, path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
            self.state.lock().unwrap().opens += 1;
            if !path.exists() {
                std::fs::write(path, b"fake-kv-node")?;
            }
            Ok(Box::new(self.clone()))
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        ctx: Ctx,
        state: Arc<Mutex<FakeState>>,
    }

    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config = temp.path().join("config");
            let data = temp.path().join("data");
            std::fs::create_dir_all(&config).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let state = Arc::new(Mutex::new(FakeState::default()));
            let opener = FakeHotkeyStore {
                state: state.clone(),
            };
            let ctx = Ctx::new(MigrationPaths::new(config, data), Arc::new(opener));
            Self {
                _temp: temp,
                ctx,
                state,
            }
        }

        fn with_kv(&self, hotkeys: Vec<String>) {
            self.state.lock().unwrap().value = Some(hotkeys);
            std::fs::write(self.ctx.storage_path(), b"fake-kv-node").unwrap();
        }

        fn kv(&self) -> Option<Vec<String>> {
            self.state.lock().unwrap().value.clone()
        }
    }

    #[test]
    fn shared_context_migrates_legacy_hotkeys_to_typed_config_in_upstream_order() {
        let mut fixture = Fixture::new();
        std::fs::write(
            fixture.ctx.chimera_config_path(),
            "hotkeys: ['clash_mode_script,Ctrl+S', 'clash_mode_rule,Ctrl+R']\nother: 42\n",
        )
        .unwrap();
        std::fs::write(fixture.ctx.application_config_path(), "language: en\n").unwrap();
        let steps = <StorageMigrator as ModuleMigrator<Ctx>>::steps(&MIGRATOR);
        assert_eq!(
            steps.iter().map(|step| step.id()).collect::<Vec<_>>(),
            ["storage/hotkeys_to_kv", "storage/hotkeys_to_typed_config"]
        );
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 0);
        HOTKEYS_TO_KV.run(&mut fixture.ctx).unwrap();
        assert_eq!(
            fixture.kv(),
            Some(vec![
                "clash_mode_script,Ctrl+S".into(),
                "clash_mode_rule,Ctrl+R".into()
            ])
        );
        let legacy: Mapping =
            serde_yaml_ng::from_slice(&std::fs::read(fixture.ctx.chimera_config_path()).unwrap())
                .unwrap();
        assert!(legacy.get("hotkeys").is_none());
        assert_eq!(legacy.get("other").and_then(Value::as_i64), Some(42));
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 1);
        assert_eq!(
            HOTKEYS_TO_TYPED_CONFIG.check(&fixture.ctx).unwrap(),
            Some(StepCheck::Needed)
        );
        HOTKEYS_TO_TYPED_CONFIG.run(&mut fixture.ctx).unwrap();
        let typed: Mapping = serde_yaml_ng::from_slice(
            &std::fs::read(fixture.ctx.application_config_path()).unwrap(),
        )
        .unwrap();
        assert_eq!(typed.get("language").and_then(Value::as_str), Some("en"));
        assert_eq!(
            typed.get("hotkeys").unwrap().as_sequence().unwrap(),
            &vec![Value::String("clash_mode_rule,Ctrl+R".into())]
        );
        assert_eq!(fixture.kv(), None);
    }

    #[test]
    fn baseline_inspection_does_not_open_or_recover_storage_before_backup() {
        let fixture = Fixture::new();
        std::fs::write(fixture.ctx.storage_path(), b"may-be-corrupt-db").unwrap();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.ctx).unwrap(), 1);
        assert_eq!(fixture.state.lock().unwrap().opens, 0);
        assert_eq!(
            std::fs::read(fixture.ctx.storage_path()).unwrap(),
            b"may-be-corrupt-db"
        );
    }

    #[test]
    fn kv_write_failure_preserves_legacy_yaml_for_retry() {
        let mut fixture = Fixture::new();
        let original = b"hotkeys: [clash_mode_rule,Ctrl+R]\nother: keep\n";
        std::fs::write(fixture.ctx.chimera_config_path(), original).unwrap();
        fixture.state.lock().unwrap().fail_write = true;
        let error = HOTKEYS_TO_KV.run(&mut fixture.ctx).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<Error>(),
            Some(Error::SaveHotkeys(_))
        ));
        assert_eq!(
            std::fs::read(fixture.ctx.chimera_config_path()).unwrap(),
            original
        );
        assert_eq!(fixture.kv(), None);
    }

    #[test]
    fn empty_kv_list_overwrites_stale_typed_hotkeys_and_removes_old_key() {
        let mut fixture = Fixture::new();
        fixture.with_kv(vec![]);
        std::fs::write(
            fixture.ctx.application_config_path(),
            "hotkeys: ['stale,Ctrl+Q']\nlanguage: en\n",
        )
        .unwrap();
        HOTKEYS_TO_TYPED_CONFIG.run(&mut fixture.ctx).unwrap();
        let typed: Mapping = serde_yaml_ng::from_slice(
            &std::fs::read(fixture.ctx.application_config_path()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            typed.get("hotkeys").unwrap().as_sequence().unwrap().len(),
            0
        );
        assert_eq!(fixture.kv(), None);
    }

    #[cfg(unix)]
    #[test]
    fn rejected_linked_typed_target_keeps_original_and_kv_value() {
        use std::os::unix::fs::symlink;
        let mut fixture = Fixture::new();
        fixture.with_kv(vec!["clash_mode_rule,Ctrl+R".into()]);
        let target = fixture.ctx.paths().app_config_dir().join("external.yaml");
        std::fs::write(&target, b"language: en\n").unwrap();
        symlink(&target, fixture.ctx.application_config_path()).unwrap();
        let error = HOTKEYS_TO_TYPED_CONFIG.run(&mut fixture.ctx).unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read(&target).unwrap(), b"language: en\n");
        assert_eq!(fixture.kv(), Some(vec!["clash_mode_rule,Ctrl+R".into()]));
    }
}
