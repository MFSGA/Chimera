//! The statistic widget's logo: its own icon set, stored next to the tray's,
//! and the [`Logo`] the widget is sent for a source and state.

use std::{borrow::Cow, io::Cursor, path::PathBuf};

use chimera_config::application::NetworkStatisticWidgetLogoSource;
use chimera_widget_protocol::{Logo, LogoPreset};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use image::{ImageFormat, RgbaImage};

use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;

use crate::core::tray::icon::{self, tray_icons_path};

/// The largest side of an image logo sent to the widget. The widget's largest
/// badge is 44 px, so this covers a scale factor of 5; anything larger only
/// costs the widget memory to decode.
const MAX_LOGO_SIDE: u32 = 256;

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

/// Stores the image at `path` as the widget's icon for `state`, or removes
/// the stored one when `path` is `None`.
pub fn set_icon(
    paths: &PathResolver,
    state: TrayIcon,
    path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let target = widget_icon_path(paths, state);
    match path {
        Some(path) => {
            let image = image::open(path)?;
            std::fs::create_dir_all(target.parent().expect("widget icon path has a parent"))?;
            image.save_with_format(target, ImageFormat::Png)?;
        }
        None => match std::fs::remove_file(target) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error)?,
            _ => {}
        },
    }
    Ok(())
}

/// Stores the encoded image bytes as the widget's icon for the selected state.
pub fn set_icon_from_bytes(
    paths: &PathResolver,
    state: TrayIcon,
    bytes: &[u8],
) -> anyhow::Result<()> {
    let image = image::load_from_memory(bytes)?;
    let target = widget_icon_path(paths, state);
    std::fs::create_dir_all(target.parent().expect("widget icon path has a parent"))?;
    image.save_with_format(target, image::ImageFormat::Png)?;
    Ok(())
}

/// The logo for `state` from `source`: the stored icon if there is one,
/// otherwise the bundled logo.
pub fn resolve(
    paths: &PathResolver,
    source: NetworkStatisticWidgetLogoSource,
    state: TrayIcon,
) -> Logo {
    let path = match source {
        NetworkStatisticWidgetLogoSource::Tray => tray_icons_path(paths, state.as_str()),
        NetworkStatisticWidgetLogoSource::Widget => widget_icon_path(paths, state),
    };
    let stored = match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("failed to read the widget logo {path:?}: {error}");
            }
            None
        }
    };
    logo_for(state, stored)
}

/// The bundled logo when nothing is stored, when the stored icon does not
/// decode, or when it has the bundled logo's pixels: the tray's presets store
/// the bundled PNG, and the widget draws the logo sharper from its vector
/// source. Any other icon is sent as a PNG of at most [`MAX_LOGO_SIDE`].
fn logo_for(state: TrayIcon, stored: Option<Vec<u8>>) -> Logo {
    let builtin = Logo::Builtin(match state {
        TrayIcon::Normal => LogoPreset::Default,
        TrayIcon::Tun => LogoPreset::Tun,
        TrayIcon::SystemProxy => LogoPreset::System,
    });
    let Some(bytes) = stored else {
        return builtin;
    };
    let image = match image::load_from_memory(&bytes) {
        Ok(image) => image.into_rgba8(),
        Err(error) => {
            tracing::warn!("the widget logo does not decode, showing the bundled one: {error}");
            return builtin;
        }
    };
    let bundled = image::load_from_memory(icon::raw_bytes(&state))
        .expect("the bundled tray icons decode")
        .into_rgba8();
    if image == bundled {
        return builtin;
    }
    if image.width().max(image.height()) <= MAX_LOGO_SIDE
        && image::guess_format(&bytes).is_ok_and(|format| format == ImageFormat::Png)
    {
        return Logo::Image(bytes);
    }
    match shrink(&image).and_then(|image| {
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)?;
        Ok(png)
    }) {
        Ok(png) => Logo::Image(png),
        Err(error) => {
            tracing::warn!("could not shrink the widget logo, showing the bundled one: {error}");
            builtin
        }
    }
}

