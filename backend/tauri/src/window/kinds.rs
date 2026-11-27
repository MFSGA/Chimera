//! The kinds of window Chimera opens. WindowManager owns their lifecycle,
//! while these definitions keep the URL, appearance, and per-kind close policy.
//! Based on ref/backend/tauri/src/window/kinds.rs at 59724e85c.

use super::{
    AppWindow, WindowConfig, WindowManager, WindowParams, WindowParamsBuilder,
    build_url_with_params,
    close::{WindowCloseOverride, WindowCloseSettings},
};
use crate::config::{Config, chimera::WindowState};
use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent};

/// Decides when the webview tray menu is dismissed on focus loss, from the
/// window's focus events alone.
#[derive(Debug, Default)]
struct TrayMenuFocus {
    /// Set for the debug menu window, which stays open on focus loss.
    persistent: bool,
    /// Set to true only after the window has received Focused(true) at least once.
    /// Prevents spurious Focused(false) events during window creation from triggering
    /// hide/close before the user has ever seen the window.
    ready: bool,
}

impl TrayMenuFocus {
    /// The menu was shown at the cursor.
    fn shown(&mut self) {
        self.persistent = false;
        self.ready = false;
    }

    /// The debug menu window was opened.
    fn shown_persistent(&mut self) {
        self.persistent = true;
    }

    fn focused(&mut self) {
        self.ready = true;
    }

    /// Whether this focus loss dismisses the menu.
    fn blurred(&mut self) -> bool {
        if !self.persistent && self.ready {
            self.ready = false;
            return true;
        }
        false
    }
}

/// The tray menu window's focus state, managed as Tauri state by the
/// composition root.
#[derive(Debug, Default)]
pub struct TrayMenuWindowController {
    focus: parking_lot::Mutex<TrayMenuFocus>,
}

impl TrayMenuWindowController {
    fn shown(&self) {
        self.focus.lock().shown();
    }

    fn shown_persistent(&self) {
        self.focus.lock().shown_persistent();
    }

    fn focused(&self) {
        self.focus.lock().focused();
    }

    fn blurred(&self) -> bool {
        self.focus.lock().blurred()
    }
}

/// Both Chimera UIs share the same persisted main-window geometry. The typed
/// SessionState snapshot is authoritative; use the legacy Verge state only
/// before the client is registered in Tauri's state container.
fn initial_main_window_state(app_handle: &AppHandle) -> Option<WindowState> {
    match app_handle.try_state::<crate::client::ChimeraClient>() {
        Some(client) => client.main_window_geometry().map(legacy_window_state),
        None => Config::verge().latest().window_size_state.clone(),
    }
}

fn legacy_window_state(state: chimera_config::state::window::WindowState) -> WindowState {
    WindowState {
        width: state.width,
        height: state.height,
        x: state.x,
        y: state.y,
        maximized: state.maximized,
        fullscreen: state.fullscreen,
    }
}

#[derive(Clone)]
pub struct MainWindow;

impl AppWindow for MainWindow {
    fn label(&self) -> &str {
        crate::consts::MAIN_WINDOW_LABEL
    }
    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }
    fn url(&self) -> &str {
        "/main"
    }
    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }
    fn get_window_state(&self, app_handle: &AppHandle) -> Option<WindowState> {
        initial_main_window_state(app_handle)
    }
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.main
    }
    fn on_dismissed(&self, window: &tauri::WebviewWindow) {
        crate::log_err!(crate::window::queue_window_state_save(window));
    }
}

/// Chimera's retained original UI; it uses the same manager and application
/// client as MainWindow, rather than owning an alternative business lifecycle.
#[derive(Clone)]
pub struct LegacyWindow;

