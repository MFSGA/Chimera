use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

mod clash_core;
mod i18n;
mod logging;

pub use clash_core::*;
pub use i18n::*;
pub use logging::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProxiesSelectorMode {
    Hidden,
    #[default]
    Normal,
    Submenu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    Light,
    Dark,
    #[default]
    System,
}

/// Chimera-specific dual-UI selector retained as a real product difference from REF.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum WindowType {
    Main,
    #[default]
    Legacy,
}

/// Typed application-owned subset of the legacy `verge.yaml` schema.
///
/// Clash-owned fields live in `clash::config::ClashConfig`; window geometry
/// remains session-state compatibility until the dual-UI state migration lands.
#[derive(Debug, Clone, Deserialize, Serialize, Type, Patch)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
pub struct ChimeraAppConfig {
    pub app_log_level: LoggingLevel,
    pub language: I18nLanguage,
    pub theme_mode: ThemeMode,
    pub lighten_animation_effects: bool,
    pub enable_service_mode: bool,
    pub enable_auto_launch: bool,
    pub enable_silent_start: bool,
    pub enable_system_proxy: bool,
    pub enable_proxy_guard: bool,
    pub system_proxy_bypass: String,
    #[patch(attribute(serde(alias = "proxy_guard_duration")))]
    pub proxy_guard_interval: u64,
    #[specta(type = String)]
    #[patch(attribute(specta(type = String)))]
    pub theme_color: CssColor,
    #[patch(attribute(serde(alias = "clash_core")))]
    pub core: ClashCore,
    pub enable_builtin_enhanced: bool,
    pub max_log_files: usize,
    pub enable_auto_check_update: bool,
    #[patch(attribute(serde(alias = "clash_tray_selector")))]
    pub tray_selector_mode: ProxiesSelectorMode,
    pub always_on_top: bool,
    pub window_type: WindowType,
}

impl Default for ChimeraAppConfig {
    fn default() -> Self {
        Self {
            app_log_level: LoggingLevel::default(),
            language: default_i18n_language(),
            theme_mode: ThemeMode::System,
            lighten_animation_effects: false,
            enable_service_mode: false,
            enable_auto_launch: false,
            enable_silent_start: false,
            enable_system_proxy: false,
            enable_proxy_guard: false,
            system_proxy_bypass: String::new(),
            proxy_guard_interval: 10,
            theme_color: CssColor::from_rgba8(24, 103, 192, 255),
            core: ClashCore::default(),
            enable_builtin_enhanced: true,
            max_log_files: 7,
            enable_auto_check_update: true,
            tray_selector_mode: ProxiesSelectorMode::default(),
            always_on_top: false,
            window_type: WindowType::Legacy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struct_patch::Status;

    #[test]
    fn patch_honours_legacy_aliases() {
        let patch: ChimeraAppConfigPatch = serde_yaml_ng::from_str(
            "proxy_guard_duration: 45\nclash_core: chimera-client\nclash_tray_selector: hidden\n",
        )
        .expect("aliased patch must deserialize");

        assert_eq!(patch.proxy_guard_interval, Some(45));
        assert_eq!(patch.core, Some(ClashCore::ChimeraClient));
        assert_eq!(patch.tray_selector_mode, Some(ProxiesSelectorMode::Hidden));
        assert!(!patch.is_empty());
    }

    #[test]
    fn patch_skips_absent_fields() {
        let mut patch = ChimeraAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let dumped = serde_yaml_ng::to_string(&patch).expect("serialize patch");
        assert!(dumped.contains("enable_system_proxy: true"));
        assert!(!dumped.contains("enable_service_mode"));
    }
}
