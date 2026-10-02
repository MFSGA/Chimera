use anyhow::Result;
use chimera_utils::dirs::{suggest_config_dir, suggest_data_dir};
use fs_err as fs;
use once_cell::sync::Lazy;
use tauri::{Env, utils::platform::resource_dir};

use std::{borrow::Cow, path::PathBuf};

use crate::{core::handle, log_err};

pub static APP_VERSION: &str = env!("CHIMERA_VERSION");

#[cfg(not(feature = "verge-dev"))]
pub const APP_NAME: &str = "clash-chimera";
#[cfg(feature = "verge-dev")]
pub const APP_NAME: &str = "clash-chimera-dev";

pub const PROFILE_YAML: &str = "profiles.yaml";

/// App Dir placeholder
/// It is used to create the config and data dir in the filesystem
/// For windows, the style should be similar to `C:/Users/nyanapasu/AppData/Roaming/Clash Chimera`
/// For macos, it should be similar to `/Users/chimera/Library/Application Support/Clash Chimera`
/// For other platforms, it should be similar to `/home/chimera/.config/clash-chimera`
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

pub const STORAGE_DB: &str = "storage.db";

#[cfg(feature = "e2e")]
fn e2e_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[cfg(target_os = "windows")]
pub fn get_portable_flag() -> bool {
    *crate::consts::IS_PORTABLE
}

pub fn app_config_dir() -> Result<PathBuf> {
    #[cfg(feature = "e2e")]
    if let Some(path) = e2e_dir("CHIMERA_E2E_CONFIG_DIR") {
        create_dir_all(&path)?;
        return Ok(path);
    }

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
    #[cfg(feature = "e2e")]
    if let Some(path) = e2e_dir("CHIMERA_E2E_DATA_DIR") {
        create_dir_all(&path)?;
        return Ok(path);
    }

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

pub fn clash_pid_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join("clash.pid"))
}

pub fn storage_path() -> Result<PathBuf> {
    Ok(app_data_dir()?.join(STORAGE_DB))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn check_core_permission(core: &chimera_utils::core::CoreType) -> anyhow::Result<bool> {
    #[cfg(target_os = "macos")]
    const ROOT_GROUP: &str = "admin";
    #[cfg(target_os = "linux")]
    const ROOT_GROUP: &str = "root";

    use anyhow::Context;
    use nix::unistd::{Gid, Group as NixGroup, Uid, User};
    use std::os::unix::fs::MetadataExt;

    let core_path =
        crate::core::clash::core::find_binary_path(core).context("clash core not found")?;
    let metadata = std::fs::metadata(&core_path).context("failed to get core metadata")?;
    let uid = metadata.uid();
    let gid = metadata.gid();
    let user = User::from_uid(Uid::from_raw(uid)).ok().flatten();
    let group = NixGroup::from_gid(Gid::from_raw(gid)).ok().flatten();
    if let (Some(user), Some(group)) = (user, group) {
        if user.name == "root" && group.name == ROOT_GROUP {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Grant the selected macOS core the privileges expected by the upstream TUN
/// launch path. This intentionally runs only from the explicit TUN-enable
/// preflight; it asks macOS for administrator authorization and verifies the
/// resulting owner, group, and set-user-ID bit before returning success.
#[cfg(target_os = "macos")]
pub fn grant_macos_tun_permission(core: &chimera_utils::core::CoreType) -> anyhow::Result<()> {
    use anyhow::Context;

    let path = crate::core::clash::core::find_binary_path(core)
        .context("clash core not found")?
        .canonicalize()
        .context("failed to resolve the selected core binary")?;
    let metadata = std::fs::metadata(&path).context("failed to inspect the selected core")?;
    if macos_tun_permission_granted(&metadata)? {
        return Ok(());
    }

    let shell_command = format!(
        "/usr/sbin/chown root:admin {} && /bin/chmod u+s {}",
        shell_single_quote(&path.to_string_lossy()),
        shell_single_quote(&path.to_string_lossy()),
    );
    let applescript = format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&shell_command)
    );
    let status = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &applescript])
        .status()
        .context("failed to request macOS administrator authorization for TUN")?;
    anyhow::ensure!(
        status.success(),
        "macOS administrator authorization for TUN failed"
    );

    let metadata = std::fs::metadata(&path).context("failed to verify the selected core")?;
    anyhow::ensure!(
        macos_tun_permission_granted(&metadata)?,
        "macOS did not apply the required TUN core permissions"
    );
    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_tun_permission_granted(metadata: &std::fs::Metadata) -> anyhow::Result<bool> {
    use nix::unistd::{Gid, Group as NixGroup, Uid, User};
    use std::os::unix::fs::MetadataExt;

    Ok(
        User::from_uid(Uid::from_raw(metadata.uid()))?.is_some_and(|user| user.name == "root")
            && NixGroup::from_gid(Gid::from_raw(metadata.gid()))?
                .is_some_and(|group| group.name == "admin")
            && metadata.mode() & 0o4000 != 0,
    )
}

#[cfg(target_os = "macos")]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "macos")]
fn applescript_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tun_permission_tests {
    use super::{applescript_escape, shell_single_quote};

    #[test]
    fn shell_path_is_quoted_as_one_argument() {
        assert_eq!(
            shell_single_quote("/tmp/Core's Bin"),
            "'/tmp/Core'\\''s Bin'"
        );
    }

    #[test]
    fn applescript_command_escapes_string_delimiters() {
        assert_eq!(applescript_escape("a\\b\"c"), "a\\\\b\\\"c");
    }
}

/// get the resources dir
pub fn app_resources_dir() -> Result<PathBuf> {
    let handle = handle::Handle::global();
    let app_handle = handle.app_handle.lock();
    if let Some(app_handle) = app_handle.as_ref() {
        let res_dir = resource_dir(app_handle.package_info(), &Env::default())
            .map_err(|_| anyhow::anyhow!("failed to get the resource dir 1"))?
            .join("resources");
        return Ok(res_dir);
    };
    Err(anyhow::anyhow!("failed to get the resource dir 2"))
}
