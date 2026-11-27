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

fn cache_file(paths: &PathResolver, mode: &TrayIcon) -> camino::Utf8PathBuf {
    paths
        .cache_dir()
        .join("icons")
        .join(format!("tray_{mode}.png"))
}

#[tracing_attributes::instrument(skip(paths))]
fn resize_image(paths: &PathResolver, mode: TrayIcon, scale_factor: f64) -> anyhow::Result<()> {
    let raw_icon = get_raw_icon(paths, mode);
    let icon = match crate::utils::help::resize_tray_image(&raw_icon, scale_factor) {
        Ok(icon) => icon,
        Err(error) => {
            tracing::warn!(%error, ?mode, "failed to resize tray icon; using raw icon");
            raw_icon.into_owned()
        }
    };
    let cache = cache_file(paths, &mode);
    std::fs::create_dir_all(cache.parent().expect("cache file has a parent"))?;
    std::fs::write(cache, icon)?;
    Ok(())
}

#[tracing_attributes::instrument(skip(paths))]
pub fn resize_images(paths: &PathResolver, scale_factor: f64) {
    for mode in TrayIcon::all_supported() {
        if let Err(error) = resize_image(paths, *mode, scale_factor) {
            tracing::warn!(%error, ?mode, "failed to resize tray icon");
        }
    }
}

pub fn get_icon(paths: &PathResolver, mode: &TrayIcon) -> Vec<u8> {
    let cache = cache_file(paths, mode);
    match std::fs::read(cache) {
        Ok(bytes) if valid_png(&bytes) => bytes,
        _ => get_raw_icon(paths, *mode).into_owned(),
    }
}

/// Store the selected file as a PNG, or restore the bundled icon.
pub fn set_icon(
    paths: &PathResolver,
    mode: TrayIcon,
    path: Option<PathBuf>,
    scale_factor: f64,
) -> anyhow::Result<()> {
    let target = tray_icons_path(paths, mode.as_str());
    match path {
        Some(path) => {
            let image = image::open(path)?;
            std::fs::create_dir_all(target.parent().expect("icon path has a parent"))?;
            image.save_with_format(target, image::ImageFormat::Png)?;
        }
        None => match std::fs::remove_file(target) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        },
    }
    resize_image(paths, mode, scale_factor)?;
    Ok(())
}

/// Store an encoded icon as PNG and refresh the tray's resized cache.
pub fn set_icon_from_bytes(
    paths: &PathResolver,
    mode: TrayIcon,
    bytes: &[u8],
    scale_factor: f64,
) -> anyhow::Result<()> {
    let image = image::load_from_memory(bytes)?;
    let path = tray_icons_path(paths, mode.as_str());
    std::fs::create_dir_all(path.parent().expect("icon path has a parent"))?;
    image.save_with_format(path, image::ImageFormat::Png)?;
    resize_image(paths, mode, scale_factor)?;
    Ok(())
}

pub fn check_icon(bytes: &[u8]) -> anyhow::Result<()> {
    tauri::image::Image::from_bytes(bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_selection_and_reset_refresh_the_tray_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        let mode = TrayIcon::Tun;
        let source = root.join("source.png");
        std::fs::write(&source, include_bytes!("../../../icons/128x128.png")).unwrap();
        set_icon(&paths, mode, Some(source.into_std_path_buf()), 2.0).unwrap();
        assert!(tray_icons_path(&paths, mode.as_str()).exists());
        assert_eq!(
            image::load_from_memory(&get_icon(&paths, &mode))
                .unwrap()
                .width(),
            64
        );
        set_icon(&paths, mode, None, 1.0).unwrap();
        assert!(!tray_icons_path(&paths, mode.as_str()).exists());
        assert_eq!(get_raw_icon(&paths, mode).as_ref(), raw_bytes(&mode));
        assert_eq!(
            image::load_from_memory(&get_icon(&paths, &mode))
                .unwrap()
                .width(),
            32
        );
        set_icon(&paths, mode, None, 1.0).unwrap();
    }

    #[test]
    fn stored_tray_icon_is_resized_to_scale_and_invalid_upload_preserves_it() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        let mode = TrayIcon::Tun;
        let stored = include_bytes!("../../../icons/128x128.png");
        set_icon_from_bytes(&paths, mode, stored, 2.0).unwrap();
        let before = get_raw_icon(&paths, mode).into_owned();
        let cached = get_icon(&paths, &mode);
        assert_eq!(image::load_from_memory(&cached).unwrap().width(), 64);
        assert_eq!(image::load_from_memory(&cached).unwrap().height(), 64);
        check_icon(&cached).unwrap();
        assert!(set_icon_from_bytes(&paths, mode, b"invalid", 2.0).is_err());
        assert_eq!(get_raw_icon(&paths, mode).as_ref(), before);
        resize_images(&paths, 1.0);
        assert_eq!(
            image::load_from_memory(&get_icon(&paths, &mode))
                .unwrap()
                .width(),
            32
        );
    }

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
