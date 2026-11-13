//! The kinds of window Chimera opens. WindowManager owns their lifecycle,
//! while these definitions keep the URL, appearance, and per-kind close policy.
//! Based on ref/backend/tauri/src/window/kinds.rs at 59724e85c.

use super::{
    AppWindow, WindowConfig, WindowParams, WindowParamsBuilder, build_url_with_params,
    close::{WindowCloseOverride, WindowCloseSettings},
};
use crate::config::{Config, chimera::WindowState};
use anyhow::{Result, anyhow};
use tauri::AppHandle;

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
    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        Config::verge().latest().window_size_state.clone()
    }
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.main
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
    fn get_window_state(&self, _app_handle: &AppHandle) -> Option<WindowState> {
        Config::verge().latest().window_size_state.clone()
    }
    fn close_override(&self, settings: &WindowCloseSettings) -> WindowCloseOverride {
        settings.main
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
