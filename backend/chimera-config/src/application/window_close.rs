use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

/// What closing a window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseBehavior {
    /// Destroy the window and its webview, freeing their memory.
    #[default]
    Destroy,
    /// Keep the window and its webview, hidden, so reopening is instant.
    Hide,
}

/// One window kind's choice: follow the global behavior or set its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowCloseOverride {
    #[default]
    Inherit,
    Destroy,
    Hide,
}

impl WindowCloseOverride {
    pub fn resolve(self, global: WindowCloseBehavior) -> WindowCloseBehavior {
        match self {
            Self::Inherit => global,
            Self::Destroy => WindowCloseBehavior::Destroy,
            Self::Hide => WindowCloseBehavior::Hide,
        }
    }
}

/// How each kind of window closes: a global behavior that every kind follows
/// unless it sets its own.
///
/// A patch names only the fields it changes, so an edit of one field, made
/// from a page that last saw the others a moment ago, leaves them alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, Type)))]
#[patch(attribute(serde(default)))]
#[serde(default)]
pub struct WindowCloseSettings {
    pub global: WindowCloseBehavior,
    pub main: WindowCloseOverride,
    pub editor: WindowCloseOverride,
    pub tray_menu: WindowCloseOverride,
}
