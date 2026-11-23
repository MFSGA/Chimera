//! Chimera-specific legacy Verge and runtime Clash conversion for shared TypedConfigMigrator.
//! The shared crate owns migration steps and typed-file state selection.

use super::super::Ctx;
use crate::{
    bridge::typed_config_from_legacy_parts,
    config::{chimera::IVerge, clash::IClashTemp},
    utils::help,
};
use anyhow::Context as _;
use chimera_config::state::PersistentState;
use chimera_migration::modules::typed_config::TypedConfigHost;
use serde::{Serialize, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub use chimera_migration::modules::typed_config::{
    MIGRATOR, REPAIR_CLASH_CONFIG, RepairClashConfig, SPLIT_LEGACY_CONFIG, SharedClashFileState,
    SplitLegacyConfig, TypedConfigMigrator, TypedFileState, classify_shared_clash_file,
    typed_file_state, typed_path_exists,
};

impl TypedConfigHost for Ctx {
    fn session_state_exists(&self) -> anyhow::Result<bool> {
        Ok(read_session_state(self)?.is_some())
    }

    fn convert_legacy_yaml(&self) -> anyhow::Result<(String, String, String)> {
        let legacy = read_legacy_verge(&self.chimera_config_path())?;
        let legacy_clash = read_legacy_clash_inputs(self)?;
        let (application, session, clash) = typed_config_from_legacy_parts(&legacy, &legacy_clash)?;
        Ok((
            serialize_yaml(&application)
                .context("failed to serialize migrated application config")?,
            serialize_yaml(&session).context("failed to serialize migrated session state")?,
            serialize_yaml(&clash).context("failed to serialize migrated clash config")?,
        ))
    }

    fn repair_legacy_clash_yaml(&self) -> anyhow::Result<String> {
        let legacy = read_legacy_verge(&self.chimera_config_path())?;
        let legacy_clash = read_legacy_clash_inputs(self)?;
        let (_, _, clash) = typed_config_from_legacy_parts(&legacy, &legacy_clash)?;
        serialize_yaml(&clash).context("failed to serialize repaired clash config")
    }
}

fn read_session_state(ctx: &Ctx) -> anyhow::Result<Option<PersistentState>> {
    crate::core::session_state::read(&ctx.session_state_path())
        .context("failed to read or recover existing session state")
}

fn read_legacy_verge(path: &Path) -> anyhow::Result<IVerge> {
    let mut merged = IVerge::template();
    if !typed_path_exists(path)? {
        return Ok(merged);
    }

    let legacy: IVerge = read_yaml(path)
        .with_context(|| format!("failed to read legacy config {}", path.display()))?;
    merged.patch_config(legacy);
    Ok(merged)
}

fn read_legacy_clash_inputs(ctx: &Ctx) -> anyhow::Result<Mapping> {
    let mut merged = IClashTemp::template().0;

    if classify_shared_clash_file(ctx)? == SharedClashFileState::LegacyRuntime {
        merge_legacy_clash_file(&mut merged, &ctx.clash_config_path())?;
    }
    merge_legacy_clash_file(&mut merged, &ctx.clash_guard_overrides_path())?;

    Ok(merged)
}

fn merge_legacy_clash_file(merged: &mut Mapping, path: &Path) -> anyhow::Result<()> {
    if !typed_path_exists(path)? {
        return Ok(());
    }

    let legacy = help::read_merge_mapping(&path.to_path_buf())
        .with_context(|| format!("failed to read legacy clash overrides {}", path.display()))?;
    for (key, value) in legacy {
        if !matches!(value, Value::Null) {
            merged.insert(key, value);
        }
    }
    Ok(())
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn serialize_yaml<T: Serialize>(value: &T) -> anyhow::Result<String> {
    serde_yaml::to_string(value).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::{MigrationStep, ModuleMigrator};
    use chimera_config::application::ChimeraAppConfig;

    use chimera_config::clash::config::ClashConfig;

    fn current_revision() -> u64 {
        <TypedConfigMigrator as ModuleMigrator<Ctx>>::steps(&MIGRATOR)
            .last()
            .map_or(0, |step| step.revision())
    }

    fn test_ctx() -> (Ctx, tempfile::TempDir) {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        (Ctx::new(config_dir, data_dir), temp)
    }

    fn write_yaml<T: Serialize>(path: &Path, value: &T) {
        let raw = serde_yaml::to_string(value).unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, raw).unwrap();
    }

    fn read_typed<T: DeserializeOwned>(path: &Path) -> T {
        let raw = std::fs::read_to_string(path).unwrap();
        serde_yaml::from_str(&raw).unwrap()
    }

    fn write_legacy_clash(path: &Path) {
        let mut legacy = IClashTemp::template().0;
        legacy.insert("mixed-port".into(), 8123.into());
        legacy.insert("external-controller".into(), "127.0.0.1:19090".into());
        legacy.insert("allow-lan".into(), true.into());
        legacy.insert("mode".into(), "global".into());
        legacy.insert("ipv6".into(), true.into());
        write_yaml(path, &legacy);
    }

    #[test]
    fn legacy_runtime_clash_config_alone_detects_baseline_zero() {
        let (ctx, _temp) = test_ctx();
        write_legacy_clash(&ctx.clash_config_path());
        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), 0);
    }

    #[test]
    fn split_legacy_config_consumes_legacy_runtime_file() {
        let (mut ctx, _temp) = test_ctx();
        let legacy = IVerge {
            enable_system_proxy: Some(true),
            enable_tun_mode: Some(true),
            verge_mixed_port: Some(7899),
            ..IVerge::template()
        };
        write_yaml(&ctx.chimera_config_path(), &legacy);
        write_legacy_clash(&ctx.clash_config_path());

        SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap();

        let application: ChimeraAppConfig = read_typed(&ctx.application_config_path());
        assert!(application.enable_system_proxy);
        let _: PersistentState = read_typed(&ctx.session_state_path());
        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert!(clash.enable_tun_mode);
        assert_eq!(clash.mixed_port.start_port, 7899);
        assert_eq!(
            classify_shared_clash_file(&ctx).unwrap(),
            SharedClashFileState::Typed
        );
    }

    #[test]
    fn existing_valid_typed_files_are_baselined_without_overwrite() {
        let (ctx, _temp) = test_ctx();
        let application = ChimeraAppConfig {
            enable_system_proxy: true,
            ..ChimeraAppConfig::default()
        };
        write_yaml(&ctx.application_config_path(), &application);
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());
        let before = std::fs::read(&ctx.application_config_path()).unwrap();

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
        assert_eq!(
            std::fs::read(&ctx.application_config_path()).unwrap(),
            before
        );
    }

    #[test]
    fn invalid_session_is_preserved_during_typed_config_baseline_detection() {
        let (ctx, temp) = test_ctx();
        write_yaml(&ctx.application_config_path(), &ChimeraAppConfig::default());
        write_yaml(&ctx.clash_config_path(), &ClashConfig::default());
        let application = std::fs::read(&ctx.application_config_path()).unwrap();
        let clash = std::fs::read(&ctx.clash_config_path()).unwrap();
        let raw = b"window_state: [invalid";
        std::fs::write(ctx.session_state_path(), raw).unwrap();

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
        let restored: PersistentState = read_typed(&ctx.session_state_path());
        assert!(restored.window_state.is_empty());
        assert_eq!(
            std::fs::read(ctx.application_config_path()).unwrap(),
            application
        );
        assert_eq!(std::fs::read(ctx.clash_config_path()).unwrap(), clash);
        let preserved = std::fs::read_dir(temp.path().join("config"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("session-state.yaml.corrupt-")
            })
            .collect::<Vec<_>>();
        assert_eq!(preserved.len(), 1);
        assert_eq!(std::fs::read(&preserved[0]).unwrap(), raw);
    }

    #[test]
    fn corrupt_session_in_partial_typed_config_is_preserved_before_rejection() {
        let (ctx, temp) = test_ctx();
        let raw = b"window_state: [invalid";
        std::fs::write(ctx.session_state_path(), raw).unwrap();

        let error = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("partial typed config migration state")
        );
        let restored: PersistentState = read_typed(&ctx.session_state_path());
        assert!(restored.window_state.is_empty());
        assert!(!ctx.application_config_path().exists());
        assert!(!ctx.clash_config_path().exists());
        let preserved = std::fs::read_dir(temp.path().join("config"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("session-state.yaml.corrupt-")
            })
            .collect::<Vec<_>>();
        assert_eq!(preserved.len(), 1);
        assert_eq!(std::fs::read(&preserved[0]).unwrap(), raw);
    }

    #[test]
    fn application_and_session_with_legacy_clash_detect_repair_baseline() {
        let (ctx, _temp) = test_ctx();
        write_yaml(&ctx.application_config_path(), &ChimeraAppConfig::default());
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_legacy_clash(&ctx.clash_config_path());

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), 1);
    }

    #[test]
    fn repair_clash_config_rebuilds_typed_state_from_legacy_inputs() {
        let (mut ctx, _temp) = test_ctx();
        let legacy = IVerge {
            enable_tun_mode: Some(true),
            verge_mixed_port: Some(7899),
            ..IVerge::template()
        };
        write_yaml(&ctx.chimera_config_path(), &legacy);
        write_yaml(&ctx.application_config_path(), &ChimeraAppConfig::default());
        write_yaml(&ctx.session_state_path(), &PersistentState::default());
        write_legacy_clash(&ctx.clash_config_path());

        REPAIR_CLASH_CONFIG.run(&mut ctx).unwrap();

        let clash: ClashConfig = read_typed(&ctx.clash_config_path());
        assert!(clash.enable_tun_mode);
        assert_eq!(clash.mixed_port.start_port, 7899);
        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
    }

    #[test]
    fn unrecognized_clash_config_fails_without_overwrite() {
        let (ctx, _temp) = test_ctx();
        std::fs::write(ctx.clash_config_path(), "unexpected: true\n").unwrap();

        let err = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("unrecognized typed config migration state"),
            "{err:#}"
        );
        assert_eq!(
            std::fs::read_to_string(ctx.clash_config_path()).unwrap(),
            "unexpected: true\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn dangling_application_link_is_not_mistaken_for_missing_typed_config() {
        use std::os::unix::fs::symlink;
        let (ctx, temp) = test_ctx();
        let missing_target = temp.path().join("missing-application.yaml");
        let path = ctx.application_config_path();
        symlink(&missing_target, &path).unwrap();

        let error = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("partial typed config migration state"),
            "{error:#}"
        );
        assert!(error.to_string().contains("existing [application.yaml]"));
        assert_eq!(std::fs::read_link(&path).unwrap(), missing_target);
        assert!(!missing_target.exists());
        assert!(!ctx.session_state_path().exists());
        assert!(!ctx.clash_config_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_clash_link_is_not_treated_as_missing_legacy_input() {
        use std::os::unix::fs::symlink;
        let (ctx, temp) = test_ctx();
        let missing_target = temp.path().join("missing-clash.yaml");
        let path = ctx.clash_config_path();
        symlink(&missing_target, &path).unwrap();

        let error = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(error.to_string().contains("failed to read"), "{error:#}");
        assert_eq!(std::fs::read_link(&path).unwrap(), missing_target);
        assert!(!missing_target.exists());
        assert!(!ctx.application_config_path().exists());
        assert!(!ctx.session_state_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_legacy_verge_link_cannot_be_replaced_with_typed_defaults() {
        use std::os::unix::fs::symlink;
        let (mut ctx, temp) = test_ctx();
        let target = temp.path().join("not-found-verge.yaml");
        let path = ctx.chimera_config_path();
        symlink(&target, &path).unwrap();

        let error = SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap_err();
        assert!(
            format!("{error:#}").contains("failed to read legacy config"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!ctx.application_config_path().exists());
        assert!(!ctx.session_state_path().exists());
        assert!(!ctx.clash_config_path().exists());
    }

    #[cfg(unix)]
    #[test]
    fn dangling_legacy_clash_override_link_cannot_be_skipped() {
        use std::os::unix::fs::symlink;
        let (mut ctx, temp) = test_ctx();
        let target = temp.path().join("not-found-overrides.yaml");
        let path = ctx.clash_guard_overrides_path();
        symlink(&target, &path).unwrap();

        let error = SPLIT_LEGACY_CONFIG.run(&mut ctx).unwrap_err();
        assert!(
            format!("{error:#}").contains("failed to read legacy clash overrides"),
            "{error:#}"
        );
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!ctx.application_config_path().exists());
        assert!(!ctx.session_state_path().exists());
        assert!(!ctx.clash_config_path().exists());
    }

    #[test]
    fn application_only_partial_state_fails_closed() {
        let (ctx, _temp) = test_ctx();
        write_yaml(&ctx.application_config_path(), &ChimeraAppConfig::default());

        let err = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("partial typed config migration state"),
            "{err:#}"
        );
        assert!(err.to_string().contains("existing [application.yaml]"));
    }

    #[test]
    fn typed_clash_only_fails_closed_without_overwrite() {
        let (ctx, _temp) = test_ctx();
        let typed = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        write_yaml(&ctx.clash_config_path(), &typed);
        let before = std::fs::read(&ctx.clash_config_path()).unwrap();

        let err = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("partial typed config migration state"),
            "{err:#}"
        );
        assert_eq!(std::fs::read(&ctx.clash_config_path()).unwrap(), before);
    }

    #[test]
    fn invalid_existing_partial_domain_fails_closed() {
        let (ctx, _temp) = test_ctx();
        std::fs::write(ctx.application_config_path(), "not: [valid").unwrap();

        let err = MIGRATOR.detect_baseline(&ctx).unwrap_err();
        assert!(
            err.to_string()
                .contains("partial typed config migration state"),
            "{err:#}"
        );
    }
}
