use once_cell::sync::Lazy;
use semver::Version;
use serde_yaml::{Mapping, Value};

use super::super::{Ctx, MigrationStep, ModuleMigrator};

pub static MIGRATOR: ProfilesMigrator = ProfilesMigrator;

static VERSION_0_23_2: Lazy<Version> = Lazy::new(|| Version::parse("0.23.2").unwrap());
static NULL_VALUE: MigrateProfilesNullValue = MigrateProfilesNullValue;
static SCRIPT_NEWTYPE: MigrateProfileScriptNewtype = MigrateProfileScriptNewtype;
static STEPS: [&dyn MigrationStep; 2] = [&NULL_VALUE, &SCRIPT_NEWTYPE];

pub struct ProfilesMigrator;

impl ModuleMigrator for ProfilesMigrator {
    fn module(&self) -> &'static str {
        "profiles"
    }

    fn detect_baseline(&self, ctx: &Ctx) -> anyhow::Result<u64> {
        let path = ctx.profiles_path();
        if !path.exists() {
            return Ok(current_revision());
        }

        let raw = std::fs::read_to_string(path)?;
        let doc: Mapping = serde_yaml::from_str(&raw)
            .map_err(|error| anyhow::anyhow!("failed to parse profiles: {error}"))?;
        if has_legacy_nulls(&doc) || has_legacy_script_tag(&doc) {
            Ok(0)
        } else {
            Ok(current_revision())
        }
    }

    fn steps(&self) -> &'static [&'static dyn MigrationStep] {
        &STEPS
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfilesNullValue;

impl MigrationStep for MigrateProfilesNullValue {
    fn id(&self) -> &'static str {
        "profiles/null_value"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        1
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_23_2
    }

    fn name(&self) -> &'static str {
        "MigrateProfilesNullValue"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        update_profiles(ctx, |doc| {
            for key in ["current", "chain"] {
                if doc.get(key).is_some_and(Value::is_null) {
                    doc.insert(key.into(), Value::Sequence(Vec::new()));
                }
            }
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateProfileScriptNewtype;

impl MigrationStep for MigrateProfileScriptNewtype {
    fn id(&self) -> &'static str {
        "profiles/script_newtype"
    }

    fn module(&self) -> &'static str {
        "profiles"
    }

    fn revision(&self) -> u64 {
        2
    }

    fn introduced_in(&self) -> &'static Version {
        &VERSION_0_23_2
    }

    fn name(&self) -> &'static str {
        "MigrateProfileScriptNewtype"
    }

    fn run(&self, ctx: &mut Ctx) -> anyhow::Result<()> {
        update_profiles(ctx, migrate_script_tags)
    }
}

fn has_legacy_nulls(doc: &Mapping) -> bool {
    ["current", "chain"]
        .into_iter()
        .any(|key| doc.get(key).is_some_and(Value::is_null))
}

fn has_legacy_script_tag(doc: &Mapping) -> bool {
    doc.get("items")
        .and_then(Value::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(Value::as_mapping)
        .filter_map(|item| item.get("type"))
        .any(|value| {
            matches!(value, Value::Tagged(tag) if tag.tag == "script" && tag.value.as_str().is_some())
        })
}

fn migrate_script_tags(doc: &mut Mapping) {
    let Some(items) = doc.get_mut("items").and_then(Value::as_sequence_mut) else {
        return;
    };

    for item in items.iter_mut().filter_map(Value::as_mapping_mut) {
        let Some(Value::Tagged(tag)) = item.get("type").cloned() else {
            continue;
        };
        if tag.tag != "script" {
            continue;
        }
        let Some(script_type) = tag.value.as_str() else {
            continue;
        };
        item.insert("type".into(), Value::String("script".into()));
        item.insert("script_type".into(), Value::String(script_type.into()));
    }
}

fn update_profiles(ctx: &Ctx, update: impl FnOnce(&mut Mapping)) -> anyhow::Result<()> {
    let path = ctx.profiles_path();
    if !path.exists() {
        return Ok(());
    }

    let raw = std::fs::read_to_string(&path)?;
    let mut doc: Mapping = serde_yaml::from_str(&raw)
        .map_err(|error| anyhow::anyhow!("failed to parse profiles: {error}"))?;
    update(&mut doc);
    let body = serde_yaml::to_string(&doc)?;
    let content = format!("# Profiles Config for Clash Chimera\n{body}");
    crate::core::migration::fs::atomic_write(&path, content.as_bytes())
}

fn current_revision() -> u64 {
    STEPS.last().map(|step| step.revision()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::profile::{item::Profile, item_type::ScriptType, profiles::Profiles};

    const LEGACY_PROFILES: &str = r#"current: null
chain: null
valid:
  - dns
  - unified-delay
  - tun
items:
  - uid: rIWXPHuafvEM
    type: remote
    name: Test Remote
    file: rIWXPHuafvEM.yaml
    desc: null
    updated: 1758110672
    url: https://example.com
  - uid: siL1cvjnvLB6
    type: !script javascript
    name: Script Chain
    file: siL1cvjnvLB6.js
    desc: ''
    updated: 1720954186
"#;

    fn ctx() -> (Ctx, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let ctx = Ctx::new(dir.path().to_path_buf(), dir.path().join("data"));
        (ctx, dir)
    }

    #[test]
    fn legacy_profile_shape_is_detected_and_migrates_to_current_model() {
        let (mut ctx, _dir) = ctx();
        std::fs::write(ctx.profiles_path(), LEGACY_PROFILES).unwrap();

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), 0);
        NULL_VALUE.run(&mut ctx).unwrap();
        SCRIPT_NEWTYPE.run(&mut ctx).unwrap();

        let raw = std::fs::read_to_string(ctx.profiles_path()).unwrap();
        let profiles: Profiles = serde_yaml::from_str(&raw).unwrap();
        assert!(profiles.current.is_empty());
        assert!(profiles.chain.is_empty());
        assert_eq!(profiles.items.len(), 2);
        assert!(matches!(
            &profiles.items[1],
            Profile::Script(script) if script.script_type == ScriptType::JavaScript
        ));
    }

    #[test]
    fn current_profile_shape_does_not_request_migration() {
        let (ctx, _dir) = ctx();
        let current = Profiles::default();
        std::fs::write(
            ctx.profiles_path(),
            serde_yaml::to_string(&current).unwrap(),
        )
        .unwrap();

        assert_eq!(MIGRATOR.detect_baseline(&ctx).unwrap(), current_revision());
    }
}
