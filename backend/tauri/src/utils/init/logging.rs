use std::fs;

use anyhow::Result;
use tracing_appender::{
    non_blocking::{NonBlocking, WorkerGuard},
    rolling::Rotation,
};
use tracing_subscriber::{EnvFilter, filter, fmt, layer::SubscriberExt, reload};

use crate::{config::chimera::logging::LoggingLevel, utils::dirs};

/// initial instance global logger
pub fn init() -> Result<()> {
    let log_dir = dirs::app_logs_dir().unwrap();
    if !log_dir.exists() {
        let _ = fs::create_dir_all(&log_dir);
    }
    let (log_level, log_max_files) = { (LoggingLevel::Debug, 7) }; // This is intended to capture config loading errors
    let (filter, filter_handle) = reload::Layer::new(
        EnvFilter::builder()
            .with_default_directive(
                std::convert::Into::<filter::LevelFilter>::into(LoggingLevel::Warn).into(),
            )
            .from_env_lossy()
            .add_directive(format!("nyanpasu={log_level}").parse().unwrap())
            .add_directive(format!("clash_nyanpasu={log_level}").parse().unwrap()),
    );

    // register the logger
    let (appender, _guard) = get_file_appender(log_max_files)?;
    let (file_layer, file_handle) = reload::Layer::new(
        fmt::layer()
            .json()
            .with_writer(appender)
            .with_current_span(true)
            .with_line_number(true)
            .with_file(true),
    );

    todo!();
    let subscriber = tracing_subscriber::registry().with(filter).with(file_layer);

    todo!()
}

fn get_file_appender(max_files: usize) -> Result<(NonBlocking, WorkerGuard)> {
    let log_dir = dirs::app_logs_dir().unwrap();
    let file_appender = tracing_appender::rolling::Builder::new()
        .filename_prefix("clash-nyanpasu")
        .filename_suffix("app.log")
        .rotation(Rotation::DAILY)
        .max_log_files(max_files)
        .build(log_dir)?;
    Ok(tracing_appender::non_blocking(file_appender))
}