/// Fits `image` into [`MAX_LOGO_SIDE`] with Lanczos3, weighting colours by
/// alpha so transparent pixels do not darken the edges.
fn shrink(image: &RgbaImage) -> anyhow::Result<RgbaImage> {
    let (width, height) = image.dimensions();
    let scale = f64::from(MAX_LOGO_SIDE) / f64::from(width.max(height));
    let target = |side: u32| ((f64::from(side) * scale).round() as u32).max(1);
    let (target_width, target_height) = (target(width), target(height));
    let source =
        fast_image_resize::images::ImageRef::new(width, height, image.as_raw(), PixelType::U8x4)?;
    let mut shrunk = Image::new(target_width, target_height, PixelType::U8x4);
    Resizer::new().resize(
        &source,
        &mut shrunk,
        &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)),
    )?;
    RgbaImage::from_raw(target_width, target_height, shrunk.into_vec())
        .ok_or_else(|| anyhow::anyhow!("the shrunk logo has the wrong size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn nothing_stored_or_an_undecodable_icon_shows_the_bundled_logo() {
        assert_eq!(
            logo_for(TrayIcon::Tun, None),
            Logo::Builtin(LogoPreset::Tun)
        );
        assert_eq!(
            logo_for(TrayIcon::SystemProxy, Some(b"not an image".to_vec())),
            Logo::Builtin(LogoPreset::System)
        );
    }

    #[test]
    fn a_stored_copy_of_the_bundled_logo_is_drawn_from_its_vector_source() {
        // Re-encoded, as storing an icon does: other bytes, the same pixels.
        for state in TrayIcon::all_supported() {
            let stored = png(&image::load_from_memory(icon::raw_bytes(state))
                .unwrap()
                .into_rgba8());
            assert!(matches!(logo_for(*state, Some(stored)), Logo::Builtin(_)));
        }
    }

    #[test]
    fn another_icon_is_sent_as_stored() {
        let custom = png(&RgbaImage::from_pixel(
            48,
            32,
            image::Rgba([10, 20, 30, 128]),
        ));
        assert_eq!(
            logo_for(TrayIcon::Tun, Some(custom.clone())),
            Logo::Image(custom)
        );
    }

    #[test]
    fn a_large_icon_is_shrunk_to_fit_the_limit() {
        let large = png(&RgbaImage::from_pixel(
            1024,
            512,
            image::Rgba([10, 20, 30, 255]),
        ));
        let Logo::Image(sent) = logo_for(TrayIcon::Normal, Some(large)) else {
            panic!("a large icon is still an image logo");
        };
        let sent = image::load_from_memory(&sent).unwrap();
        assert_eq!((sent.width(), sent.height()), (256, 128));
    }

    #[test]
    fn resolve_uses_the_selected_icon_source_and_reset_restores_builtin() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let paths = PathResolver::with_base_dirs(root.join("config"), root.join("data"));
        let state = TrayIcon::Tun;
        let tray = png(&RgbaImage::from_pixel(20, 10, image::Rgba([1, 2, 3, 255])));
        let widget = png(&RgbaImage::from_pixel(10, 20, image::Rgba([4, 5, 6, 128])));
        icon::set_icon_from_bytes(&paths, state, &tray, 1.0).unwrap();
        let source_file = root.join("source.png");
        std::fs::write(&source_file, &widget).unwrap();
        set_icon(&paths, state, Some(source_file.into_std_path_buf())).unwrap();
        let Logo::Image(tray_resolved) =
            resolve(&paths, NetworkStatisticWidgetLogoSource::Tray, state)
        else {
            panic!("custom tray logo expected");
        };
        let Logo::Image(widget_resolved) =
            resolve(&paths, NetworkStatisticWidgetLogoSource::Widget, state)
        else {
            panic!("custom widget logo expected");
        };
        assert_eq!(
            image::load_from_memory(&tray_resolved)
                .unwrap()
                .into_rgba8(),
            image::load_from_memory(&tray).unwrap().into_rgba8()
        );
        assert_eq!(
            image::load_from_memory(&widget_resolved)
                .unwrap()
                .into_rgba8(),
            image::load_from_memory(&widget).unwrap().into_rgba8()
        );
        set_icon(&paths, state, None).unwrap();
        set_icon(&paths, state, None).unwrap();
        assert_eq!(
            resolve(&paths, NetworkStatisticWidgetLogoSource::Widget, state),
            Logo::Builtin(LogoPreset::Tun)
        );
        assert!(matches!(
            resolve(&paths, NetworkStatisticWidgetLogoSource::Tray, state),
            Logo::Image(_)
        ));
    }

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
        set_icon_from_bytes(&paths, TrayIcon::SystemProxy, stored).unwrap();
        let saved = get_raw_icon(&paths, TrayIcon::SystemProxy).into_owned();
        assert_eq!(
            image::load_from_memory(&saved).unwrap().into_rgba8(),
            image::load_from_memory(stored).unwrap().into_rgba8()
        );
        assert!(set_icon_from_bytes(&paths, TrayIcon::SystemProxy, b"invalid").is_err());
        assert_eq!(get_raw_icon(&paths, TrayIcon::SystemProxy).as_ref(), saved);
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
