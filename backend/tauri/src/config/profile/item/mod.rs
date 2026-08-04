use std::{
    borrow::Borrow,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use ambassador::{Delegate, delegatable_trait};
use anyhow::{Context, Result, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use chimera_macro::EnumWrapperCombined;
use serde_yaml::{Mapping, Value};

use crate::{
    config::profile::{
        item::{local::LocalProfile, remote::RemoteProfile},
        item_type::ProfileItemType,
    },
    utils::dirs,
};

/// 0
pub mod local;
/// 1
pub mod remote;
/// 2
pub mod shared;
/// 3
pub mod utils;

pub const MAX_PROFILE_YAML_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_PROFILE_FILE_NAME_BYTES: usize = 255;

pub(crate) fn validate_profile_text_size(content: &str, max_bytes: usize) -> Result<()> {
    if content.len() > max_bytes {
        bail!("profile YAML exceeds the maximum size of {max_bytes} bytes");
    }
    Ok(())
}

/// Some getter is provided due to `Profile` is a enum type, and could not be used directly.
/// If access to inner data is needed, you should use the `as_xxx` or `as_mut_xxx` method to get the inner specific profile item.
#[delegatable_trait]
pub trait ProfileMetaGetter {
    fn uid(&self) -> &str;
}

#[derive(
    serde::Deserialize, serde::Serialize, Debug, Clone, EnumWrapperCombined, specta::Type, Delegate,
)]
#[delegate(ProfileMetaGetter)]
#[delegate(ProfileKindGetter)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Profile {
    Remote(RemoteProfile),
    Local(LocalProfile),
}

fn profile_file_path_in(directory: &Path, file: impl AsRef<Path>) -> Result<PathBuf> {
    let file = file.as_ref();
    let mut components = file.components();
    let Some(Component::Normal(file_name)) = components.next() else {
        bail!("profile file must be a relative file name");
    };
    if components.next().is_some() {
        bail!("profile file must not contain directory components");
    }
    validate_profile_file_name(file_name)?;

    Ok(directory.join(file_name))
}

fn validate_profile_file_name(file_name: &std::ffi::OsStr) -> Result<()> {
    let Some(file_name) = file_name.to_str() else {
        bail!("profile file name must be valid Unicode");
    };
    if file_name.len() > MAX_PROFILE_FILE_NAME_BYTES {
        bail!("profile file name exceeds the maximum size of {MAX_PROFILE_FILE_NAME_BYTES} bytes");
    }

    #[cfg(target_os = "windows")]
    {
        if file_name.contains(':') {
            bail!("profile file must not use a Windows alternate data stream");
        }
        if file_name.chars().any(|character| {
            matches!(character, '<' | '>' | '"' | '|' | '?' | '*') || character.is_control()
        }) {
            bail!("profile file contains characters that are invalid on Windows");
        }
        if file_name.ends_with('.') || file_name.ends_with(' ') {
            bail!("profile file must not end with a dot or space on Windows");
        }

        let device_name = file_name
            .split('.')
            .next()
            .unwrap_or_default()
            .trim_end_matches([' ', '.'])
            .to_ascii_uppercase();
        let is_reserved_numbered_device = |prefix: &str| {
            matches!(
                device_name.strip_prefix(prefix),
                Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            )
        };
        let reserved = matches!(device_name.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || is_reserved_numbered_device("COM")
            || is_reserved_numbered_device("LPT");
        if reserved {
            bail!("profile file must not use a reserved Windows device name");
        }
    }

    Ok(())
}

fn validate_canonical_profile_target_in(directory: &Path, target: &Path) -> Result<()> {
    if !target.starts_with(directory) {
        bail!("profile file target escapes the profiles directory");
    }
    Ok(())
}

fn ensure_existing_profile_target_in(directory: &Path, target: &Path) -> Result<()> {
    match std::fs::symlink_metadata(target) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect profile file {}", target.display()));
        }
    }

    let canonical_directory = directory.canonicalize().with_context(|| {
        format!(
            "failed to resolve profile directory {}",
            directory.display()
        )
    })?;
    let canonical_target = target
        .canonicalize()
        .with_context(|| format!("failed to resolve profile file {}", target.display()))?;
    validate_canonical_profile_target_in(&canonical_directory, &canonical_target)?;
    if !canonical_target.is_file() {
        bail!("profile file target must be a regular file");
    }
    Ok(())
}

