mod manageable;
mod tun;

pub use manageable::ManageableField;
pub use tun::{TunOverrides, TunOverridesPatch};

use serde::{Deserialize, Serialize};
use struct_patch::Patch;

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum LogLevel {
    Silent,
    Error,
    Warning,
    #[default]
    Info,
    Debug,
}

#[derive(
    Default,
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    strum::EnumString,
    strum::Display,
    specta::Type,
)]
#[repr(u8)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Rule,
    Global,
    Direct,
    Script,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    specta::Type,
    Patch,
    chimera_macro::OwnedFields,
)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
#[patch(attribute(serde(default, rename_all = "kebab-case")))]
#[serde(rename_all = "kebab-case")]
pub struct ClashGuardOverrides {
    log_level: LogLevel,
    allow_lan: bool,
    mode: Mode,
    secret: String,
    #[cfg(feature = "default-meta")]
    unified_delay: ManageableField<bool>,
    #[cfg(feature = "default-meta")]
    tcp_concurrent: ManageableField<bool>,
    ipv6: bool,
}

impl Default for ClashGuardOverrides {
    fn default() -> Self {
        Self {
            log_level: LogLevel::Info,
            allow_lan: false,
            mode: Mode::Rule,
            secret: uuid::Uuid::new_v4().to_string().to_lowercase(),
            #[cfg(feature = "default-meta")]
            unified_delay: ManageableField::Managed(true),
            #[cfg(feature = "default-meta")]
            tcp_concurrent: ManageableField::Managed(true),
            ipv6: false,
        }
    }
}

impl ClashGuardOverrides {
    pub fn secret(&self) -> &str {
        &self.secret
    }

    pub fn log_level(&self) -> LogLevel {
        self.log_level
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }
}

#[cfg(test)]
mod patch_tests {
    use super::*;
    use struct_patch::Patch;

    /// The patch shares the config's `kebab-case` wire shape: it decodes
    /// `allow-lan`/`log-level` and applies only the fields present.
    #[test]
    fn patch_uses_kebab_case_and_applies() {
        let patch: ClashGuardOverridesPatch =
            serde_yaml_ng::from_str("allow-lan: true\nlog-level: debug\n")
                .expect("kebab-case patch must deserialize");

        let mut overrides = ClashGuardOverrides::default();
        let kept_secret = overrides.secret.clone();
        overrides.apply(patch);

        assert!(overrides.allow_lan);
        assert_eq!(overrides.log_level, LogLevel::Debug);
        assert_eq!(overrides.mode, Mode::Rule, "absent field unchanged");
        assert_eq!(overrides.secret, kept_secret, "absent secret unchanged");
    }

    #[cfg(feature = "default-meta")]
    #[test]
    fn legacy_boolean_guard_fields_deserialize_as_managed() {
        let overrides: ClashGuardOverrides = serde_yaml_ng::from_str(
            "log-level: info\nallow-lan: false\nmode: rule\nsecret: legacy\nunified-delay: false\ntcp-concurrent: true\nipv6: false\n",
        )
        .expect("legacy boolean guard fields must remain readable");

        assert_eq!(overrides.unified_delay, ManageableField::Managed(false));
        assert_eq!(overrides.tcp_concurrent, ManageableField::Managed(true));

        let serialized = serde_json::to_value(&overrides).unwrap();
        assert_eq!(
            serialized["unified-delay"],
            serde_json::json!({ "kind": "managed", "value": false })
        );
    }

    #[cfg(feature = "default-meta")]
    #[test]
    fn legacy_boolean_guard_patches_remain_readable() {
        let mut overrides = ClashGuardOverrides::default();
        let patch: ClashGuardOverridesPatch =
            serde_yaml_ng::from_str("unified-delay: false\ntcp-concurrent: true\n")
                .expect("legacy boolean guard patch must remain readable");
        overrides.apply(patch);

        assert_eq!(overrides.unified_delay, ManageableField::Managed(false));
        assert_eq!(overrides.tcp_concurrent, ManageableField::Managed(true));
    }

    #[cfg(feature = "default-meta")]
    #[test]
    fn ownership_entries_distinguish_managed_and_unmanaged_fields() {
        use crate::runtime::post_process::{OwnedFields, Ownership};

        let mut overrides = ClashGuardOverrides::default();
        let patch: ClashGuardOverridesPatch = serde_yaml_ng::from_str(
            "unified-delay:\n  kind: unmanaged\ntcp-concurrent:\n  kind: managed\n  value: false\n",
        )
        .expect("managed-field patch must deserialize");
        overrides.apply(patch);

        let entries = overrides.ownership_entries();
        assert!(entries.contains(&("unified-delay", Ownership::Inherited)));
        assert!(entries.contains(&(
            "tcp-concurrent",
            Ownership::Forced(crate::runtime::value::ConfigValue::from_json_value(
                serde_json::json!(false)
            ))
        )));
    }

    #[test]
    fn mode_accessor_returns_the_typed_override() {
        let mut patch = ClashGuardOverrides::new_empty_patch();
        patch.mode = Some(Mode::Script);
        let mut overrides = ClashGuardOverrides::default();

        overrides.apply(patch);

        assert_eq!(overrides.mode(), Mode::Script);
    }

    /// A serialized patch keeps the `kebab-case` keys and skips absent fields.
    #[test]
    fn patch_serializes_kebab_case_and_skips_none() {
        let mut patch = ClashGuardOverrides::new_empty_patch();
        patch.allow_lan = Some(true);

        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("allow-lan: true"), "got:\n{dumped}");
        assert!(
            !dumped.contains("log-level"),
            "absent skipped, got:\n{dumped}"
        );
    }
}
