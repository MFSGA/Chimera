use chimera_macro::BuilderUpdate;
use derive_builder::Builder;
use serde::{Deserialize, Serialize};

use crate::config::profile::{item::ProfileMetaGetter, item_type::ProfileItemType};

pub(crate) const MAX_PROFILE_UID_BYTES: usize = 128;

pub(crate) fn profile_timestamp_from_unix_seconds(timestamp: i64) -> usize {
    usize::try_from(timestamp).unwrap_or(0)
}

pub(crate) fn current_profile_timestamp() -> usize {
    profile_timestamp_from_unix_seconds(chrono::Local::now().timestamp())
}

pub(crate) fn validate_profile_uid(uid: &str) -> anyhow::Result<()> {
    if uid.trim().is_empty() {
        anyhow::bail!("profile identifier must not be empty");
    }
    if uid.trim() != uid {
        anyhow::bail!("profile identifier must not contain leading or trailing whitespace");
    }
    if uid.chars().any(char::is_control) {
        anyhow::bail!("profile identifier must not contain control characters");
    }
    if uid.len() > MAX_PROFILE_UID_BYTES {
        anyhow::bail!(
            "profile identifier exceeds the maximum size of {MAX_PROFILE_UID_BYTES} bytes"
        );
    }
    Ok(())
}

#[derive(
    Default,
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    Builder,
    BuilderUpdate,
    specta::Type,
)]
#[builder(
    derive(Debug, serde::Serialize, serde::Deserialize, specta::Type),
    build_fn(skip)
)]
#[builder_update(patch_fn = "apply", getter)]
pub struct ProfileShared {
    /// Profile ID
    pub uid: String,
    /// profile name
    pub name: String,
    /// profile holds the file
    // #[serde(alias = "file", deserialize_with = "deserialize_option_single_or_vec")]
    #[builder(default = "self.default_files()?")]
    pub file: String,
    /// profile description
    #[builder(default, setter(strip_option))]
    pub desc: Option<String>,
    #[builder(default = "current_profile_timestamp()")]
    /// update time
    pub updated: usize,
}

impl ProfileShared {
    pub fn get_default_builder(kind: &ProfileItemType) -> ProfileSharedBuilder {
        let mut builder = ProfileSharedBuilder::default();
        builder
            .name(ProfileSharedBuilder::default_name(kind).to_string())
            .uid(ProfileSharedBuilder::default_uid(kind));
        builder
    }
}

impl ProfileSharedBuilder {
    fn default_uid(kind: &ProfileItemType) -> String {
        super::utils::generate_uid(kind)
    }

    pub fn default_name(kind: &ProfileItemType) -> &'static str {
        match kind {
            ProfileItemType::Remote => "Remote Profile",
            ProfileItemType::Local => "Local Profile",
            // ProfileItemType::Merge => "Merge Profile",
            // ProfileItemType::Script(_) => "Script Profile",
        }
    }

    pub fn default_file_name(kind: &ProfileItemType, uid: &str) -> String {
        match kind {
            ProfileItemType::Remote => format!("{uid}.yaml"),
            ProfileItemType::Local => format!("{uid}.yaml"),
            // ProfileItemType::Merge => format!("{uid}.yaml"),
            // ProfileItemType::Script(ScriptType::JavaScript) => format!("{uid}.js"),
            // ProfileItemType::Script(ScriptType::Lua) => format!("{uid}.lua"),
        }
    }

    pub fn build(
        &self,
        kind: &ProfileItemType,
    ) -> Result<ProfileShared, ProfileSharedBuilderError> {
        let uid = self.uid.clone().unwrap_or_else(|| Self::default_uid(kind));
        validate_profile_uid(&uid)
            .map_err(|error| ProfileSharedBuilderError::from(error.to_string()))?;
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| Self::default_name(kind).to_string());
        let file = self
            .file
            .clone()
            .unwrap_or_else(|| Self::default_file_name(kind, &uid));

        Ok(ProfileShared {
            uid,
            name,
            file,
            desc: self.desc.clone().unwrap_or_default(),
            updated: self.updated.unwrap_or_else(current_profile_timestamp),
        })
    }
}

impl ProfileMetaGetter for ProfileShared {
    fn uid(&self) -> &str {
        &self.uid
    }
}

impl super::ProfileMetaSetter for ProfileShared {
    fn set_uid(&mut self, uid: String) {
        self.uid = uid;
    }

    fn set_name(&mut self, name: String) {
        self.name = name;
    }

