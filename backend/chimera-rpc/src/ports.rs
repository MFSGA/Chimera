//! Narrow host facilities consumed by the shared command layer.

use anyhow::Result;
use chimera_paths::PathResolver;
use chimera_rpc_protocol::values::TrayIcon;

/// Resolve the shell's executable directory with its platform-specific rules.
pub trait CoreDirectoryResolver: Send + Sync {
    fn core_directory(&self) -> Result<String>;
}

/// Host icon files; presentation refresh remains a client operation.
#[async_trait::async_trait]
pub trait HostIconFiles: Send + Sync {
    fn tray_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8>;
    fn widget_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8>;
    async fn is_tray_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool;
    fn is_widget_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool;
    fn set_tray_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> Result<()>;
}

/// Format installation instructions without installing or elevating.
#[async_trait::async_trait]
pub trait ServiceInstallPrompt: Send + Sync {
    async fn service_install_prompt(&self, paths: &PathResolver) -> Result<String>;
}

/// Process facts computed by the composing shell.
pub struct HostEnvironment {
    pub is_appimage: bool,
}
