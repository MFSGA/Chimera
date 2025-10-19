use anyhow::Result;

mod logging;

/// Initialize all the config files
/// before tauri setup
pub fn init_config() -> Result<()> {
    // init log
    logging::init().unwrap();

    Ok(())
}
