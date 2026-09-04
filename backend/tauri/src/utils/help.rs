use std::{
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, anyhow, bail};
use atomicwrites::{AtomicFile, OverwriteBehavior};
use fs_err as fs;
use nanoid::nanoid;
use serde::{Serialize, de::DeserializeOwned};
use serde_yaml::{Mapping, Value};
use tauri::{AppHandle, Manager, process::current_binary};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::ShellExt;
use tracing::{debug, instrument};

use crate::{client::ChimeraClient, utils::resolve};

const ALPHABET: [char; 62] = [
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i',
    'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'A', 'B',
    'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U',
    'V', 'W', 'X', 'Y', 'Z',
];

#[macro_export]
macro_rules! trace_err {
    ($result: expr, $err_str: expr) => {
        if let Err(err) = $result {
            log::trace!(target: "app", "{}, err {:?}", $err_str, err);
        }
    }
}

#[macro_export]
macro_rules! log_err {
    ($result: expr) => {
        if let Err(err) = $result {
            log::error!(target: "app", "{:#?}", err);
        }
    };

    ($result: expr, $label: expr) => {
        if let Err(err) = $result {
            log::error!(target: "app", "{}: {:#?}", $label, err);
        }
    };
}

/// generate the uid
pub fn get_uid(prefix: &str) -> String {
    let id = nanoid!(11, &ALPHABET);
    format!("{prefix}{id}")
}

/// parse the string
/// xxx=123123; => 123123
pub fn parse_str<T: FromStr>(target: &str, key: &str) -> Option<T> {
    target.split(';').map(str::trim).find_map(|s| {
        let mut parts = s.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some(k), Some(v)) if k == key => v.parse::<T>().ok(),
            _ => None,
        }
    })
}

/// read data from yaml as struct T
pub fn read_yaml<T: DeserializeOwned, P: AsRef<Path>>(path: P) -> Result<T> {
    let path = path.as_ref();
    if !path.exists() {
        bail!("file not found \"{}\"", path.display());
    }

    let yaml_str = fs::read_to_string(path)
        .with_context(|| format!("failed to read the file \"{}\"", path.display()))?;

    serde_yaml::from_str::<T>(&yaml_str).with_context(|| {
        format!(
            "failed to read the file with yaml format \"{}\"",
            path.display()
        )
    })
}

/// open file
/// use vscode by default
pub fn open_file(app: tauri::AppHandle, path: PathBuf) -> Result<()> {
    #[cfg(target_os = "macos")]
    let code = "Visual Studio Code";
    #[cfg(windows)]
    let code = "code.cmd";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let code = "code";

    let shell = app.shell();

    trace_err!(
        match which::which(code) {
            Ok(code_path) => {
                log::debug!(target: "app", "find VScode `{}`", code_path.display());
                #[cfg(not(windows))]
                {
                    crate::utils::open::with(path, code)
                }
                #[cfg(windows)]
                {
                    use std::ffi::OsString;
                    let mut buf = OsString::with_capacity(path.as_os_str().len() + 2);
                    buf.push("\"");
                    buf.push(path.as_os_str());
                    buf.push("\"");

                    open::with_detached(buf, code)
                }
            }
            Err(err) => {
                log::error!(target: "app", "Can't find VScode `{err:?}`");
                // default open
                app.opener()
                    .open_url(path.to_string_lossy().to_string(), None::<String>)
                    .map_err(std::io::Error::other)
            }
        },
        "Can't open file"
    );

    Ok(())
}

/// save the data to the file
/// can set `prefix` string to add some comments
pub fn save_yaml<T: Serialize, P: AsRef<Path>>(
    path: P,
    data: &T,
    prefix: Option<&str>,
) -> Result<()> {
    let path = path.as_ref();
    let data_str = serde_yaml::to_string(data)?;

    let yaml_str = match prefix {
        Some(prefix) => format!("{prefix}\n\n{data_str}"),
        None => data_str,
    };

    let path_str = path.as_os_str().to_string_lossy().to_string();
    AtomicFile::new(path, OverwriteBehavior::AllowOverwrite)
        .write(|file| file.write_all(yaml_str.as_bytes()))
        .with_context(|| format!("failed to atomically save file \"{path_str}\""))
}

/// read mapping from yaml fix #165
pub fn read_merge_mapping(path: &PathBuf) -> Result<Mapping> {
    let mut val: Value = read_yaml(path)?;
    val.apply_merge()
        .with_context(|| format!("failed to apply merge \"{}\"", path.display()))?;

    Ok(val
        .as_mapping()
        .ok_or(anyhow!(
            "failed to transform to yaml mapping \"{}\"",
            path.display()
        ))?
        .to_owned())
}

#[instrument(skip(app_handle))]
pub fn cleanup_processes(app_handle: &AppHandle) -> Result<()> {
    debug!(target: "app", "cleanup processes");
    // let _ = super::resolve::save_window_state(app_handle, true);
    resolve::resolve_reset();
    /* let widget_manager = app_handle.state::<crate::widget::WidgetManager>(); */
    let connector = app_handle
        .try_state::<crate::core::clash::ws::ClashConnectionsConnector>()
        .map(|state| state.inner().clone());
    let client = app_handle
        .try_state::<ChimeraClient>()
        .map(|state| state.inner().clone());
    nyanpasu_utils::runtime::block_on(async {
        if let Some(connector) = connector {
            connector.stop().await;
        }
        let client = client.ok_or_else(|| anyhow!("ChimeraClient is not managed"))?;
        client.ensure_core_stopped_for_update().await
    })?;
    #[cfg(windows)]
    crate::shutdown_hook::set_ready_for_shutdown();
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde::ser::{Error as _, Serializer};

    use super::*;

    struct SerializationFailure;

    impl Serialize for SerializationFailure {
        fn serialize<S>(&self, _serializer: S) -> std::result::Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            Err(S::Error::custom("expected serialization failure"))
        }
    }

    #[test]
    fn save_yaml_atomically_replaces_existing_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");
        std::fs::write(&path, "old: value\n").unwrap();

        save_yaml(
            &path,
            &serde_yaml::from_str::<Value>("new: value").unwrap(),
            None,
        )
        .unwrap();

        assert_eq!(std::fs::read_to_string(path).unwrap(), "new: value\n");
    }

    #[test]
    fn save_yaml_serialization_failure_preserves_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yaml");
        std::fs::write(&path, "old: value\n").unwrap();

        let error = save_yaml(&path, &SerializationFailure, None).unwrap_err();

        assert!(error.to_string().contains("expected serialization failure"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "old: value\n");
    }
}

#[instrument(skip(app_handle))]
pub fn quit_application(app_handle: &AppHandle) {
    app_handle.exit(0);
}

#[instrument(skip(app_handle))]
pub fn restart_application(app_handle: &AppHandle) {
    crate::log_err!(cleanup_processes(app_handle));
    let env = app_handle.env();
    let path = current_binary(&env).unwrap();
    let arg = std::env::args().collect::<Vec<String>>();
    let mut args = vec!["launch".to_string(), "--".to_string()];
    // filter out the first arg
    if arg.len() > 1 {
        args.extend(arg.iter().skip(1).cloned());
    }
    tracing::info!("restart app: {:#?} with args: {:#?}", path, args);
    std::process::Command::new(path)
        .args(args)
        .spawn()
        .expect("application failed to start");
    app_handle.exit(0);
    std::process::exit(0);
}
