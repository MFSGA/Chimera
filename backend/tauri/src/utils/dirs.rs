use anyhow::Result;
use fs_err as fs;
use nyanpasu_utils::dirs::{suggest_config_dir, suggest_data_dir};
use once_cell::sync::Lazy;

use std::{borrow::Cow, path::PathBuf};

use crate::log_err;

pub static APP_VERSION: &str = env!("CHIMERA_VERSION");

#[cfg(not(feature = "verge-dev"))]
pub const APP_NAME: &str = "clash-chimera";
#[cfg(feature = "verge-dev")]
pub const APP_NAME: &str = "clash-chimera-dev";

pub const PROFILE_YAML: &str = "profiles.yaml";

/// App Dir placeholder
/// It is used to create the config and data dir in the filesystem
/// For windows, the style should be similar to `C:/Users/nyanapasu/AppData/Roaming/Clash Nyanpasu`
/// For macos, it should be similar to `/Users/nyanpasu/Library/Application Support/Clash Nyanpasu`
/// For other platforms, it should be similar to `/home/nyanpasu/.config/clash-nyanpasu`
pub static APP_DIR_PLACEHOLDER: Lazy<Cow<'static, str>> = Lazy::new(|| {
    use convert_case::{Case, Casing};
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        Cow::Owned(APP_NAME.to_case(Case::Title))
    } else {
        Cow::Borrowed(APP_NAME)
    }
});

pub const CHIMERA_CONFIG: &str = "chimera-config.yaml";
pub const CLASH_CFG_GUARD_OVERRIDES: &str = "clash-guard-overrides.yaml";

#[cfg(target_os = "windows")]
pub fn get_portable_flag() -> bool {
    *crate::consts::IS_PORTABLE
}

pub fn app_config_dir() -> Result<PathBuf> {
    let path: Option<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            if get_portable_flag() {
                let app_dir = app_install_dir()?;
                Some(app_dir.join(".config").join(APP_NAME))
            } else if let Ok(Some(path)) = super::winreg::get_app_dir() {
                Some(path)
            } else {
                None
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    };

    match path {
        Some(path) => Ok(path),
        None => suggest_config_dir(&APP_DIR_PLACEHOLDER)
            .ok_or(anyhow::anyhow!("failed to get the app config dir")),
    }
    .and_then(|dir| {
        create_dir_all(&dir)?;
        Ok(dir)
    })
}

/// profiles dir
pub fn app_profiles_dir() -> Result<PathBuf> {
    let path = app_config_dir()?.join("profiles");
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log_err!(create_dir_all(&path));
    });
    Ok(path)
}

/// App install dir, sidecars should placed here
pub fn app_install_dir() -> Result<PathBuf> {
    let exe = tauri::utils::platform::current_exe()?;
    let exe = dunce::canonicalize(exe)?;
    let dir = exe
        .parent()
        .ok_or(anyhow::anyhow!("failed to get the app install dir"))?;
    Ok(PathBuf::from(dir))
}

pub fn profiles_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(PROFILE_YAML))
}

fn create_dir_all(dir: &PathBuf) -> Result<(), std::io::Error> {
    let meta = fs::metadata(dir);
    if let Ok(meta) = meta {
        if !meta.is_dir() {
            fs_err::remove_file(dir)?;
        } else {
            return Ok(());
        }
    }
    fs_extra::dir::create_all(dir, false).map_err(|e| {
        std::io::Error::other(format!("failed to create dir: {:?}, kind: {:?}", e, e.kind))
    })?;
    Ok(())
}

pub fn app_data_dir() -> Result<PathBuf> {
    let path: Option<PathBuf> = {
        #[cfg(target_os = "windows")]
        {
            if get_portable_flag() {
                let app_dir = app_install_dir()?;
                Some(app_dir.join(".data").join(APP_NAME))
            } else {
                None
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    };

    match path {
        Some(path) => Ok(path),
        None => suggest_data_dir(&APP_DIR_PLACEHOLDER)
            .ok_or(anyhow::anyhow!("failed to get the app data dir")),
    }
    .and_then(|dir| {
        create_dir_all(&dir)?;
        Ok(dir)
    })
}

/// logs dir
pub fn app_logs_dir() -> Result<PathBuf> {
    let path = app_data_dir()?.join("logs");
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        log_err!(create_dir_all(&path));
    });
    Ok(path)
}

pub fn chimera_config_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(CHIMERA_CONFIG))
}

pub fn clash_guard_overrides_path() -> Result<PathBuf> {
    Ok(app_config_dir()?.join(CLASH_CFG_GUARD_OVERRIDES))
}
