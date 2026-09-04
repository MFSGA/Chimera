use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use chimera_config::clash::config::ClashConfig;
use serde::de::DeserializeOwned;
use serde_yaml::{Mapping, Value};

use crate::{
    bridge::clash::clash_config_from_legacy,
    config::{chimera::IVerge, clash::IClashTemp, core::Config},
    utils::{dirs, help},
};

const SHARED_CLASH_FILE: &str = "clash-config.yaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SharedClashFileState {
    Missing,
    Typed,
    LegacyRuntime,
    Unrecognized,
}

pub(crate) fn repair_shared_clash_config_path() -> anyhow::Result<()> {
    let shared_path = dirs::app_config_dir()?.join(SHARED_CLASH_FILE);
    let guard_path = dirs::clash_guard_overrides_path()?;
    let legacy_verge = Config::verge().latest().clone();

    if repair_at(&shared_path, &guard_path, &legacy_verge)? {
        log::info!(
            target: "app",
            "migrated legacy runtime clash-config.yaml to typed ClashConfig"
        );
    }

    Ok(())
}

fn repair_at(shared_path: &Path, guard_path: &Path, legacy_verge: &IVerge) -> anyhow::Result<bool> {
    match classify_shared_clash_file(shared_path)? {
        SharedClashFileState::Missing | SharedClashFileState::Typed => Ok(false),
        SharedClashFileState::LegacyRuntime => {
            let mut merged = IClashTemp::template().0;
            merge_legacy_mapping(&mut merged, shared_path)?;
            merge_legacy_mapping(&mut merged, guard_path)?;

            let typed = clash_config_from_legacy(legacy_verge, &merged)
                .context("failed to project legacy clash config into typed state")?;
            help::save_yaml(shared_path, &typed, Some("# Migrated by Clash Chimera"))
                .context("failed to write migrated typed clash config")?;
            Ok(true)
        }
        SharedClashFileState::Unrecognized => bail!(
            "existing {} is neither a valid typed ClashConfig nor a recognized legacy runtime clash config",
            shared_path.display()
        ),
    }
}

fn classify_shared_clash_file(path: &Path) -> anyhow::Result<SharedClashFileState> {
    if !path
        .try_exists()
        .with_context(|| format!("failed to inspect {}", path.display()))?
    {
        return Ok(SharedClashFileState::Missing);
    }

    if read_yaml::<ClashConfig>(path).is_ok() {
        return Ok(SharedClashFileState::Typed);
    }

    let raw = fs_err::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let value: Value = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;

    if looks_like_legacy_runtime_clash_mapping(&value) {
        Ok(SharedClashFileState::LegacyRuntime)
    } else {
        Ok(SharedClashFileState::Unrecognized)
    }
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
    .any(|key| map.contains_key(Value::String((*key).to_owned())))
}

fn merge_legacy_mapping(merged: &mut Mapping, path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let legacy = help::read_merge_mapping(&PathBuf::from(path))
        .with_context(|| format!("failed to read legacy clash mapping {}", path.display()))?;
    for (key, value) in legacy {
        if !matches!(value, Value::Null) {
            merged.insert(key, value);
        }
    }
    Ok(())
}

fn read_yaml<T: DeserializeOwned>(path: &Path) -> anyhow::Result<T> {
    let raw = fs_err::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_yaml<T: serde::Serialize>(path: &Path, value: &T) {
        fs_err::write(path, serde_yaml::to_string(value).unwrap()).unwrap();
    }

    fn write_legacy_runtime(path: &Path) {
        let mut legacy = IClashTemp::template().0;
        legacy.insert("mixed-port".into(), 8123.into());
        legacy.insert("external-controller".into(), "127.0.0.1:19090".into());
        legacy.insert("allow-lan".into(), false.into());
        legacy.insert("mode".into(), "rule".into());
        legacy.insert("ipv6".into(), false.into());
        write_yaml(path, &legacy);
    }

    #[test]
    fn legacy_runtime_file_is_classified_and_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join(SHARED_CLASH_FILE);
        let guard = dir.path().join("clash-guard-overrides.yaml");
        write_legacy_runtime(&shared);

        let mut guard_mapping = Mapping::new();
        guard_mapping.insert("allow-lan".into(), true.into());
        guard_mapping.insert("mode".into(), "global".into());
        guard_mapping.insert("ipv6".into(), true.into());
        write_yaml(&guard, &guard_mapping);

        assert_eq!(
            classify_shared_clash_file(&shared).unwrap(),
            SharedClashFileState::LegacyRuntime
        );
        assert!(repair_at(&shared, &guard, &IVerge::template()).unwrap());
        assert_eq!(
            classify_shared_clash_file(&shared).unwrap(),
            SharedClashFileState::Typed
        );

        let typed: ClashConfig = read_yaml(&shared).unwrap();
        let overrides = serde_yaml::to_value(&typed.overrides).unwrap();
        let overrides = overrides.as_mapping().unwrap();
        assert_eq!(overrides.get("allow-lan"), Some(&Value::Bool(true)));
        assert_eq!(overrides.get("mode"), Some(&Value::String("global".into())));
        assert_eq!(overrides.get("ipv6"), Some(&Value::Bool(true)));
        assert_eq!(typed.mixed_port.start_port, 8123);
    }

    #[test]
    fn existing_typed_file_is_left_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join(SHARED_CLASH_FILE);
        let guard = dir.path().join("clash-guard-overrides.yaml");
        let typed = ClashConfig::default();
        write_yaml(&shared, &typed);
        let before = fs_err::read(&shared).unwrap();

        assert!(!repair_at(&shared, &guard, &IVerge::template()).unwrap());
        assert_eq!(fs_err::read(&shared).unwrap(), before);
    }

    #[test]
    fn unrecognized_shared_file_fails_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join(SHARED_CLASH_FILE);
        let guard = dir.path().join("clash-guard-overrides.yaml");
        fs_err::write(&shared, "unexpected: true\n").unwrap();

        let error = repair_at(&shared, &guard, &IVerge::template()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("neither a valid typed ClashConfig")
        );
        assert_eq!(
            fs_err::read_to_string(&shared).unwrap(),
            "unexpected: true\n"
        );
    }
}
