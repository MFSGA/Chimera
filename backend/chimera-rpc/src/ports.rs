//! Narrow host facilities consumed by the shared command layer.

use anyhow::Result;
use chimera_paths::PathResolver;

/// Resolve the shell's executable directory with its platform-specific rules.
pub trait CoreDirectoryResolver: Send + Sync {
    fn core_directory(&self) -> Result<String>;
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
