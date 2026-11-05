use super::*;

#[test]
fn default_config_initialization_uses_only_injected_config_root() {
    let temp = tempfile::tempdir().unwrap();
    let config_dir = temp.path().join("isolated-config");
    let data_dir = temp.path().join("untouched-data");
    let paths = PathResolver::with_base_dirs(config_dir.clone(), data_dir.clone());
    fs::create_dir_all(&config_dir).unwrap();

    initialize_default_configs(&paths).unwrap();

    assert!(paths.app_profiles_dir().is_dir());
    for path in [
        paths.clash_guard_overrides_path(),
        paths.chimera_config_path(),
        paths.profiles_path(),
    ] {
        assert!(
            path.is_file(),
            "startup did not initialize {}",
            path.display()
        );
    }
    assert!(
        !data_dir.exists(),
        "defaults must not resolve or create a separate data root"
    );
}

#[test]
fn missing_legacy_file_gets_its_original_default_header() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("chimera-config.yaml");
    initialize_default_yaml(&path, &IVerge::template()).unwrap();
    let actual = std::fs::read_to_string(&path).unwrap();
    assert!(actual.starts_with("# Clash Chimera\n\n"));
    assert!(actual.contains("clash_core:"));
}

#[test]
fn existing_legacy_file_is_never_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("profiles.yaml");
    std::fs::write(&path, b"existing: user-profile\n").unwrap();
    initialize_default_yaml(&path, &Profiles::default()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"existing: user-profile\n");
}

#[cfg(unix)]
#[test]
fn dangling_legacy_config_symlink_is_not_replaced_by_a_default() {
    use std::os::unix::fs::symlink;
    let temp = tempfile::tempdir().unwrap();
    for name in [
        "chimera-config.yaml",
        "clash-guard-overrides.yaml",
        "profiles.yaml",
    ] {
        let path = temp.path().join(name);
        let target = temp.path().join(format!("missing-{name}"));
        symlink(&target, &path).unwrap();
        initialize_default_yaml(&path, &IVerge::template()).unwrap();
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!target.exists());
    }
    assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 3);
}

#[cfg(unix)]
#[test]
fn unreadable_parent_shape_is_an_error_without_creating_a_file() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("parent-is-a-file");
    std::fs::write(&parent, b"keep").unwrap();
    let path = parent.join("chimera-config.yaml");
    let error = initialize_default_yaml(&path, &IVerge::template()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("failed to inspect startup config")
    );
    assert_eq!(std::fs::read(&parent).unwrap(), b"keep");
}
