//! Desktop implementations of the shared RPC layer's host facilities.

use chimera_paths::PathResolver;
use chimera_rpc::ports::{CoreDirectoryResolver, HostIconFiles, ServiceInstallPrompt};
use chimera_rpc_protocol::values::TrayIcon;

use crate::core::{tray::icon, widget_logo};

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

pub struct DesktopIconFiles;
#[async_trait::async_trait]
impl HostIconFiles for DesktopIconFiles {
    fn tray_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8> {
        icon::get_raw_icon(paths, mode).into_owned()
    }
    fn widget_icon(&self, paths: &PathResolver, mode: TrayIcon) -> Vec<u8> {
        widget_logo::get_raw_icon(paths, mode).into_owned()
    }
    async fn is_tray_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool {
        let icon_path = icon::tray_icons_path(paths, mode.as_str());
        tokio::fs::metadata(icon_path).await.is_ok()
    }
    fn is_widget_icon_set(&self, paths: &PathResolver, mode: TrayIcon) -> bool {
        widget_logo::is_icon_set(paths, mode)
    }
    fn set_tray_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let scale = crate::core::handle::Handle::app_handle()
            .as_ref()
            .map(crate::core::tray::Tray::max_scale_factor)
            .unwrap_or(1.0);
        icon::set_icon_from_bytes(paths, mode, bytes, scale)?;
        icon::check_icon(&icon::get_icon(paths, &mode))?;
        Ok(())
    }
    fn set_widget_icon_from_bytes(
        &self,
        paths: &PathResolver,
        mode: TrayIcon,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        widget_logo::set_icon_from_bytes(paths, mode, bytes)?;
        Ok(())
    }
}

pub struct OsServiceInstallPrompt;
#[async_trait::async_trait]
impl ServiceInstallPrompt for OsServiceInstallPrompt {
    async fn service_install_prompt(&self, paths: &PathResolver) -> anyhow::Result<String> {
        let app_dir = paths.app_install_dir()?;
        let args = chimera_os::service::get_service_install_args(
            paths.app_data_dir().as_std_path(),
            paths.app_config_dir().as_std_path(),
            app_dir.as_std_path(),
        )
        .await?
        .into_iter()
        .map(|arg| {
            #[cfg(unix)]
            {
                format!("'{}'", arg.to_string_lossy().replace('\'', "'\\''"))
            }
            #[cfg(windows)]
            {
                arg.to_string_lossy().to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
        let mut prompt = format!("./chimera-service {args}");
        if cfg!(not(windows)) {
            prompt = format!("sudo {prompt}");
        }
        Ok(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn service_prompt_uses_injected_paths_without_creating_directories() {
        let temp = tempfile::tempdir().unwrap();
        let root = camino::Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        let config = root.join("config with spaces");
        let data = root.join("data with spaces");
        let install = root.join("install with spaces");
        let paths = PathResolver::with_base_dirs_and_install_dir(
            config.clone(),
            data.clone(),
            Ok(install.clone()),
        );
        let prompt = OsServiceInstallPrompt
            .service_install_prompt(&paths)
            .await
            .unwrap();
        assert!(prompt.contains("./chimera-service"));
        for path in [&config, &data, &install] {
            assert!(prompt.contains(path.as_str()));
            assert!(!path.exists());
        }
        #[cfg(unix)]
        assert!(prompt.contains(&format!("'{}'", config)));
    }
}
