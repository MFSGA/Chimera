use std::{fs, path::Path};

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp, profile::profiles::Profiles},
    utils::{dirs, help, path::PathResolver},
};

mod logging;
pub use logging::jobs_capture;
pub use logging::refresh_logger;

/// Initialize all the config files
/// before tauri setup
pub fn init_config(paths: &PathResolver) -> Result<()> {
    // Initialize the logger against the same resolved data root used by setup.
    logging::init(paths)?;
    tracing::info!("initializing logging config...");
    initialize_default_configs(paths)
}

fn initialize_default_configs(paths: &PathResolver) -> Result<()> {
    if let Err(error) = fs::create_dir_all(paths.app_profiles_dir()) {
        tracing::error!(%error, "failed to create profiles directory");
    }

    crate::log_err!(initialize_default_yaml(
        &paths.clash_guard_overrides_path(),
        &IClashTemp::template().0,
    ));
    crate::log_err!(initialize_default_yaml(
        &paths.chimera_config_path(),
        &IVerge::template(),
    ));
    crate::log_err!(initialize_default_yaml(
        &paths.profiles_path(),
        &Profiles::default(),
    ));

    Ok(())
}

/// A dangling symlink is an existing config node, not an empty installation.
/// This runs before the typed migration and must not erase links or evidence.
fn initialize_default_yaml<T: Serialize>(path: &Path, template: &T) -> Result<()> {
    if !crate::core::corrupt::config_node_exists(path)
        .with_context(|| format!("failed to inspect startup config {}", path.display()))?
    {
        help::save_yaml(path, template, Some("# Clash Chimera"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

/// initialize app resources
/// after tauri setup
pub fn init_resources() -> Result<()> {
    let app_dir = dirs::app_data_dir()?;
    let res_dir = dirs::app_resources_dir()?;

    if !app_dir.exists() {
        let _ = fs::create_dir_all(&app_dir);
    }
    if !res_dir.exists() {
        let _ = fs::create_dir_all(&res_dir);
    }

    #[cfg(target_os = "windows")]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat", "wintun.dll"];
    #[cfg(not(target_os = "windows"))]
    let file_list = ["Country.mmdb", "geoip.dat", "geosite.dat"];

    // copy the resource file
    // if the source file is newer than the destination file, copy it over
    for file in file_list.iter() {
        let src_path = res_dir.join(file);
        let dest_path = app_dir.join(file);

        let handle_copy = || {
            match fs::copy(&src_path, &dest_path) {
                Ok(_) => log::debug!(target: "app", "resources copied '{file}'"),
                Err(err) => {
                    log::error!(target: "app", "failed to copy resources '{file}', {err:?}")
                }
            };
        };

        if src_path.exists() && !dest_path.exists() {
            handle_copy();
            continue;
        }

        let src_modified = fs::metadata(&src_path).and_then(|m| m.modified());
        let dest_modified = fs::metadata(&dest_path).and_then(|m| m.modified());

        match (src_modified, dest_modified) {
            (Ok(src_modified), Ok(dest_modified)) => {
                if src_modified > dest_modified {
                    handle_copy();
                } else {
                    log::debug!(target: "app", "skipping resource copy '{file}'");
                }
            }
            _ => {
                log::debug!(target: "app", "failed to get modified '{file}'");
                handle_copy();
            }
        };
    }

    Ok(())
}

/// initialize service resources
/// after tauri setup
#[tracing::instrument(skip(client))]
pub fn init_service(client: crate::client::ChimeraClient) -> Result<()> {
    use chimera_utils::runtime::block_on;

    tracing::debug!("init services");
    block_on(async move {
        let enable_service = {
            *crate::config::core::Config::verge()
                .latest()
                .enable_service_mode
                .as_ref()
                .unwrap_or(&false)
        };
        if enable_service {
            match crate::core::service::control::probe_service_status().await {
                Ok(status) => {
                    tracing::info!(
                        "service mode is enabled and service is running, do a update check"
                    );
                    if let Some(info) = status.server
                        && let (Some(server_ver), Some(app_ver)) = (
                            chimera_core::service::compat::parse_service_version(
                                info.version.as_ref(),
                            ),
                            chimera_core::service::compat::parse_service_version(
                                status.version.as_ref(),
                            ),
                        )
                    {
                        if app_ver > server_ver {
                            tracing::info!(
                                "client service ver is newer than exist one, do service update"
                            );
                            if let Err(e) = client.update_service().await {
                                log::error!(target: "app", "failed to update service: {e:?}");
                            }
                        }
                    } else {
                        tracing::warn!(
                            "service version not comparable; skipping the auto-update check"
                        );
                    }
                }
                Err(e) => {
                    log::error!(target: "app", "failed to get service status: {e:?}");
                }
            }
        }
        crate::core::service::init_service(client).await;
    });
    Ok(())
}
