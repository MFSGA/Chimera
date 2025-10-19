pub mod logging;

pub use logging::LoggingLevel;
use serde::Deserialize;

use crate::utils::{dirs, help};

#[derive(Default, Debug, Clone, specta::Type, Deserialize)]
// TODO: use new managedState and builder pattern instead
pub struct IVerge {
    /// 1. 日记轮转时间，单位：天
    pub max_log_files: Option<usize>,
    /// 2. app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: Option<logging::LoggingLevel>,
}

impl IVerge {
    pub fn new() -> Self {
        match dirs::chimera_config_path().and_then(|path| help::read_yaml::<IVerge, _>(&path)) {
            Ok(mut config) => {
                todo!()
                // Validate and fix theme_color if it's invalid
                /* if let Some(ref theme_color) = config.theme_color {
                    if !theme_color.is_empty() && !is_hex_color(theme_color) {
                        log::warn!(target: "app", "Invalid theme color detected: {}, resetting to default", theme_color);
                        config.theme_color = None;
                    }
                }

                Self::merge_with_template(config) */
            }
            Err(err) => {
                log::error!(target: "app", "{err:?}");
                Self::template()
            }
        }
    }

    pub fn template() -> Self {
        Self {
            /// 1
            max_log_files: Some(7), // 7 days
            /// 2
            app_log_level: Some(logging::LoggingLevel::default()),
        }
    }
}