fn profile_file_path_checked_in(directory: &Path, file: impl AsRef<Path>) -> Result<PathBuf> {
    let path = profile_file_path_in(directory, file)?;
    ensure_existing_profile_target_in(directory, &path)?;
    Ok(path)
}

pub(crate) fn profile_materialized_target_in(
    directory: &Path,
    file: impl AsRef<Path>,
) -> Result<PathBuf> {
    let path = profile_file_path_checked_in(directory, file)?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => path
            .canonicalize()
            .with_context(|| format!("failed to resolve profile file {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(error) => {
            Err(error).with_context(|| format!("failed to inspect profile file {}", path.display()))
        }
    }
}

pub(crate) fn profile_file_path(file: impl AsRef<Path>) -> Result<PathBuf> {
    let directory = dirs::app_profiles_dir()?;
    profile_file_path_checked_in(&directory, file)
}

pub(crate) fn profile_materialized_path(file: impl AsRef<Path>) -> Result<PathBuf> {
    profile_materialized_target_in(&dirs::app_profiles_dir()?, file)
}

fn profile_cleanup_path_in(directory: &Path, file: impl AsRef<Path>) -> Option<PathBuf> {
    profile_file_path_in(directory, file).ok()
}

pub(crate) fn profile_cleanup_path(file: impl AsRef<Path>) -> Result<Option<PathBuf>> {
    Ok(profile_cleanup_path_in(&dirs::app_profiles_dir()?, file))
}

pub(crate) fn read_file_bytes_with_limit(path: &Path, max_bytes: usize) -> Result<Vec<u8>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open file {}", path.display()))?;
    let declared_size = file
        .metadata()
        .with_context(|| format!("failed to inspect file {}", path.display()))?
        .len();
    if declared_size > max_bytes as u64 {
        bail!("file exceeds the maximum size of {max_bytes} bytes");
    }

    let mut bytes = Vec::with_capacity(declared_size as usize);
    file.take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read file {}", path.display()))?;
    if bytes.len() > max_bytes {
        bail!("file exceeds the maximum size of {max_bytes} bytes");
    }
    Ok(bytes)
}

fn read_profile_text_with_limit(path: &Path, max_bytes: usize) -> Result<String> {
    let bytes = read_file_bytes_with_limit(path, max_bytes)
        .map_err(|error| anyhow::anyhow!("profile YAML {error}"))?;
    String::from_utf8(bytes)
        .with_context(|| format!("failed to read profile file {} as UTF-8", path.display()))
}

pub(crate) fn write_profile_bytes_atomic(path: &Path, content: &[u8]) -> Result<()> {
    if content.len() > MAX_PROFILE_YAML_BYTES {
        bail!(
            "profile YAML exceeds the maximum size of {} bytes",
            MAX_PROFILE_YAML_BYTES
        );
    }
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(content))
        .with_context(|| format!("failed to atomically save profile file {}", path.display()))
}

pub(crate) fn write_profile_text_atomic(path: &Path, content: &str) -> Result<()> {
    validate_profile_text_size(content, MAX_PROFILE_YAML_BYTES)?;
    write_profile_bytes_atomic(path, content.as_bytes())
}

pub(crate) fn validate_profile_mapping_keys(mapping: &Mapping) -> Result<()> {
    let mut normalized = std::collections::HashSet::with_capacity(mapping.len());
    for key in mapping.keys() {
        let key = key
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("profile YAML top-level keys must be strings"))?;
        if key.trim().is_empty() {
            bail!("profile YAML top-level keys must not be empty");
        }
        if key.trim() != key {
            bail!("profile YAML top-level keys must not have surrounding whitespace: {key:?}");
        }
        let normalized_key = key.to_ascii_lowercase();
        if !normalized.insert(normalized_key) {
            bail!("duplicate profile YAML top-level key after normalization: {key}");
        }
    }
    Ok(())
}

