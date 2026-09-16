use csscolorparser::Color as CssColor;
use serde::{Deserialize, Serialize};
use specta::Type;
use struct_patch::Patch;

mod clash_core;
mod i18n;
mod logging;
mod widget;

pub use clash_core::*;
pub use i18n::*;
pub use logging::*;
pub use widget::*;

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

/// Whether the tray menu uses the system-native menu or the WebView menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuMode {
    Native,
    Webview,
}

impl Default for TrayMenuMode {
    fn default() -> Self {
        if cfg!(windows) {
            Self::Webview
        } else {
            Self::Native
        }
    }
}

/// What happens to the WebView tray menu window when it loses focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuCloseBehavior {
    #[default]
    Hide,
    Close,
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
#[serde(default)]
#[patch(attribute(serde_with::skip_serializing_none))]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)))]
pub struct ChimeraAppConfig {
    /// app listening port for app singleton
    pub app_singleton_port: u16,
    pub app_log_level: LoggingLevel,
    pub language: I18nLanguage,
    pub theme_mode: ThemeMode,
    /// enable traffic graph
    pub traffic_graph: bool,
    /// show memory info (only for Clash Meta)
    pub enable_memory_usage: bool,
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
    /// hotkey map, formatted as `{func},{key}`
    pub hotkeys: Vec<String>,
    /// default URL used for latency tests
    pub default_latency_test: String,
    pub enable_builtin_enhanced: bool,
    /// proxy page layout column count
    pub proxy_layout_column: i32,
    pub max_log_files: usize,
    pub enable_auto_check_update: bool,
    #[patch(attribute(serde(alias = "clash_tray_selector")))]
    pub tray_selector_mode: ProxiesSelectorMode,
    pub always_on_top: bool,
    pub tray_menu_mode: TrayMenuMode,
    pub tray_menu_close_behavior: TrayMenuCloseBehavior,
    pub network_statistic_widget: NetworkStatisticWidgetConfig,
    /// PAC URL for automatic proxy configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[patch(attribute(serde(default, with = "::serde_with::rust::double_option")))]
    pub pac_url: Option<url::Url>,
    pub enable_tray_text: bool,
    pub enable_tray_traffic: bool,
    /// Use legacy UI (the original `/` route) when opening the app.
    pub use_legacy_ui: bool,
    pub window_type: WindowType,
}

impl Default for ChimeraAppConfig {
    fn default() -> Self {
        Self {
            app_singleton_port: 0,
            app_log_level: LoggingLevel::default(),
            language: default_i18n_language(),
            theme_mode: ThemeMode::System,
            traffic_graph: true,
            enable_memory_usage: true,
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
            hotkeys: Vec::new(),
            default_latency_test: "http://www.gstatic.com/generate_204".into(),
            enable_builtin_enhanced: true,
            proxy_layout_column: 0,
            max_log_files: 7,
            enable_auto_check_update: true,
            tray_selector_mode: ProxiesSelectorMode::default(),
            always_on_top: false,
            tray_menu_mode: TrayMenuMode::default(),
            tray_menu_close_behavior: TrayMenuCloseBehavior::default(),
            network_statistic_widget: NetworkStatisticWidgetConfig::default(),
            pac_url: None,
            enable_tray_text: false,
            enable_tray_traffic: false,
            use_legacy_ui: false,
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

    #[test]
    fn missing_ref_fields_use_compatibility_defaults() {
        let config: ChimeraAppConfig =
            serde_yaml_ng::from_str("enable_system_proxy: true\nwindow_type: legacy\n")
                .expect("older typed config must remain readable");

        assert!(config.enable_system_proxy);
        assert_eq!(config.app_singleton_port, 0);
        assert!(config.traffic_graph);
        assert_eq!(
            config.default_latency_test,
            "http://www.gstatic.com/generate_204"
        );
        assert!(!config.use_legacy_ui);
    }
}
