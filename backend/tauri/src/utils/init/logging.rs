use std::fs;

use anyhow::Result;

use crate::{config::chimera::logging::LoggingLevel, utils::dirs};

/// initial instance global logger
pub fn init() -> Result<()> {
    let log_dir = dirs::app_logs_dir().unwrap();
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }
    let (log_level, log_max_files) = { (LoggingLevel::Debug, 7) }; // This is intended to capture config loading errors

    todo!()
}
