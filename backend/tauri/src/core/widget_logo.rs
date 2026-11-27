//! Read-only widget icon files, following ref's host icon storage.

use std::{borrow::Cow, path::PathBuf};

use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;

use crate::core::tray::icon::{self, tray_icons_path};

/// Where the widget's own icon for `state` is stored.
pub fn widget_icon_path(paths: &PathResolver, state: TrayIcon) -> PathBuf {
    tray_icons_path(paths, &format!("widget_{}", state.as_str()))
}

/// The widget's own icon for `state`, or the bundled one if none is set.
pub fn get_raw_icon(paths: &PathResolver, state: TrayIcon) -> Cow<'static, [u8]> {
    match std::fs::read(widget_icon_path(paths, state)) {
        Ok(bytes) if icon::valid_png(&bytes) => Cow::Owned(bytes),
        Ok(_) => Cow::Borrowed(icon::raw_bytes(&state)),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::error!("failed to read the widget icon: {error}");
            }
            Cow::Borrowed(icon::raw_bytes(&state))
        }
    }
}

pub fn is_icon_set(paths: &PathResolver, state: TrayIcon) -> bool {
    widget_icon_path(paths, state).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widget_and_tray_read_separate_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        let path = widget_icon_path(&paths, TrayIcon::SystemProxy);
        assert_eq!(
            path,
            root.join("config/icons/widget_system_proxy.png")
                .into_std_path_buf()
        );
        assert_eq!(
            get_raw_icon(&paths, TrayIcon::SystemProxy).as_ref(),
            icon::raw_bytes(&TrayIcon::SystemProxy)
        );
        let stored = include_bytes!("../../icons/128x128.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, stored).unwrap();
        assert_eq!(get_raw_icon(&paths, TrayIcon::SystemProxy).as_ref(), stored);
        assert_eq!(
            icon::get_raw_icon(&paths, TrayIcon::SystemProxy).as_ref(),
            icon::raw_bytes(&TrayIcon::SystemProxy)
        );
        std::fs::write(path, b"not a png").unwrap();
        assert_eq!(
            get_raw_icon(&paths, TrayIcon::SystemProxy).as_ref(),
            icon::raw_bytes(&TrayIcon::SystemProxy)
        );
    }
}
