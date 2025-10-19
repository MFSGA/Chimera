use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{Context, Result, bail};
use fs_err as fs;
use nanoid::nanoid;
use serde::de::DeserializeOwned;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_shell::ShellExt;

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