fn parse_profile_mapping(path: &Path, raw: &str) -> Result<Mapping> {
    let mut value = serde_yaml::from_str::<Value>(raw)
        .with_context(|| format!("failed to parse profile YAML {}", path.display()))?;
    value
        .apply_merge()
        .with_context(|| format!("failed to apply profile YAML merge {}", path.display()))?;
    let mapping = value.as_mapping().cloned().ok_or_else(|| {
        anyhow::anyhow!("profile YAML root must be a mapping: {}", path.display())
    })?;
    validate_profile_mapping_keys(&mapping)
        .with_context(|| format!("invalid profile YAML keys in {}", path.display()))?;
    Ok(mapping)
}

impl Profile {
    pub fn file(&self) -> &str {
        match self {
            Profile::Remote(profile) => &profile.shared.file,
            Profile::Local(profile) => &profile.shared.file,
        }
    }

    /// get the file data
    pub fn read_file(&self) -> Result<String> {
        let path = profile_materialized_path(self.file())?;
        if !path.exists() {
            bail!("file does not exist");
        }
        read_profile_text_with_limit(&path, MAX_PROFILE_YAML_BYTES)
    }

    pub fn read_mapping(&self) -> Result<Mapping> {
        let path = profile_materialized_path(self.file())?;
        let raw = read_profile_text_with_limit(&path, MAX_PROFILE_YAML_BYTES)?;
        parse_profile_mapping(&path, &raw)
    }

    /// save the file data atomically
    pub fn save_file<T: Borrow<String>>(&self, data: T) -> Result<()> {
        let data = data.borrow();
        let path = profile_materialized_path(self.file())?;
        write_profile_text_atomic(&path, data)
    }
}

/// Profile Setter Helper
/// It is intended to be used in the default trait implementation, so it is PRIVATE.
/// NOTE: this just a setter for fields, NOT do any file operation.
#[delegatable_trait]
trait ProfileMetaSetter {
    fn set_uid(&mut self, uid: String);
    fn set_name(&mut self, name: String);
    fn set_desc(&mut self, desc: Option<String>);
    fn set_file(&mut self, file: String);
    fn set_updated(&mut self, updated: usize);
}

#[delegatable_trait]
pub trait ProfileKindGetter {
    fn kind(&self) -> ProfileItemType;
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use serde_yaml::Mapping;

    use super::{
        MAX_PROFILE_FILE_NAME_BYTES, parse_profile_mapping, profile_cleanup_path_in,
        profile_file_path_checked_in, profile_file_path_in, profile_materialized_target_in,
        read_profile_text_with_limit, validate_canonical_profile_target_in,
        validate_profile_mapping_keys, validate_profile_text_size, write_profile_bytes_atomic,
        write_profile_text_atomic,
    };

    #[test]
    fn atomic_profile_byte_write_preserves_exact_binary_content() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        let content = [0xff_u8, 0x00, 0x7f];

        write_profile_bytes_atomic(&path, &content)
            .expect("atomic profile byte write must succeed");

        assert_eq!(
            std::fs::read(&path).expect("read atomic profile byte fixture"),
            content
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn atomic_profile_write_through_internal_link_preserves_the_link() {
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().expect("temporary profile directory");
        let target = directory.path().join("target.yaml");
        let link = directory.path().join("profile.yaml");
        std::fs::write(&target, "old: true").expect("write profile target fixture");
        symlink_file(&target, &link).expect("create profile link fixture");

        let materialized = profile_materialized_target_in(directory.path(), "profile.yaml")
            .expect("resolve profile link target");
        write_profile_text_atomic(&materialized, "new: true")
            .expect("atomic profile target write must succeed");

        assert!(
            std::fs::symlink_metadata(&link)
                .expect("inspect preserved profile link")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("read updated profile target fixture"),
            "new: true"
        );
    }

    #[test]
    fn atomic_profile_write_replaces_existing_content() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, "old: true").expect("write existing profile fixture");

