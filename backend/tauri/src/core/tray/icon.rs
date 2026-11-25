use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;
use std::{borrow::Cow, path::PathBuf};

pub(crate) fn icon_path(config_dir: &std::path::Path, mode: &str) -> PathBuf {
    config_dir.join("icons").join(format!("{mode}.png"))
}

pub(crate) fn tray_icons_path(paths: &PathResolver, mode: &str) -> PathBuf {
    let config = paths.app_config_dir();
    icon_path(config.as_std_path(), mode)
}

pub fn raw_bytes(mode: &TrayIcon) -> &'static [u8] {
    // Keep Chimera artwork rather than importing the upstream cat identity.
    match mode {
        TrayIcon::Normal | TrayIcon::Tun | TrayIcon::SystemProxy => {
            include_bytes!("../../../icons/32x32.png")
        }
    }
}

#[tracing_attributes::instrument(skip(paths))]
pub fn get_raw_icon<'n>(paths: &PathResolver, mode: TrayIcon) -> Cow<'n, [u8]> {
    let path = tray_icons_path(paths, mode.as_str());
    if !path.exists() {
        return Cow::Borrowed(raw_bytes(&mode));
    }
    match std::fs::read(path) {
        Ok(bytes) if valid_png(&bytes) => Cow::Owned(bytes),
        Ok(_) => Cow::Borrowed(raw_bytes(&mode)),
        Err(e) => {
            tracing::error!("failed to read icon file: {:?}", e);
            Cow::Borrowed(raw_bytes(&mode))
        }
    }
}

pub(crate) fn valid_png(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a])
        && tauri::image::Image::from_bytes(bytes).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mode_specific_file_and_falls_back_for_missing_or_invalid_png() {
        let temp = tempfile::tempdir().unwrap();
        let config = camino::Utf8PathBuf::from_path_buf(temp.path().join("config")).unwrap();
        let paths = PathResolver::with_base_dirs(config.clone(), config.join("data"));
        assert_eq!(
            get_raw_icon(&paths, TrayIcon::Normal).as_ref(),
            raw_bytes(&TrayIcon::Normal)
        );
        assert!(
            !config.exists(),
            "read-only lookup must not create config directories"
        );
        let stored = include_bytes!("../../../icons/128x128.png");
        let path = tray_icons_path(&paths, "tun");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, stored).unwrap();
        assert_eq!(get_raw_icon(&paths, TrayIcon::Tun).as_ref(), stored);
        assert_eq!(
            get_raw_icon(&paths, TrayIcon::Normal).as_ref(),
            raw_bytes(&TrayIcon::Normal)
        );
        std::fs::write(path, b"not an image").unwrap();
        assert_eq!(
            get_raw_icon(&paths, TrayIcon::Tun).as_ref(),
            raw_bytes(&TrayIcon::Tun)
        );
    }
}
