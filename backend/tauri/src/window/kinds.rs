//! The kinds of window Chimera opens. WindowManager owns their lifecycle,
//! while these definitions keep the URL, appearance, and per-kind close policy.
//! Based on ref/backend/tauri/src/window/kinds.rs at 59724e85c.

use super::{
    AppWindow, WindowConfig, WindowParams, WindowParamsBuilder, build_url_with_params,
    close::{WindowCloseOverride, WindowCloseSettings},
};
use crate::config::{Config, chimera::WindowState};
use anyhow::{Result, anyhow};
use tauri::{AppHandle, Manager};

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
