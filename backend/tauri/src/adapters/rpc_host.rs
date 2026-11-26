//! Desktop implementations of the shared RPC layer's host facilities.

use chimera_paths::PathResolver;
use chimera_rpc::ports::{CoreDirectoryResolver, ServiceInstallPrompt};

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
