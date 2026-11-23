//! Ref-aligned typed-config migration orchestration and typed YAML state checks.
//! The desktop adapter supplies only the legacy Verge/Clash conversion and
//! session recovery hook; this crate owns revision policy, typed-file
//! classification, data-preserving write order and destination protection.

use crate::{
    context::Ctx,
    module::{MigrationStep, ModuleMigrator},
};
use anyhow::{Context as _, bail};
use chimera_config::{application::ChimeraAppConfig, clash::config::ClashConfig};
use semver::Version;
use serde::de::DeserializeOwned;
use serde_yaml::Value;
use std::{path::Path, sync::LazyLock};

/// The environment-specific legacy conversion and disposable window-state recovery.
/// Methods must preserve the original error source and never delete input files.
pub trait TypedConfigHost: AsRef<Ctx> {
    fn session_state_exists(&self) -> anyhow::Result<bool>;
    fn convert_legacy_yaml(&self) -> anyhow::Result<(String, String, String)>;
    fn repair_legacy_clash_yaml(&self) -> anyhow::Result<String>;
}

pub static MIGRATOR: TypedConfigMigrator = TypedConfigMigrator;

static VERSION_0_23_0: LazyLock<Version> = LazyLock::new(|| Version::parse("0.23.0").unwrap());
pub static SPLIT_LEGACY_CONFIG: SplitLegacyConfig = SplitLegacyConfig;
pub static REPAIR_CLASH_CONFIG: RepairClashConfig = RepairClashConfig;
struct TypedConfigSteps<C>(std::marker::PhantomData<C>);
impl<C: TypedConfigHost + 'static> TypedConfigSteps<C> {
    const ALL: [&'static dyn MigrationStep<C>; 2] = [&SPLIT_LEGACY_CONFIG, &REPAIR_CLASH_CONFIG];
}

pub struct TypedConfigMigrator;

