use chimera_config::application::{
    ChimeraAppConfig, ClashCore as AppClashCore, I18nLanguage, ThemeMode,
};

use crate::config::chimera::{ClashCore, IVerge};

pub(crate) fn application_from_legacy(legacy: &IVerge) -> anyhow::Result<ChimeraAppConfig> {
    let mut next = ChimeraAppConfig::default();

    if let Some(value) = &legacy.app_log_level {
        next.app_log_level = super::yaml_convert(value)?;
    }
    if let Some(value) = &legacy.language {
        next.language = language_from_legacy(value).unwrap_or(next.language);
    }
    if let Some(value) = &legacy.theme_mode {
        next.theme_mode = theme_mode_from_legacy(value).unwrap_or(next.theme_mode);
    }
    if let Some(value) = legacy.lighten_animation_effects {
        next.lighten_animation_effects = value;
    }
    if let Some(value) = legacy.enable_service_mode {
        next.enable_service_mode = value;
    }
    if let Some(value) = legacy.enable_auto_launch {
        next.enable_auto_launch = value;
    }
    if let Some(value) = legacy.enable_silent_start {
        next.enable_silent_start = value;
    }
    if let Some(value) = legacy.enable_system_proxy {
        next.enable_system_proxy = value;
    }
    if let Some(value) = legacy.enable_proxy_guard {
        next.enable_proxy_guard = value;
    }
    if let Some(value) = &legacy.system_proxy_bypass {
        next.system_proxy_bypass = value.clone();
    }
    if let Some(value) = legacy.proxy_guard_interval {
        next.proxy_guard_interval = value;
    }
    if let Some(value) = &legacy.theme_color
        && let Ok(value) = value.parse()
    {
        next.theme_color = value;
    }
    if let Some(value) = legacy.clash_core {
        next.core = super::yaml_convert(value)?;
    }
    if let Some(value) = legacy.enable_builtin_enhanced {
        next.enable_builtin_enhanced = value;
    }
    if let Some(value) = legacy.max_log_files {
        next.max_log_files = value;
    }
    if let Some(value) = legacy.enable_auto_check_update {
        next.enable_auto_check_update = value;
    }
    if let Some(value) = legacy.clash_tray_selector {
        next.tray_selector_mode = super::yaml_convert(value)?;
    }
    if let Some(value) = legacy.always_on_top {
        next.always_on_top = value;
    }
    if let Some(value) = legacy.window_type {
        next.window_type = super::yaml_convert(value)?;
    }

    Ok(next)
}

pub(crate) fn legacy_core_from_typed(core: AppClashCore) -> ClashCore {
    match core {
        AppClashCore::ClashPremium => ClashCore::ClashPremium,
        AppClashCore::ClashRs => ClashCore::ClashRs,
        AppClashCore::Mihomo => ClashCore::Mihomo,
        AppClashCore::ChimeraClient => ClashCore::ChimeraClient,
        AppClashCore::MihomoAlpha => ClashCore::MihomoAlpha,
        AppClashCore::ClashRsAlpha => ClashCore::ClashRsAlpha,
    }
}

fn language_from_legacy(value: &str) -> Option<I18nLanguage> {
    match value.to_ascii_lowercase().as_str() {
        "en" | "en-us" => Some(I18nLanguage::English),
        "ko" | "ko-kr" => Some(I18nLanguage::Korean),
        "ru" | "ru-ru" => Some(I18nLanguage::Russian),
        "zh-cn" | "zh-hans" => Some(I18nLanguage::SimplifiedChinese),
        "zh-tw" | "zh-hant" | "zh-hk" | "zh-mo" => Some(I18nLanguage::TraditionalChinese),
        _ => None,
    }
}

fn theme_mode_from_legacy(value: &str) -> Option<ThemeMode> {
    match value.to_ascii_lowercase().as_str() {
        "light" => Some(ThemeMode::Light),
        "dark" => Some(ThemeMode::Dark),
        "system" => Some(ThemeMode::System),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::chimera::{ClashCore, ProxiesSelectorMode, WindowType};
    use chimera_config::application::{
        ProxiesSelectorMode as AppProxiesSelectorMode, WindowType as AppWindowType,
    };

    #[test]
    fn application_projection_preserves_chimera_fields_and_defaults() {
        let legacy = IVerge {
            app_log_level: Some(crate::config::chimera::LoggingLevel::Debug),
            language: Some("zh-CN".into()),
            theme_mode: Some("dark".into()),
            lighten_animation_effects: Some(true),
            enable_service_mode: Some(true),
            enable_auto_launch: Some(true),
            enable_silent_start: Some(true),
            enable_system_proxy: Some(true),
            enable_proxy_guard: Some(true),
            system_proxy_bypass: Some("localhost".into()),
            proxy_guard_interval: Some(15),
            theme_color: Some("#1867c0".into()),
            clash_core: Some(ClashCore::ChimeraClient),
            enable_builtin_enhanced: Some(false),
            max_log_files: Some(14),
            enable_auto_check_update: Some(false),
            clash_tray_selector: Some(ProxiesSelectorMode::Hidden),
            always_on_top: Some(true),
            window_type: Some(WindowType::Main),
            ..IVerge::default()
        };

        let typed = application_from_legacy(&legacy).expect("legacy app config should project");

        assert_eq!(typed.language, I18nLanguage::SimplifiedChinese);
        assert_eq!(typed.theme_mode, ThemeMode::Dark);
        assert_eq!(typed.core, AppClashCore::ChimeraClient);
        assert_eq!(typed.tray_selector_mode, AppProxiesSelectorMode::Hidden);
        assert_eq!(typed.window_type, AppWindowType::Main);
        assert_eq!(typed.proxy_guard_interval, 15);
        assert_eq!(typed.max_log_files, 14);
        assert!(typed.enable_service_mode);
        assert!(typed.enable_system_proxy);
        assert!(typed.always_on_top);
    }

    #[test]
    fn absent_legacy_values_keep_chimera_compatible_defaults() {
        let typed = application_from_legacy(&IVerge::default())
            .expect("default legacy app config should project");

        assert_eq!(typed.core, AppClashCore::Mihomo);
        assert_eq!(typed.proxy_guard_interval, 10);
        assert_eq!(typed.window_type, AppWindowType::Legacy);
        assert!(typed.enable_builtin_enhanced);
        assert_eq!(typed.max_log_files, 7);
    }
}
