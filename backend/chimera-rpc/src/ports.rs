//! Narrow host facilities consumed by the shared command layer.

use anyhow::Result;

/// Resolve the shell's executable directory with its platform-specific rules.
pub trait CoreDirectoryResolver: Send + Sync {
    fn core_directory(&self) -> Result<String>;
}

/// Process facts computed by the composing shell.
pub struct HostEnvironment {
    pub is_appimage: bool,
}