impl<C: TypedConfigHost + 'static> ModuleMigrator<C> for TypedConfigMigrator {
    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn detect_baseline(&self, ctx: &C) -> anyhow::Result<u64> {
        match typed_file_state(ctx)? {
            TypedFileState::All => Ok(current_revision()),
            TypedFileState::None => Ok(0),
            TypedFileState::NeedsClashRepair => Ok(1),
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep<C>] {
        &TypedConfigSteps::<C>::ALL
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SplitLegacyConfig;

impl<C: TypedConfigHost + 'static> MigrationStep<C> for SplitLegacyConfig {
    fn id(&self) -> &'static str {
        "typed_config/split_legacy_config"
    }

    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_23_0
    }

    fn name(&self) -> &'static str {
        "SplitLegacyConfig"
    }

    fn run(&self, ctx: &mut C) -> anyhow::Result<()> {
        match typed_file_state(ctx)? {
            TypedFileState::All => return Ok(()),
            TypedFileState::None => {}
            TypedFileState::NeedsClashRepair => return Ok(()),
        }

        let (application_yaml, session_yaml, clash_yaml) = ctx.convert_legacy_yaml()?;

        write_typed_files(ctx, &application_yaml, &session_yaml, &clash_yaml)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RepairClashConfig;

impl<C: TypedConfigHost + 'static> MigrationStep<C> for RepairClashConfig {
    fn id(&self) -> &'static str {
        "typed_config/repair_clash_config"
    }

    fn module(&self) -> &'static str {
        "typed_config"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_23_0
    }

    fn name(&self) -> &'static str {
        "RepairClashConfig"
    }

    fn run(&self, ctx: &mut C) -> anyhow::Result<()> {
        match typed_file_state(ctx)? {
            TypedFileState::All => return Ok(()),
            TypedFileState::None => {
                bail!("cannot repair typed clash config before split_legacy_config has completed")
            }
            TypedFileState::NeedsClashRepair => {}
        }

        let clash_yaml = ctx.repair_legacy_clash_yaml()?;
        crate::fs::atomic_write(&ctx.as_ref().clash_config_path(), clash_yaml.as_bytes())
            .context("failed to write repaired clash config")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedFileState {
    All,
    None,
    NeedsClashRepair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedClashFileState {
    Missing,
    Typed,
    LegacyRuntime,
    Unrecognized,
}

pub fn typed_file_state<C: TypedConfigHost>(ctx: &C) -> anyhow::Result<TypedFileState> {
    let application_exists = typed_path_exists(&ctx.as_ref().application_config_path())?;
    // Ref: recover session state before partial-file classification so bad
    // window data is preserved even when other config files are absent.
    let session_exists = ctx.session_state_exists()?;
    let clash_state = classify_shared_clash_file(ctx)?;

    if !application_exists && !session_exists {
        return match clash_state {
            SharedClashFileState::Missing | SharedClashFileState::LegacyRuntime => {
                Ok(TypedFileState::None)
            }
            SharedClashFileState::Typed => partial_typed_file_state(
                vec!["clash-config.yaml"],
                vec!["application.yaml", "session-state.yaml"],
            ),
            SharedClashFileState::Unrecognized => bail!(
                "unrecognized typed config migration state: existing {} is neither a valid typed clash config nor a recognized legacy runtime config; restore or remove it before retrying",
                ctx.as_ref().clash_config_path().display()
            ),
        };
    }

    if application_exists && session_exists {
        match clash_state {
            SharedClashFileState::Typed => {
                validate_existing_typed_files(ctx)?;
                return Ok(TypedFileState::All);
            }
            SharedClashFileState::Missing | SharedClashFileState::LegacyRuntime => {
                validate_existing_application_and_session(ctx)?;
                return Ok(TypedFileState::NeedsClashRepair);
            }
            SharedClashFileState::Unrecognized => {}
        }
    }

    if clash_state == SharedClashFileState::Unrecognized {
        bail!(
            "unrecognized typed config migration state: existing {} is neither a valid typed clash config nor a recognized legacy runtime config; restore or remove it before retrying",
            ctx.as_ref().clash_config_path().display()
        );
    }

    let mut existing = Vec::new();
    let mut missing = Vec::new();
    push_file_state(
        &mut existing,
        &mut missing,
        application_exists,
        "application.yaml",
    );
    push_file_state(
        &mut existing,
        &mut missing,
        session_exists,
        "session-state.yaml",
    );
    match clash_state {
        SharedClashFileState::Typed => existing.push("clash-config.yaml"),
        SharedClashFileState::Missing | SharedClashFileState::LegacyRuntime => {
            missing.push("clash-config.yaml")
        }
        SharedClashFileState::Unrecognized => unreachable!("handled above"),
    }

    partial_typed_file_state(existing, missing)
}

fn push_file_state(
    existing: &mut Vec<&'static str>,
    missing: &mut Vec<&'static str>,
    exists: bool,
    name: &'static str,
) {
    if exists {
        existing.push(name);
    } else {
        missing.push(name);
    }
}

fn partial_typed_file_state(
    existing: Vec<&'static str>,
    missing: Vec<&'static str>,
) -> anyhow::Result<TypedFileState> {
    bail!(
        "partial typed config migration state: existing [{}], missing [{}]; restore or remove the typed config files before retrying",
        existing.join(", "),
        missing.join(", ")
    );
}

pub fn typed_path_exists(path: &Path) -> anyhow::Result<bool> {
    crate::fs::config_node_exists(path)
        .with_context(|| format!("failed to inspect typed config file {}", path.display()))
}

pub fn classify_shared_clash_file<C: AsRef<Ctx>>(ctx: &C) -> anyhow::Result<SharedClashFileState> {
    let path = ctx.as_ref().clash_config_path();
    if !typed_path_exists(&path)? {
        return Ok(SharedClashFileState::Missing);
    }

    if read_yaml::<ClashConfig>(&path).is_ok() {
        return Ok(SharedClashFileState::Typed);
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    if looks_like_legacy_runtime_clash_mapping(&value) {
        return Ok(SharedClashFileState::LegacyRuntime);
    }

    Ok(SharedClashFileState::Unrecognized)
}

fn looks_like_legacy_runtime_clash_mapping(value: &Value) -> bool {
    let Some(map) = value.as_mapping() else {
        return false;
    };

    [
        "mixed-port",
        "port",
        "socks-port",
        "redir-port",
        "tproxy-port",
        "external-controller",
        "allow-lan",
        "log-level",
        "mode",
        "ipv6",
        "dns",
        "tun",
        "listeners",
        "proxies",
        "proxy-groups",
        "rules",
        "proxy-providers",
        "rule-providers",
    ]
    .iter()
    .any(|key| map.contains_key(Value::String((*key).to_string())))
}

fn validate_existing_typed_files<C: TypedConfigHost>(ctx: &C) -> anyhow::Result<()> {
    validate_existing_application_and_session(ctx)?;
    read_yaml::<ClashConfig>(&ctx.as_ref().clash_config_path())
        .context("failed to validate existing clash config")?;
    Ok(())
}

fn validate_existing_application_and_session<C: TypedConfigHost>(ctx: &C) -> anyhow::Result<()> {
    read_yaml::<ChimeraAppConfig>(&ctx.as_ref().application_config_path())
        .context("failed to validate existing application config")?;
    ctx.session_state_exists()?;
    Ok(())
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn write_typed_files<C: TypedConfigHost>(
    ctx: &C,
    application_yaml: &str,
    session_yaml: &str,
    clash_yaml: &str,
) -> anyhow::Result<()> {
    let outputs = [
        (
            "application config",
            ctx.as_ref().application_config_path(),
            application_yaml.as_bytes(),
        ),
        (
            "session state",
            ctx.as_ref().session_state_path(),
            session_yaml.as_bytes(),
        ),
        (
            "clash config",
            ctx.as_ref().clash_config_path(),
            clash_yaml.as_bytes(),
        ),
    ];
    let mut created = Vec::new();

    for (name, path, contents) in outputs {
        let existed = path.exists();
        if let Err(error) = crate::fs::atomic_write(&path, contents) {
            for created_path in created.iter().rev() {
                let _ = std::fs::remove_file(created_path);
            }
            return Err(error).with_context(|| format!("failed to write migrated {name}"));
        }
        if !existed {
            created.push(path);
        }
    }

    Ok(())
}

fn current_revision() -> u64 {
    2 // two frozen typed-config migration steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::MigrationPaths;
    use chimera_config::state::PersistentState;
    use chimera_contracts::{
        BoxedCause,
        storage::{HotkeyStore, HotkeyStoreOpener},
    };
    use std::{path::Path, sync::Arc};

    #[derive(Debug)]
    struct NeverOpenHotkeys;
    impl HotkeyStoreOpener for NeverOpenHotkeys {
        fn open(&self, _: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
            unreachable!("typed migration does not open KV")
        }
    }

    struct FakeHost {
        ctx: Ctx,
        fail_conversion: bool,
    }

    impl AsRef<Ctx> for FakeHost {
        fn as_ref(&self) -> &Ctx {
            &self.ctx
        }
    }

    impl TypedConfigHost for FakeHost {
        fn session_state_exists(&self) -> anyhow::Result<bool> {
            let path = self.ctx.session_state_path();
            let present = crate::fs::config_node_exists(&path)?;
            if present {
                read_yaml::<PersistentState>(&path)?;
            }
            Ok(present)
        }
        fn convert_legacy_yaml(&self) -> anyhow::Result<(String, String, String)> {
            if self.fail_conversion {
                anyhow::bail!("injected legacy conversion failure");
            }
            Ok((
                serde_yaml::to_string(&ChimeraAppConfig::default())?,
                serde_yaml::to_string(&PersistentState::default())?,
                serde_yaml::to_string(&ClashConfig::default())?,
            ))
        }
        fn repair_legacy_clash_yaml(&self) -> anyhow::Result<String> {
            if self.fail_conversion {
                anyhow::bail!("injected clash conversion failure");
            }
            Ok(serde_yaml::to_string(&ClashConfig::default())?)
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        host: FakeHost,
    }
    impl Fixture {
        fn new() -> Self {
            let temp = tempfile::tempdir().unwrap();
            let config = temp.path().join("config");
            let data = temp.path().join("data");
            std::fs::create_dir_all(&config).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let host = FakeHost {
                ctx: Ctx::new(
                    MigrationPaths::new(config, data),
                    Arc::new(NeverOpenHotkeys),
                ),
                fail_conversion: false,
            };
            Self { _temp: temp, host }
        }
    }

    #[test]
    fn shared_migrator_splits_once_and_detects_complete_typed_files() {
        let mut fixture = Fixture::new();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.host).unwrap(), 0);
        let steps = <TypedConfigMigrator as ModuleMigrator<FakeHost>>::steps(&MIGRATOR);
        assert_eq!(
            steps
                .iter()
                .map(|step| (step.id(), step.revision()))
                .collect::<Vec<_>>(),
            [
                ("typed_config/split_legacy_config", 1),
                ("typed_config/repair_clash_config", 2),
            ]
        );
        SPLIT_LEGACY_CONFIG.run(&mut fixture.host).unwrap();
        assert_eq!(
            typed_file_state(&fixture.host).unwrap(),
            TypedFileState::All
        );
        assert_eq!(MIGRATOR.detect_baseline(&fixture.host).unwrap(), 2);
        let bytes = std::fs::read(fixture.host.ctx.application_config_path()).unwrap();
        SPLIT_LEGACY_CONFIG.run(&mut fixture.host).unwrap();
        assert_eq!(
            std::fs::read(fixture.host.ctx.application_config_path()).unwrap(),
            bytes
        );
    }

    #[test]
    fn typed_clash_repair_keeps_application_and_session_unchanged() {
        let mut fixture = Fixture::new();
        let (app, session, _clash) = fixture.host.convert_legacy_yaml().unwrap();
        std::fs::write(fixture.host.ctx.application_config_path(), &app).unwrap();
        std::fs::write(fixture.host.ctx.session_state_path(), &session).unwrap();
        std::fs::write(fixture.host.ctx.clash_config_path(), "mixed-port: 7890\n").unwrap();
        assert_eq!(
            classify_shared_clash_file(&fixture.host).unwrap(),
            SharedClashFileState::LegacyRuntime
        );
        assert_eq!(MIGRATOR.detect_baseline(&fixture.host).unwrap(), 1);
        REPAIR_CLASH_CONFIG.run(&mut fixture.host).unwrap();
        assert_eq!(MIGRATOR.detect_baseline(&fixture.host).unwrap(), 2);
        assert_eq!(
            std::fs::read(fixture.host.ctx.application_config_path()).unwrap(),
            app.as_bytes()
        );
        assert_eq!(
            std::fs::read(fixture.host.ctx.session_state_path()).unwrap(),
            session.as_bytes()
        );
        let _: ClashConfig = read_yaml(&fixture.host.ctx.clash_config_path()).unwrap();
    }

    #[test]
    fn failure_from_conversion_does_not_create_typed_files() {
        let mut fixture = Fixture::new();
        fixture.host.fail_conversion = true;
        let error = SPLIT_LEGACY_CONFIG.run(&mut fixture.host).unwrap_err();
        assert!(format!("{error:#}").contains("injected legacy conversion failure"));
        assert!(!fixture.host.ctx.application_config_path().exists());
        assert!(!fixture.host.ctx.session_state_path().exists());
        assert!(!fixture.host.ctx.clash_config_path().exists());
    }

    #[test]
    fn partial_typed_files_fail_without_replacing_original_bytes() {
        let fixture = Fixture::new();
        let path = fixture.host.ctx.application_config_path();
        let original = b"legacy invalid: [yaml";
        std::fs::write(&path, original).unwrap();
        let error = MIGRATOR.detect_baseline(&fixture.host).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("partial typed config migration state")
        );
        assert_eq!(std::fs::read(path).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn linked_clash_output_is_not_followed_and_new_typed_outputs_are_rolled_back() {
        use std::os::unix::fs::symlink;
        let mut fixture = Fixture::new();
        let target = fixture._temp.path().join("outside.yaml");
        std::fs::write(&target, "mixed-port: 7890\n").unwrap();
        symlink(&target, fixture.host.ctx.clash_config_path()).unwrap();
        let error = SPLIT_LEGACY_CONFIG.run(&mut fixture.host).unwrap_err();
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!(std::fs::read(&target).unwrap(), b"mixed-port: 7890\n");
        assert!(!fixture.host.ctx.application_config_path().exists());
        assert!(!fixture.host.ctx.session_state_path().exists());
        assert_eq!(
            std::fs::read_link(fixture.host.ctx.clash_config_path()).unwrap(),
            target
        );
    }
}
