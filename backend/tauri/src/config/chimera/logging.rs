use serde::{Deserialize, Serialize};
use strum::Display;
use tracing_subscriber::filter;

#[derive(Deserialize, Serialize, Debug, Clone, specta::Type, Display)]
pub enum LoggingLevel {
    #[serde(rename = "silent", alias = "off")]
    Silent,
    #[serde(rename = "trace", alias = "tracing")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warn", alias = "warning")]
    Warn,
    #[serde(rename = "error")]
    Error,
}

impl From<LoggingLevel> for filter::LevelFilter {
    fn from(level: LoggingLevel) -> Self {
        match level {
            LoggingLevel::Silent => filter::LevelFilter::OFF,
            LoggingLevel::Trace => filter::LevelFilter::TRACE,
            LoggingLevel::Debug => filter::LevelFilter::DEBUG,
            LoggingLevel::Info => filter::LevelFilter::INFO,
            LoggingLevel::Warn => filter::LevelFilter::WARN,
            LoggingLevel::Error => filter::LevelFilter::ERROR,
        }
    }
}