impl AppWindow for LegacyWindow {
    fn label(&self) -> &str {
        crate::consts::LEGACY_WINDOW_LABEL
    }
    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }
    fn url(&self) -> &str {
        "/"
    }
    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .default_size(800.0, 636.0)
            .min_size(400.0, 600.0)
            .center(true)
    }
    fn get_window_state(&self, app_handle: &AppHandle) -> Option<WindowState> {
        initial_main_window_state(app_handle)
    }
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.main
    }
    fn on_dismissed(&self, window: &tauri::WebviewWindow) {
        crate::log_err!(crate::window::queue_window_state_save(window));
    }
}

/// Ref-compatible editor kind with Chimera's existing labels and routes.
#[derive(Clone, Debug)]
pub struct EditorWindow {
    label: String,
    title: &'static str,
    url: &'static str,
}

#[derive(Clone, Copy, Debug, serde::Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum EditorWindowType {
    Profile,
    CssEditor,
}

impl EditorWindow {
    pub fn for_type(
        window_type: EditorWindowType,
        uid: Option<&str>,
    ) -> Result<(Self, Option<WindowParams>)> {
        match window_type {
            EditorWindowType::Profile => {
                let uid = uid.ok_or_else(|| anyhow!("uid required for profile editor"))?;
                Ok((
                    Self {
                        label: format!("profile-editor-{uid}"),
                        title: "Profile Editor",
                        url: "/editor/profile",
                    },
                    WindowParamsBuilder::new().param("uid", uid).build(),
                ))
            }
            EditorWindowType::CssEditor => Ok((
                Self {
                    label: "editor-css".into(),
                    title: "Clash Chimera - CSS Editor",
                    url: "/editor/css",
                },
                None,
            )),
        }
    }
}

impl AppWindow for EditorWindow {
    fn label(&self) -> &str {
        &self.label
    }
    fn title(&self) -> &str {
        self.title
    }
    fn url(&self) -> &str {
        self.url
    }
    fn url_with_params(&self, params: Option<&WindowParams>) -> String {
        build_url_with_params(self.url(), params)
    }
    fn config(&self) -> WindowConfig {
        match self.url {
            "/editor/profile" => WindowConfig::new()
                .singleton(true)
                .default_size(960.0, 680.0)
                .min_size(600.0, 400.0)
                .center(true),
            _ => WindowConfig::new()
                .singleton(true)
                .default_size(800.0, 636.0)
                .min_size(400.0, 500.0)
                .center(true),
        }
    }
    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        None
    }
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.editor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_main_geometry_maps_to_existing_window_state_shape() {
        let typed = chimera_config::state::window::WindowState {
            width: 1180,
            height: 790,
            x: -22,
            y: 140,
            maximized: true,
            fullscreen: false,
        };
        let legacy = legacy_window_state(typed);
        assert_eq!((legacy.width, legacy.height), (1180, 790));
        assert_eq!((legacy.x, legacy.y), (-22, 140));
        assert!(legacy.maximized);
        assert!(!legacy.fullscreen);
    }

    #[test]
    fn retained_editor_urls_and_labels_are_unchanged() {
        let (profile, params) =
            EditorWindow::for_type(EditorWindowType::Profile, Some("a/b")).unwrap();
        assert_eq!(profile.label(), "profile-editor-a/b");
        assert_eq!(
            profile.url_with_params(params.as_ref()),
            "/editor/profile?uid=a%2Fb"
        );
        let (css, params) = EditorWindow::for_type(EditorWindowType::CssEditor, None).unwrap();
        assert_eq!(css.label(), "editor-css");
        assert_eq!(css.url_with_params(params.as_ref()), "/editor/css");
        assert!(EditorWindow::for_type(EditorWindowType::Profile, None).is_err());

        let (profile_with_space, params) =
            EditorWindow::for_type(EditorWindowType::Profile, Some("my profile&1")).unwrap();
        assert_eq!(
            profile_with_space.url_with_params(params.as_ref()),
            "/editor/profile?uid=my%20profile%261"
        );
    }
}

