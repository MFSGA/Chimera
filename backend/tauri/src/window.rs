use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::config::{Config, chimera::WindowState};

#[cfg(windows)]
const WEBVIEW2_BROWSER_ARGS: &str = "--enable-features=msWebView2EnableDraggableRegions --disable-features=OverscrollHistoryNavigation,msExperimentalScrolling";
const RESTORE_PLACEHOLDER_SIZE: (f64, f64) = (800.0, 800.0);

#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Whether only one instance of this window type is allowed
    pub singleton: bool,
    /// Whether the window should be visible when created
    pub visible_on_create: bool,
    pub default_size: (f64, f64),
    pub min_size: Option<(f64, f64)>,
    pub center: bool,
    pub resizable: bool,
    pub always_on_top: Option<bool>,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            singleton: true,
            visible_on_create: true,
            default_size: (800.0, 636.0),
            min_size: Some((400.0, 600.0)),
            center: true,
            resizable: true,
            always_on_top: None,
        }
    }
}

impl WindowConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set whether only one instance is allowed
    pub fn singleton(mut self, singleton: bool) -> Self {
        self.singleton = singleton;
        self
    }

    /// Set whether window is visible on creation
    pub fn visible_on_create(mut self, visible: bool) -> Self {
        self.visible_on_create = visible;
        self
    }

    pub fn default_size(mut self, width: f64, height: f64) -> Self {
        self.default_size = (width, height);
        self
    }

    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some((width, height));
        self
    }

    pub fn center(mut self, center: bool) -> Self {
        self.center = center;
        self
    }
}

fn focus_existing_window(app_handle: &AppHandle, label: &str) -> bool {
    let Some(window) = app_handle.get_webview_window(label) else {
        return false;
    };

    crate::trace_err!(window.unminimize(), "set win unminimize");
    crate::trace_err!(window.show(), "set win visible");
    crate::trace_err!(window.set_focus(), "set win focus");
    true
}

fn resolve_always_on_top(config: &WindowConfig) -> bool {
    config.always_on_top.unwrap_or_else(|| {
        *Config::verge()
            .latest()
            .always_on_top
            .as_ref()
            .unwrap_or(&false)
    })
}

fn default_inner_size((width, height): (f64, f64)) -> (f64, f64) {
    #[cfg(target_os = "windows")]
    {
        (width, height)
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        (width, height + 6.0)
    }
}

fn clamp_window_size(
    window: &WebviewWindow,
    state: &WindowState,
    min_size: Option<(f64, f64)>,
) -> (u32, u32) {
    let mut width = state.width;
    let mut height = state.height;

    if let Some((min_width, min_height)) = min_size {
        let scale_factor = window.scale_factor().unwrap_or(1.0);
        let min_width = (min_width * scale_factor) as u32;
        let min_height = (min_height * scale_factor) as u32;
        width = width.max(min_width);
        height = height.max(min_height);
    }

    (width, height)
}

fn restore_window_state(
    window: &WebviewWindow,
    state: Option<&WindowState>,
    min_size: Option<(f64, f64)>,
) {
    let Some(state) = state else {
        return;
    };

    let (width, height) = clamp_window_size(window, state, min_size);

    crate::trace_err!(
        window.set_position(PhysicalPosition {
            x: state.x,
            y: state.y,
        }),
        "set win position"
    );
    crate::trace_err!(
        window.set_size(PhysicalSize { width, height }),
        "set win size"
    );

    if state.maximized {
        crate::trace_err!(window.maximize(), "set win maximize");
    }

    if state.fullscreen {
        crate::trace_err!(window.set_fullscreen(true), "set win fullscreen");
    }
}

fn should_center_window(window: &WebviewWindow, state: Option<&WindowState>) -> Result<bool> {
    let Some(state) = state else {
        return Ok(true);
    };

    let monitor = window
        .current_monitor()?
        .ok_or_else(|| anyhow!("monitor not found"))?;
    let PhysicalPosition { x, y } = *monitor.position();
    let PhysicalSize { width, height } = *monitor.size();
    let right = x + width as i32;
    let bottom = y + height as i32;

    let points = [
        (state.x, state.y),
        (state.x + state.width as i32, state.y),
        (state.x, state.y + state.height as i32),
        (state.x + state.width as i32, state.y + state.height as i32),
    ];

    Ok(!points
        .into_iter()
        .any(|(px, py)| px >= x && px < right && py >= y && py < bottom))
}

fn capture_window_state(window: &WebviewWindow) -> Result<Option<WindowState>> {
    if window.current_monitor()?.is_none() {
        return Ok(None);
    }

    let mut state = WindowState {
        maximized: window.is_maximized()?,
        fullscreen: window.is_fullscreen()?,
        ..WindowState::default()
    };
    let is_minimized = window.is_minimized()?;

    let size = window.inner_size()?;
    if size.width > 0 && size.height > 0 && !state.maximized && !is_minimized {
        state.width = size.width;
        state.height = size.height;
    }

    let position = window.outer_position()?;
    if !state.maximized && !is_minimized {
        state.x = position.x;
        state.y = position.y;
    }

    Ok(Some(state))
}

/// Trait for window management
pub trait AppWindow {
    fn label(&self) -> &str;
    fn title(&self) -> &str;
    fn url(&self) -> &str;

    fn config(&self) -> WindowConfig {
        WindowConfig::default()
    }

    fn get_window_state(&self) -> Option<WindowState>;
    fn set_window_state(&self, state: Option<WindowState>);

    fn create(&self, app_handle: &AppHandle) -> Result<()> {
        if focus_existing_window(app_handle, self.label()) {
            return Ok(());
        }

        let config = self.config();
        let window_state = self.get_window_state();
        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            self.label(),
            tauri::WebviewUrl::App(self.url().into()),
        )
        .title(self.title())
        .fullscreen(false)
        .always_on_top(resolve_always_on_top(&config))
        .resizable(config.resizable)
        .disable_drag_drop_handler();

        if let Some((width, height)) = config.min_size {
            builder = builder.min_inner_size(width, height);
        }

        if window_state.is_some() {
            builder = builder
                .inner_size(RESTORE_PLACEHOLDER_SIZE.0, RESTORE_PLACEHOLDER_SIZE.1)
                .position(0.0, 0.0);
        } else {
            let (width, height) = default_inner_size(config.default_size);
            builder = builder.inner_size(width, height);

            if config.center {
                builder = builder.center();
            }
        }

        #[cfg(windows)]
        let window = builder
            .decorations(false)
            .transparent(true)
            .visible(false)
            .additional_browser_args(WEBVIEW2_BROWSER_ARGS)
            .build();

        #[cfg(not(windows))]
        let window = builder.build();

        let window = match window {
            Ok(window) => window,
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                return Err(err.into());
            }
        };

        restore_window_state(&window, window_state.as_ref(), config.min_size);

        #[cfg(windows)]
        crate::trace_err!(window.set_shadow(true), "set win shadow");

        if should_center_window(&window, window_state.as_ref()).unwrap_or(true) {
            crate::trace_err!(window.center(), "set win center");
        }

        Ok(())
    }

    fn save_state(&self, app_handle: &AppHandle, save_to_file: bool) -> Result<()> {
        let window = app_handle
            .get_webview_window(self.label())
            .ok_or_else(|| anyhow!("failed to get window"))?;

        self.set_window_state(capture_window_state(&window)?);

        if save_to_file {
            Config::verge().data().save_file()?;
        }

        Ok(())
    }
}
