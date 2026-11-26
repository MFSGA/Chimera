//! Desktop implementations of the shared RPC layer's host facilities.

use chimera_rpc::ports::CoreDirectoryResolver;

pub struct DesktopCoreDirectory;
impl CoreDirectoryResolver for DesktopCoreDirectory {
    fn core_directory(&self) -> anyhow::Result<String> {
        let core_dir = tauri::utils::platform::current_exe()?;
        let core_dir = core_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("failed to get core dir"))?;
        let core_dir = dunce::canonicalize(core_dir)?;
        Ok(core_dir.to_string_lossy().to_string())
    }
}
