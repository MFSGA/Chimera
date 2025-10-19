use anyhow::Result;
use once_cell::sync::Lazy;
use std::{borrow::Cow, path::PathBuf};

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

    todo!()
}

/// profiles dir
pub fn app_profiles_dir() -> Result<PathBuf> {
    let path = app_config_dir()?.join("profiles");
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        todo!();
        // log_err!(create_dir_all(&path));
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
