pub mod logging;

use chimera_macro::VergePatch;
pub use logging::LoggingLevel;
use serde::{Deserialize, Serialize};

use crate::utils::{dirs, help};

#[derive(Default, Debug, Clone, specta::Type, Deserialize, Serialize, VergePatch)]
#[verge(patch_fn = "patch_config")]
// TODO: use new managedState and builder pattern instead
pub struct IVerge {
    /// 1. 日记轮转时间，单位：天
    pub max_log_files: Option<usize>,
    /// 2. app log level
    /// silent | error | warn | info | debug | trace
    pub app_log_level: Option<logging::LoggingLevel>,
    /// 3. theme setting
    pub theme_color: Option<String>,
    /// 4. clash tun mode
    pub enable_tun_mode: Option<bool>,
    /// 5. set system proxy
    pub enable_system_proxy: Option<bool>,
    /// 6. windows service mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_service_mode: Option<bool>,
}

impl IVerge {
    pub fn new() -> Self {
        match dirs::chimera_config_path().and_then(|path| help::read_yaml::<IVerge, _>(&path)) {
            Ok(mut config) => {
                // Validate and fix theme_color if it's invalid
                if let Some(ref theme_color) = config.theme_color {
                    if !theme_color.is_empty() && !is_hex_color(theme_color) {
                        log::warn!(target: "app", "Invalid theme color detected: {}, resetting to default", theme_color);
                        config.theme_color = None;
                    }
                }

                Self::merge_with_template(config)
            }
            Err(err) => {
                log::error!(target: "app", "{err:?}");
                Self::template()
            }
        }
    }

    pub fn template() -> Self {
        Self {
            max_log_files: Some(7), // 7 days
            app_log_level: Some(logging::LoggingLevel::default()),
            ..Self::default()
        }
    }

    fn merge_with_template(mut config: IVerge) -> Self {
        let template = Self::template();

        if config.max_log_files.is_none() {
            config.max_log_files = template.max_log_files;
        }

        config
    }
}

/// Validates if a string is a valid hex color code
pub fn is_hex_color(color: &str) -> bool {
    if color.len() != 7 || !color.starts_with('#') {
        return false;
    }

    color[1..].chars().all(|c| c.is_ascii_hexdigit())
}