    fn set_desc(&mut self, desc: Option<String>) {
        self.desc = desc;
    }

    fn set_file(&mut self, file: String) {
        self.file = file;
    }

    fn set_updated(&mut self, updated: usize) {
        self.updated = updated;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_PROFILE_UID_BYTES, ProfileSharedBuilder, profile_timestamp_from_unix_seconds,
        validate_profile_uid,
    };
    use crate::config::profile::item_type::ProfileItemType;

    #[test]
    fn profile_timestamp_conversion_rejects_pre_epoch_and_platform_overflow() {
        assert_eq!(profile_timestamp_from_unix_seconds(-1), 0);
        assert_eq!(profile_timestamp_from_unix_seconds(0), 0);
        assert_eq!(profile_timestamp_from_unix_seconds(123), 123);
        assert_eq!(
            profile_timestamp_from_unix_seconds(i64::MAX),
            usize::try_from(i64::MAX).unwrap_or(0)
        );
    }

    #[test]
    fn shared_builder_generates_consistent_defaults_without_panicking() {
        let shared = ProfileSharedBuilder::default()
            .build(&ProfileItemType::Local)
            .expect("default shared profile fields must build");

        assert!(!shared.uid.is_empty());
        assert_eq!(shared.name, "Local Profile");
        assert_eq!(shared.file, format!("{}.yaml", shared.uid));
        assert!(shared.desc.is_none());
    }

    #[test]
    fn profile_uid_validation_accepts_unicode_and_rejects_ambiguous_or_control_values() {
        for valid in ["profile-a", "订阅-東京", "профиль_1"] {
            validate_profile_uid(valid).expect("ordinary Unicode profile identifiers must work");
        }

        for invalid in [
            "",
            "   ",
            " profile-a",
            "profile-a ",
            "profile\nnext",
            "profile\rnext",
            "profile\tnext",
            "profile\0next",
        ] {
            validate_profile_uid(invalid)
                .expect_err("empty, padded, and control-character identifiers must be rejected");
        }
    }

    #[test]
    fn profile_uid_validation_enforces_exact_utf8_byte_boundaries() {
        let exact_ascii = "a".repeat(MAX_PROFILE_UID_BYTES);
        validate_profile_uid(&exact_ascii).expect("the exact ASCII UID limit must be accepted");
        validate_profile_uid(&format!("{exact_ascii}a"))
            .expect_err("an ASCII UID beyond the byte limit must be rejected");

        let exact_multibyte = "界".repeat(MAX_PROFILE_UID_BYTES / 3);
        assert!(exact_multibyte.len() <= MAX_PROFILE_UID_BYTES);
        validate_profile_uid(&exact_multibyte)
            .expect("a multibyte UID within the UTF-8 byte limit must be accepted");
        validate_profile_uid(&format!("{exact_multibyte}界"))
            .expect_err("a multibyte UID beyond the UTF-8 byte limit must be rejected");
    }

    #[test]
    fn shared_builder_rejects_invalid_custom_uids() {
        for uid in [" profile-a", "profile\nnext", "profile\0next"] {
            let mut builder = ProfileSharedBuilder::default();
            builder.uid(uid.to_string());
            builder
                .build(&ProfileItemType::Remote)
                .expect_err("the shared builder must enforce profile identifier validation");
        }
    }

    #[test]
    fn shared_builder_uses_custom_uid_for_default_file_name() {
        let mut builder = ProfileSharedBuilder::default();
        builder.uid("custom-profile".to_string());

        let shared = builder
            .build(&ProfileItemType::Remote)
            .expect("custom shared profile UID must build");

        assert_eq!(shared.uid, "custom-profile");
        assert_eq!(shared.name, "Remote Profile");
        assert_eq!(shared.file, "custom-profile.yaml");
    }

    #[test]
    fn shared_builder_preserves_explicit_fields() {
        let mut builder = ProfileSharedBuilder::default();
        builder
            .uid("profile-a".to_string())
            .name("Custom".to_string())
            .file("custom.yaml".to_string())
            .desc("Description".to_string())
            .updated(42);

        let shared = builder
            .build(&ProfileItemType::Local)
            .expect("explicit shared profile fields must build");

        assert_eq!(shared.uid, "profile-a");
        assert_eq!(shared.name, "Custom");
        assert_eq!(shared.file, "custom.yaml");
        assert_eq!(shared.desc.as_deref(), Some("Description"));
        assert_eq!(shared.updated, 42);
    }
}