const TRAY_MENU_WIDTH: f64 = 240.0;
const TRAY_MENU_HEIGHT: f64 = if cfg!(target_os = "linux") {
    504.0
} else {
    448.0
};

/// Webview tray menu window
#[derive(Clone)]
struct TrayMenuWindow {
    /// Where a window built for this request is placed, before it can be shown.
    cursor: Option<PhysicalPosition<f64>>,
}

impl AppWindow for TrayMenuWindow {
    fn label(&self) -> &str {
        crate::consts::TRAY_MENU_WINDOW_LABEL
    }

    fn title(&self) -> &str {
        crate::consts::APP_NAME
    }

    fn url(&self) -> &str {
        "/tray-menu"
    }

    fn config(&self) -> WindowConfig {
        WindowConfig::new()
            .singleton(true)
            .default_size(TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT)
            .min_size(TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT)
            .center(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            // macOS builds it titled, like the main window, so the OS gives
            // it the same rounded corners and shadow; its traffic lights are
            // hidden on creation. A borderless window there has square
            // corners.
            .decorations(cfg!(target_os = "macos"))
            // Its focus loss dismisses it, so only the focus it gets when it
            // is shown may count as its first.
            .focused_on_create(false)
    }

    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        None
    }

    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.tray_menu
    }

    fn on_created(&self, window: &WebviewWindow) {
        #[cfg(target_os = "macos")]
        hide_traffic_lights(window);
        if let Some(cursor) = self.cursor {
            place_tray_menu_window(window, cursor);
        }
    }

    /// Closes the menu on focus loss, as [`TrayMenuFocus`] decides.
    fn on_window_event(&self, window: &WebviewWindow, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(true) => {
                window.state::<TrayMenuWindowController>().focused();
            }
            WindowEvent::Focused(false) if window.state::<TrayMenuWindowController>().blurred() => {
                window.state::<WindowManager>().close(window.label());
            }
            _ => {}
        }
    }
}

/// Hides the close, minimize and zoom buttons of a titled window. Queued on
/// the main thread, so it runs before the window is shown, which is queued
/// after it.
#[cfg(target_os = "macos")]
fn hide_traffic_lights(win: &WebviewWindow) {
    use objc2_app_kit::{NSWindow, NSWindowButton};

    let window = win.clone();
    let _ = win.run_on_main_thread(move || {
        let Ok(ns_window) = window.ns_window() else {
            return;
        };
        // SAFETY: `ns_window` is the live NSWindow of `window`, and this runs
        // on the main thread.
        let ns_window = unsafe { &*ns_window.cast::<NSWindow>() };
        for kind in [
            NSWindowButton::CloseButton,
            NSWindowButton::MiniaturizeButton,
            NSWindowButton::ZoomButton,
        ] {
            if let Some(button) = ns_window.standardWindowButton(kind) {
                button.setHidden(true);
            }
        }
    });
}

/// Open a persistent tray menu window for debugging.
pub fn create_debug_tray_menu_window(app_handle: &AppHandle) -> Result<()> {
    app_handle
        .state::<TrayMenuWindowController>()
        .shown_persistent();

    let params = WindowParamsBuilder::new()
        .param("persistent", "true")
        .build();
    app_handle
        .state::<WindowManager>()
        .open(&TrayMenuWindow { cursor: None }, params)?;

    Ok(())
}

/// Show the webview tray menu window near the given cursor position.
pub fn show_tray_menu_window(app_handle: &AppHandle, cursor: PhysicalPosition<f64>) -> Result<()> {
    app_handle.state::<TrayMenuWindowController>().shown();

    // A window that is shown again is moved before it can be shown, so it
    // never appears at its old position first. A new one is placed as part of
    // its creation.
    if let Some(existing) = app_handle.get_webview_window(crate::consts::TRAY_MENU_WINDOW_LABEL) {
        place_tray_menu_window(&existing, cursor);
    }
    app_handle.state::<WindowManager>().open(
        &TrayMenuWindow {
            cursor: Some(cursor),
        },
        None,
    )?;

    Ok(())
}

