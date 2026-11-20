use std::{fs, path::Path, path::PathBuf};

use anyhow::{Context as _, Result};
use serde::Serialize;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp},
    utils::{help, path::PathResolver},
};

mod logging;
pub use logging::jobs_capture;
pub use logging::refresh_logger;

/// Initialize logging after the primary instance is claimed but before
/// migrations. This must not read or synthesize legacy config documents.
pub fn init_logging(paths: &PathResolver) -> Result<()> {
    logging::init(paths)
}

/// Only called after the migration Runner has committed successfully.
/// Initializing legacy defaults before migration changes baseline detection
/// and the pre-migration backup's set of original files.
pub fn initialize_post_migration_defaults(paths: &PathResolver) -> Result<()> {
    tracing::info!("initializing compatibility defaults after migration");
    initialize_default_configs(paths)
}

/// Read the migrated app's log preferences only after migration. Delaying
/// `Config::verge()` also prevents its global legacy cache from being seeded
/// before the typed documents have been upgraded.
pub fn refresh_migrated_logging() {
    logging::refresh_from_config();
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
    // A stamped Profiles document is required. Do not silently boot with
    // migration state ahead of a missing or unwritable document.
    initialize_stamped_profiles_default(&paths.profiles_path())?;

    Ok(())
}

/// Default Profiles are a typed document, not a legacy unstamped YAML.
/// The migration state has already advanced to revision 4, so writing the
/// pre-migration `current: []`/`chain: []` shape here would make the next
/// startup reject it as a document behind that state.
fn initialize_stamped_profiles_default(path: &Path) -> Result<()> {
    use chimera_persistence::format::StampedDocument as _;
    if crate::core::corrupt::config_node_exists(path)
        .with_context(|| format!("failed to inspect startup config {}", path.display()))?
    {
        return Ok(());
    }
    let payload = serde_yaml_ng::to_value(chimera_config::profile::Profiles::default())?;
    let serde_yaml_ng::Value::Mapping(payload) = payload else {
        anyhow::bail!("default typed profiles must serialize as a YAML mapping");
    };
    chimera_migration::document::write_document(
        path,
        chimera_migration::modules::profiles::ProfilesDocument::DOCUMENT,
        chimera_migration::modules::profiles::ProfilesDocument::SCHEMA_REVISION,
        payload,
        Some("# Clash Chimera"),
    )
}

/// A dangling symlink is an existing config node, not an empty installation.
/// Defaults run *after* typed migration and must never erase links or evidence.
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

/// The bundled `resources` directory, which only Tauri can locate.
pub fn bundled_resources_dir(manager: &impl tauri::Manager<tauri::Wry>) -> tauri::Result<PathBuf> {
    Ok(manager.path().resource_dir()?.join("resources"))
}

/// Initialize app resources after Tauri setup, using the already-resolved data root.
pub fn init_resources(app_dir: &Path, res_dir: Option<&Path>) -> Result<()> {
    let res_dir = res_dir.ok_or_else(|| anyhow::anyhow!("failed to locate bundled resources"))?;

    if !app_dir.exists() {
        let _ = fs::create_dir_all(app_dir);
    }
    if !res_dir.exists() {
        let _ = fs::create_dir_all(res_dir);
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
pub async fn init_service(client: crate::client::ChimeraClient) -> Result<()> {
    tracing::debug!("init services");
    let check_service_update = client
        .get_app_config()
        .context("failed to read typed Service Mode preference during startup")?
        .enable_service_mode;
    if check_service_update {
        match crate::core::service::control::probe_service_status().await {
            Ok(status) => {
                tracing::info!("service mode is enabled and service is running, do a update check");
                let service_version = status.server.as_ref().and_then(|info| {
                    chimera_core::service::compat::parse_service_version(info.version.as_ref())
                });
                let app_version =
                    chimera_core::service::compat::parse_service_version(status.version.as_ref());
                match (service_version, app_version) {
                    (Some(server_ver), Some(app_ver)) if app_ver > server_ver => {
                        tracing::info!(
                            "client service ver is newer than exist one, do service update"
                        );
                        if let Err(error) =
                            crate::core::service::update_service_for_startup(&client).await
                        {
                            log::error!(target: "app", "failed to update service: {error:?}");
                        }
                    }
                    (Some(_), Some(_)) => {}
                    _ => tracing::warn!(
                        "service version not comparable; skipping the auto-update check"
                    ),
                }
            }
            Err(error) => {
                log::error!(target: "app", "failed to get service status: {error:?}");
            }
        }
    }
    crate::core::service::init_service(client).await?;
    Ok(())
}
