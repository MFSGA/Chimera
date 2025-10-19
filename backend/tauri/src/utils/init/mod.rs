use std::fs;

use anyhow::Result;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp, profile::profiles::Profiles},
    utils::{dirs, help},
};

mod logging;

/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    // init log
    logging::init().unwrap();

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
