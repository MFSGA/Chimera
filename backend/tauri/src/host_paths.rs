//! GUI package inputs for the frontend-independent directory resolver.
use camino::Utf8PathBuf;
#[cfg(feature = "e2e")]
use chimera_paths::Root;
use chimera_paths::{DiscoverError, PathResolver};
#[cfg(not(feature = "e2e"))]
use chimera_paths::{HostInputs, InstallDirError};
#[cfg(not(feature = "e2e"))]
use std::{collections::BTreeMap, sync::Arc};

#[cfg(not(feature = "verge-dev"))]
pub(crate) const APP_NAME: &str = "clash-chimera";
#[cfg(feature = "verge-dev")]
pub(crate) const APP_NAME: &str = "clash-chimera-dev";

/// Collects the host inputs once and resolves the process's directories from them. `run()`
/// calls this once and passes the resolver down.
#[cfg(feature = "e2e")]
pub fn discover() -> Result<PathResolver, DiscoverError> {
    // Fail closed if the E2E runner omits either isolated root: never read or
    // write the user's real configuration from a test executable.
    e2e_roots()
}

#[cfg(not(feature = "e2e"))]
pub fn discover() -> Result<PathResolver, DiscoverError> {
    let install_dir = tauri::utils::platform::current_exe()
        .map_err(|error| InstallDirError::Executable {
            source: Arc::new(error),
        })
        .and_then(|executable| chimera_paths::installation_dir(&executable));
    // Only Windows has a portable layout. The marker is a package property, read from the
    // same install dir the resolver is given.
    #[cfg(windows)]
    let portable = install_dir
        .as_ref()
        .is_ok_and(|dir| crate::bundle::is_portable(dir.as_std_path()));
    #[cfg(not(windows))]
    let portable = false;
    PathResolver::discover(HostInputs {
        app_name: APP_NAME.to_owned(),
        install_dir,
        portable,
        development_binaries: development_binaries(),
    })
}

#[cfg(feature = "e2e")]
fn e2e_roots() -> Result<PathResolver, DiscoverError> {
    e2e_roots_for(
        std::env::var_os("CHIMERA_E2E_CONFIG_DIR"),
        std::env::var_os("CHIMERA_E2E_DATA_DIR"),
    )
}

/// Pure converter for test isolation, without changing process environment.
#[cfg(feature = "e2e")]
fn e2e_roots_for(
    config: Option<std::ffi::OsString>,
    data: Option<std::ffi::OsString>,
) -> Result<PathResolver, DiscoverError> {
    let (config, data) = match (config, data) {
        (Some(config), Some(data)) => (config, data),
        (None, _) => return Err(DiscoverError::MissingE2ERoot { root: Root::Config }),
        (_, None) => return Err(DiscoverError::MissingE2ERoot { root: Root::Data }),
    };
    let config =
        Utf8PathBuf::from_path_buf(config.into()).map_err(|path| DiscoverError::NotUtf8 {
            root: Root::Config,
            path,
        })?;
    let data = Utf8PathBuf::from_path_buf(data.into()).map_err(|path| DiscoverError::NotUtf8 {
        root: Root::Data,
        path,
    })?;
    Ok(PathResolver::with_base_dirs(config, data))
}

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use super::*;

    #[test]
    fn e2e_directory_overrides_must_be_a_complete_pair() {
        assert!(matches!(
            e2e_roots_for(None, None),
            Err(DiscoverError::MissingE2ERoot { root: Root::Config })
        ));
        assert!(matches!(
            e2e_roots_for(Some("config".into()), None),
            Err(DiscoverError::MissingE2ERoot { root: Root::Data })
        ));
        assert!(matches!(
            e2e_roots_for(None, Some("data".into())),
            Err(DiscoverError::MissingE2ERoot { root: Root::Config })
        ));
    }

    #[test]
    fn e2e_directory_overrides_are_passed_through_without_host_lookup() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("isolated-config");
        let data = temp.path().join("isolated-data");
        let paths = e2e_roots_for(
            Some(config.clone().into_os_string()),
            Some(data.clone().into_os_string()),
        )
        .unwrap();
        assert_eq!(paths.app_config_dir().as_std_path(), config);
        assert_eq!(paths.app_data_dir().as_std_path(), data);
        assert!(!config.exists());
        assert!(!data.exists());
    }
}

#[cfg(all(debug_assertions, not(feature = "e2e")))]
fn development_binaries() -> BTreeMap<String, Utf8PathBuf> {
    let Ok(triple) = tauri::utils::platform::target_triple() else {
        return BTreeMap::new();
    };
    chimera_utils::core::CoreType::get_supported_cores()
        .iter()
        .map(|core| {
            let name = core.get_executable_name();
            let stem = name
                .strip_suffix(std::env::consts::EXE_SUFFIX)
                .unwrap_or(name);
            let path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("sidecar")
                .join(format!("{stem}-{triple}{}", std::env::consts::EXE_SUFFIX));
            (name.to_owned(), path)
        })
        .collect()
}
#[cfg(all(not(debug_assertions), not(feature = "e2e")))]
fn development_binaries() -> BTreeMap<String, Utf8PathBuf> {
    BTreeMap::new()
}