/// Moves the tray menu window next to the cursor, inside the work area of the
/// monitor the cursor is on.
fn place_tray_menu_window(win: &WebviewWindow, cursor: PhysicalPosition<f64>) {
    let (menu_w, menu_h) = win
        .outer_size()
        .map(|size| {
            let width = if size.width == 0 {
                TRAY_MENU_WIDTH
            } else {
                size.width as f64
            };
            let height = if size.height == 0 {
                TRAY_MENU_HEIGHT
            } else {
                size.height as f64
            };
            (width, height)
        })
        .unwrap_or((TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT));
    let margin = 8.0_f64;

    let (screen_x, screen_y, screen_w, screen_h) = win
        .available_monitors()
        .ok()
        .and_then(|monitors| {
            monitors
                .iter()
                .find(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    let left = position.x as f64;
                    let top = position.y as f64;
                    let right = left + size.width as f64;
                    let bottom = top + size.height as f64;

                    cursor.x >= left && cursor.x < right && cursor.y >= top && cursor.y < bottom
                })
                .or_else(|| monitors.first())
                .map(|monitor| {
                    let work_area = monitor.work_area();
                    let size = if work_area.size.width == 0 || work_area.size.height == 0 {
                        *monitor.size()
                    } else {
                        work_area.size
                    };
                    let position = if work_area.size.width == 0 || work_area.size.height == 0 {
                        *monitor.position()
                    } else {
                        work_area.position
                    };

                    (
                        position.x as f64,
                        position.y as f64,
                        size.width as f64,
                        size.height as f64,
                    )
                })
        })
        .unwrap_or((0.0, 0.0, 1920.0, 1080.0));

    let left = screen_x + margin;
    let top = screen_y + margin;
    let right = screen_x + screen_w - margin;
    let bottom = screen_y + screen_h - margin;

    let max_x = (right - menu_w).max(left);
    let max_y = (bottom - menu_h).max(top);
    let mut x = if cursor.x + menu_w > right {
        cursor.x - menu_w
    } else {
        cursor.x
    };
    let mut y = if cursor.y + menu_h > bottom {
        cursor.y - menu_h
    } else {
        cursor.y
    };

    x = x.max(left).min(max_x);
    y = y.max(top).min(max_y);

    let _ = win.set_position(PhysicalPosition {
        x: x as i32,
        y: y as i32,
    });
}

#[cfg(test)]
mod tray_menu_tests {
    use super::*;
    #[test]
    fn the_tray_menu_takes_no_focus_while_it_is_built_hidden() {
        assert!(!TrayMenuWindow { cursor: None }.config().focused_on_create);
    }

    #[test]
    fn a_blur_before_the_first_focus_keeps_the_menu() {
        let mut focus = TrayMenuFocus::default();
        focus.shown();
        assert!(!focus.blurred(), "never focused");
    }

    #[test]
    fn a_blur_after_focus_dismisses_the_menu() {
        let mut focus = TrayMenuFocus::default();
        focus.shown();
        focus.focused();
        assert!(focus.blurred());
    }

    #[test]
    fn a_dismissal_waits_for_the_next_focus() {
        let mut focus = TrayMenuFocus::default();
        focus.shown();
        focus.focused();
        assert!(focus.blurred());
        assert!(!focus.blurred());

        focus.focused();
        assert!(focus.blurred());
    }

    #[test]
    fn the_debug_menu_stays_open_until_shown_normally() {
        let mut focus = TrayMenuFocus::default();
        focus.shown_persistent();
        focus.focused();
        assert!(!focus.blurred());

        focus.shown();
        assert!(!focus.blurred(), "showing again waits for a new focus");
        focus.focused();
        assert!(focus.blurred());
    }
}