        write_profile_text_atomic(&path, "new: true")
            .expect("atomic profile replacement must succeed");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read replaced profile fixture"),
            "new: true"
        );
    }

    #[test]
    fn oversized_atomic_profile_write_preserves_existing_content() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, "old: true").expect("write existing profile fixture");
        let oversized = "x".repeat(super::MAX_PROFILE_YAML_BYTES + 1);

        write_profile_text_atomic(&path, &oversized)
            .expect_err("oversized atomic profile write must be rejected");

        assert_eq!(
            std::fs::read_to_string(&path).expect("read preserved profile fixture"),
            "old: true"
        );
    }

    #[test]
    fn failed_atomic_profile_write_cleans_temporary_files() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::create_dir(&path).expect("create colliding profile directory fixture");

        write_profile_text_atomic(&path, "new: true")
            .expect_err("atomic write over a directory must fail");

        let entries = std::fs::read_dir(directory.path())
            .expect("list atomic profile write fixture directory")
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("profile.yaml")]);
        assert!(path.is_dir());
    }

    #[test]
    fn profile_write_size_accepts_exactly_the_limit() {
        validate_profile_text_size("éé", 4)
            .expect("profile content exactly at the byte limit must be accepted");
    }

    #[test]
    fn profile_write_size_rejects_oversized_utf8_content() {
        let error = validate_profile_text_size("éé", 3)
            .expect_err("profile content size must be measured in UTF-8 bytes");

        assert!(error.to_string().contains("maximum size of 3 bytes"));
    }

    #[test]
    fn bounded_profile_read_accepts_exactly_the_limit() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, "abcd").expect("write exact-limit profile fixture");

        assert_eq!(
            read_profile_text_with_limit(&path, 4)
                .expect("profile exactly at the limit must be readable"),
            "abcd"
        );
    }

    #[test]
    fn bounded_profile_read_rejects_oversized_files() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, "abcde").expect("write oversized profile fixture");

        let error = read_profile_text_with_limit(&path, 4)
            .expect_err("oversized profile file must be rejected");

        assert!(error.to_string().contains("maximum size of 4 bytes"));
    }

    #[test]
    fn bounded_profile_read_counts_utf8_bytes() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, "éé").expect("write UTF-8 profile fixture");

        let error = read_profile_text_with_limit(&path, 3)
            .expect_err("UTF-8 profile size must be measured in bytes");

        assert!(error.to_string().contains("maximum size of 3 bytes"));
    }

    #[test]
    fn bounded_profile_read_rejects_invalid_utf8() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let path = directory.path().join("profile.yaml");
        std::fs::write(&path, [0xff_u8]).expect("write invalid UTF-8 profile fixture");

        let error = read_profile_text_with_limit(&path, 1)
            .expect_err("invalid UTF-8 profile file must be rejected");

        assert!(error.to_string().contains("failed to read profile file"));
    }

    #[test]
    fn profile_mapping_parser_preserves_yaml_merge_semantics() {
        let mapping = parse_profile_mapping(
            Path::new("profile.yaml"),
            "defaults: &defaults\n  mixed-port: 7890\n<<: *defaults\n",
        )
        .expect("valid YAML merge profile must be accepted");

        assert_eq!(
            mapping.get(serde_yaml::Value::String("mixed-port".into())),
            Some(&serde_yaml::Value::Number(7890.into()))
        );
    }

    #[test]
    fn profile_mapping_keys_reject_non_string_and_empty_keys() {
        for yaml in ["1: value\n", "\"\": value\n", "\"   \": value\n"] {
            let mapping: Mapping =
                serde_yaml::from_str(yaml).expect("valid mapping-key fixture YAML");
            let error = validate_profile_mapping_keys(&mapping)
                .expect_err("invalid top-level profile key must be rejected");
            assert!(error.to_string().contains("top-level keys"));
        }
    }

    #[test]
    fn profile_mapping_keys_reject_surrounding_whitespace() {
        let mapping: Mapping =
            serde_yaml::from_str("\" dns \": true\n").expect("valid spaced key fixture YAML");

        let error = validate_profile_mapping_keys(&mapping)
            .expect_err("surrounding whitespace in profile key must be rejected");

        assert!(error.to_string().contains("surrounding whitespace"));
    }

    #[test]
    fn profile_mapping_keys_reject_case_insensitive_duplicates() {
        let mapping: Mapping = serde_yaml::from_str("dns: {}\nDNS: {}\n")
            .expect("valid duplicate-normalization fixture YAML");

        let error = validate_profile_mapping_keys(&mapping)
            .expect_err("case-insensitive duplicate profile keys must be rejected");

        assert!(error.to_string().contains("after normalization"));
    }

    #[test]
    fn profile_mapping_keys_accept_nonempty_unicode_strings() {
        let mapping: Mapping =
            serde_yaml::from_str("代理: true\n").expect("valid Unicode mapping-key fixture YAML");

        validate_profile_mapping_keys(&mapping)
            .expect("nonempty Unicode top-level profile key must be accepted");
    }

    #[test]
    fn profile_mapping_parser_rejects_non_mapping_roots() {
        let error = parse_profile_mapping(Path::new("profile.yaml"), "- one\n- two\n")
            .expect_err("profile sequence root must be rejected");

        assert!(error.to_string().contains("root must be a mapping"));
    }

    #[test]
    fn profile_file_path_accepts_a_direct_child_file() {
        let directory = Path::new("profiles");
        let path = profile_file_path_in(directory, "local-profile.yaml")
            .expect("direct profile file name must be accepted");

        assert_eq!(path, directory.join("local-profile.yaml"));
    }

    #[test]
    fn profile_file_path_enforces_exact_utf8_component_boundaries() {
        let exact_ascii = "a".repeat(MAX_PROFILE_FILE_NAME_BYTES);
        profile_file_path_in(Path::new("profiles"), &exact_ascii)
            .expect("the exact ASCII file-name limit must be accepted");
        profile_file_path_in(Path::new("profiles"), format!("{exact_ascii}a"))
            .expect_err("an ASCII file name beyond the byte limit must be rejected");

        let exact_multibyte = "界".repeat(MAX_PROFILE_FILE_NAME_BYTES / 3);
        assert_eq!(exact_multibyte.len(), MAX_PROFILE_FILE_NAME_BYTES);
        profile_file_path_in(Path::new("profiles"), &exact_multibyte)
            .expect("the exact multibyte file-name limit must be accepted");
        profile_file_path_in(Path::new("profiles"), format!("{exact_multibyte}界"))
            .expect_err("a multibyte file name beyond the byte limit must be rejected");
    }

    #[test]
    fn profile_file_path_rejects_parent_and_nested_components() {
        for file in ["../outside.yaml", "nested/profile.yaml"] {
            let error = profile_file_path_in(Path::new("profiles"), file)
                .expect_err("profile path with directory components must be rejected");
            assert!(
                error.to_string().contains("directory components")
                    || error.to_string().contains("relative file name"),
                "unexpected error for {file}: {error:#}"
            );
        }
    }

    #[test]
    fn profile_file_path_rejects_absolute_paths() {
        let absolute = PathBuf::from(Path::new(std::path::MAIN_SEPARATOR_STR)).join("outside.yaml");

        let error = profile_file_path_in(Path::new("profiles"), &absolute)
            .expect_err("absolute profile path must be rejected");

        assert!(error.to_string().contains("relative file name"));
    }

    #[test]
    fn invalid_historical_profile_path_has_no_cleanup_target() {
        assert_eq!(
            profile_cleanup_path_in(Path::new("profiles"), "../outside.yaml"),
            None
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_path_rejects_windows_alternate_data_streams() {
        let error = profile_file_path_in(Path::new("profiles"), "profile.yaml:secret")
            .expect_err("Windows alternate data streams must be rejected");

        assert!(error.to_string().contains("alternate data stream"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_path_rejects_windows_trailing_dot_or_space() {
        for file in ["profile.yaml.", "profile.yaml "] {
            let error = profile_file_path_in(Path::new("profiles"), file)
                .expect_err("Windows trailing dot or space must be rejected");
            assert!(error.to_string().contains("dot or space"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_path_rejects_reserved_windows_device_names() {
        for file in [
            "NUL",
            "nul.yaml",
            "NUL .yaml",
            "COM1.yml",
            "COM1 .yaml",
            "LPT9",
        ] {
            let error = profile_file_path_in(Path::new("profiles"), file)
                .expect_err("reserved Windows device name must be rejected");
            assert!(error.to_string().contains("reserved Windows device name"));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_path_accepts_similar_non_reserved_windows_names() {
        for file in ["null.yaml", "COM10.yaml", "LPT0.yaml"] {
            let path = profile_file_path_in(Path::new("profiles"), file)
                .expect("non-reserved Windows file name must be accepted");
            assert_eq!(path, Path::new("profiles").join(file));
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn profile_file_path_rejects_other_invalid_windows_characters() {
        for file in [
            "profile?.yaml",
            "profile*.yaml",
            "profile<copy>.yaml",
            "profile|copy.yaml",
            "profile\u{001f}.yaml",
        ] {
            let error = profile_file_path_in(Path::new("profiles"), file)
                .expect_err("invalid Windows file characters must be rejected");
            assert!(error.to_string().contains("invalid on Windows"));
        }
    }

    #[test]
    fn canonical_profile_target_accepts_a_target_inside_the_directory() {
        validate_canonical_profile_target_in(
            Path::new("C:/app/profiles"),
            Path::new("C:/app/profiles/profile.yaml"),
        )
        .expect("canonical child target must be accepted");
    }

    #[test]
    fn canonical_profile_target_rejects_a_link_resolving_outside_the_directory() {
        let error = validate_canonical_profile_target_in(
            Path::new("C:/app/profiles"),
            Path::new("C:/outside/profile.yaml"),
        )
        .expect_err("resolved target outside profiles directory must be rejected");

        assert!(error.to_string().contains("escapes the profiles directory"));
    }

    #[test]
    fn canonical_profile_target_rejects_a_similar_sibling_directory_prefix() {
        let error = validate_canonical_profile_target_in(
            Path::new("C:/app/profiles"),
            Path::new("C:/app/profiles-backup/profile.yaml"),
        )
        .expect_err("similar sibling directory prefix must not be treated as a child");

        assert!(error.to_string().contains("escapes the profiles directory"));
    }

    #[test]
    fn checked_profile_path_accepts_an_existing_regular_file() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let expected = directory.path().join("profile.yaml");
        std::fs::write(&expected, "mixed-port: 7890").expect("write profile file");

        let path = profile_file_path_checked_in(directory.path(), "profile.yaml")
            .expect("existing regular profile file must be accepted");

        assert_eq!(path, expected);
    }

    #[test]
    fn checked_profile_path_accepts_a_missing_future_file() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        let expected = directory.path().join("future.yaml");

        let path = profile_file_path_checked_in(directory.path(), "future.yaml")
            .expect("missing future profile file must be accepted");

        assert_eq!(path, expected);
        assert!(!path.exists());
    }

    #[test]
    fn checked_profile_path_rejects_a_directory_disguised_as_a_profile_file() {
        let directory = tempfile::tempdir().expect("temporary profile directory");
        std::fs::create_dir(directory.path().join("profile.yaml"))
            .expect("create colliding directory");

        let error = profile_file_path_checked_in(directory.path(), "profile.yaml")
            .expect_err("directory target must not be accepted as a profile file");

        assert!(error.to_string().contains("regular file"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn checked_profile_path_rejects_a_broken_file_symlink() {
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().expect("temporary profile directory");
        let link = directory.path().join("profile.yaml");
        symlink_file(directory.path().join("missing.yaml"), &link)
            .expect("create broken profile symlink");

        let error = profile_file_path_checked_in(directory.path(), "profile.yaml")
            .expect_err("broken profile symlink must not be treated as a future file");

        assert!(error.to_string().contains("failed to resolve profile file"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn checked_profile_path_accepts_a_file_symlink_resolving_inside_the_directory() {
        use std::os::windows::fs::symlink_file;

        let directory = tempfile::tempdir().expect("temporary profile directory");
        let target = directory.path().join("target.yaml");
        std::fs::write(&target, "mixed-port: 7890").expect("write profile target");
        symlink_file(&target, directory.path().join("profile.yaml"))
            .expect("create internal profile symlink");

        let path = profile_file_path_checked_in(directory.path(), "profile.yaml")
            .expect("profile symlink resolving inside the directory must be accepted");

        assert_eq!(path, directory.path().join("profile.yaml"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn checked_profile_path_rejects_a_file_symlink_resolving_outside_the_directory() {
        use std::os::windows::fs::symlink_file;

        let root = tempfile::tempdir().expect("temporary root directory");
        let directory = root.path().join("profiles");
        std::fs::create_dir(&directory).expect("create profile directory");
        let outside = root.path().join("outside.yaml");
        std::fs::write(&outside, "mixed-port: 7890").expect("write outside target");
        symlink_file(&outside, directory.join("profile.yaml"))
            .expect("create outside profile symlink");

        let error = profile_file_path_checked_in(&directory, "profile.yaml")
            .expect_err("profile symlink resolving outside the directory must be rejected");

        assert!(error.to_string().contains("escapes the profiles directory"));
    }
}
