pub mod logging;

mod clash_strategy;

pub use self::clash_strategy::{ClashStrategy, ExternalControllerPortStrategy};

use anyhow::Result;
use chimera_macro::VergePatch;
use enumflags2::bitflags;
pub use logging::LoggingLevel;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::utils::{dirs, help};

// TODO: when support sing-box, remove this struct
#[bitflags]
#[repr(u8)]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Type)]
pub enum ClashCore {
    #[serde(rename = "clash", alias = "clash-premium")]
    ClashPremium = 0b0001,
    #[serde(rename = "clash-rs")]
    ClashRs,
    #[serde(rename = "mihomo", alias = "clash-meta")]
    Mihomo,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs-alpha")]
    ClashRsAlpha,
}

impl Default for ClashCore {
    fn default() -> Self {
        Self::Mihomo
    }
}

impl From<&ClashCore> for nyanpasu_utils::core::CoreType {
    fn from(core: &ClashCore) -> Self {
        match core {
            ClashCore::ClashPremium => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashPremium,
            ),
            ClashCore::ClashRs => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRust,
            ),
            ClashCore::Mihomo => {
                nyanpasu_utils::core::CoreType::Clash(nyanpasu_utils::core::ClashCoreType::Mihomo)
            }
            ClashCore::MihomoAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::MihomoAlpha,
            ),
            ClashCore::ClashRsAlpha => nyanpasu_utils::core::CoreType::Clash(
                nyanpasu_utils::core::ClashCoreType::ClashRustAlpha,
            ),
        }
    }
}

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
    /// 7. 是否使用内部的脚本支持，默认为真
    pub enable_builtin_enhanced: Option<bool>,
    /// 8. clash core path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clash_core: Option<ClashCore>,
    /// 9. 支持关闭字段过滤，避免meta的新字段都被过滤掉，默认为真
    pub enable_clash_fields: Option<bool>,
    /// 10. Clash 相关策略
    pub clash_strategy: Option<ClashStrategy>,
    /// 11. set system proxy bypass
    pub system_proxy_bypass: Option<String>,
    /// 12. verge mixed port 用于覆盖 clash 的 mixed port
    pub verge_mixed_port: Option<u16>,
    /// 13. enable proxy guard
    pub enable_proxy_guard: Option<bool>,
    /// 14. proxy guard interval
    #[serde(alias = "proxy_guard_duration")]
    pub proxy_guard_interval: Option<u64>,
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
            clash_core: Some(ClashCore::default()),
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
        if config.clash_core.is_none() {
            config.clash_core = template.clash_core;
        }

        config
    }

    /// Save IVerge App Config
    pub fn save_file(&self) -> Result<()> {
        help::save_yaml(
            &dirs::chimera_config_path()?,
            &self,
            Some("# Clash Chimera"),
        )
    }
}

/// Validates if a string is a valid hex color code
pub fn is_hex_color(color: &str) -> bool {
    if color.len() != 7 || !color.starts_with('#') {
        return false;
    }

    color[1..].chars().all(|c| c.is_ascii_hexdigit())
}
