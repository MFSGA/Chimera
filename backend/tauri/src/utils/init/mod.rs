use std::fs;

use anyhow::Result;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp, profile::profiles::Profiles},
    utils::{dirs, help},
};

mod logging;
pub use logging::refresh_logger;

/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    // init log
    logging::init().unwrap();
    tracing::info!("initializing logging config...");
    crate::log_err!(dirs::app_profiles_dir().map(|profiles_dir| {
        if !profiles_dir.exists() {
            let _ = fs::create_dir_all(&profiles_dir);
        }
    }));

    crate::log_err!(dirs::clash_guard_overrides_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &IClashTemp::template().0, Some("# Clash Chimera"))?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::chimera_config_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &IVerge::template(), Some("# Clash Chimera"))?;
        }
        <Result<()>>::Ok(())
    }));

    crate::log_err!(dirs::profiles_path().map(|path| {
        if !path.exists() {
            help::save_yaml(&path, &Profiles::default(), Some("# Clash Chimera"))?;
        }
        <Result<()>>::Ok(())
    }));

    Ok(())
}

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
#[tracing::instrument]
pub fn init_service() -> Result<()> {
    use nyanpasu_utils::runtime::block_on;

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
            match crate::core::service::control::status().await {
                Ok(status) => {
                    tracing::info!(
                        "service mode is enabled and service is running, do a update check"
                    );
                    if let Some(info) = status.server {
                        let server_ver = semver::Version::parse(info.version.as_ref()).unwrap();
                        let app_ver = semver::Version::parse(status.version.as_ref()).unwrap();
                        if app_ver > server_ver {
                            tracing::info!(
                                "client service ver is newer than exist one, do service update"
                            );
                            if let Err(e) = crate::core::service::control::update_service().await {
                                log::error!(target: "app", "failed to update service: {e:?}");
                            }
                        }
                    }
                }
                Err(e) => {
                    log::error!(target: "app", "failed to get service status: {e:?}");
                }
            }
        }
        crate::core::service::init_service().await;
    });
    Ok(())
}
