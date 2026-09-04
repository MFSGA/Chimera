use std::sync::Arc;

use chimera_config::application::{
    ChimeraAppConfig, ClashCore as AppClashCore, I18nLanguage, ThemeMode,
};

use crate::{
    config::{
        chimera::{ClashCore, IVerge},
        core::Config,
    },
    state::mirror::{PreparedLegacyMirror, VergeLegacyBridge as VergeLegacyBridgeTrait},
};

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

pub(crate) fn apply_app_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &ChimeraAppConfig,
) -> anyhow::Result<()> {
    draft.app_log_level = Some(super::yaml_convert(&snap.app_log_level)?);
    draft.language = Some(super::yaml_convert(snap.language)?);
    draft.theme_mode = Some(super::yaml_convert(snap.theme_mode)?);
    draft.lighten_animation_effects = Some(snap.lighten_animation_effects);
    draft.enable_service_mode = Some(snap.enable_service_mode);
    draft.enable_auto_launch = Some(snap.enable_auto_launch);
    draft.enable_silent_start = Some(snap.enable_silent_start);
    draft.enable_system_proxy = Some(snap.enable_system_proxy);
    draft.enable_proxy_guard = Some(snap.enable_proxy_guard);
    draft.system_proxy_bypass = if snap.system_proxy_bypass.is_empty() {
        None
    } else {
        Some(snap.system_proxy_bypass.clone())
    };
    draft.proxy_guard_interval = Some(snap.proxy_guard_interval);
    draft.theme_color = Some(super::yaml_convert(&snap.theme_color)?);
    draft.clash_core = Some(super::yaml_convert(snap.core)?);
    draft.enable_builtin_enhanced = Some(snap.enable_builtin_enhanced);
    draft.max_log_files = Some(snap.max_log_files);
    draft.enable_auto_check_update = Some(snap.enable_auto_check_update);
    draft.clash_tray_selector = Some(super::yaml_convert(snap.tray_selector_mode)?);
    draft.always_on_top = Some(snap.always_on_top);
    draft.window_type = Some(super::yaml_convert(snap.window_type)?);
    Ok(())
}

fn apply_prepared_app_projection(target: &mut IVerge, projected: &IVerge) {
    target.app_log_level = projected.app_log_level.clone();
    target.language = projected.language.clone();
    target.theme_mode = projected.theme_mode.clone();
    target.lighten_animation_effects = projected.lighten_animation_effects;
    target.enable_service_mode = projected.enable_service_mode;
    target.enable_auto_launch = projected.enable_auto_launch;
    target.enable_silent_start = projected.enable_silent_start;
    target.enable_system_proxy = projected.enable_system_proxy;
    target.enable_proxy_guard = projected.enable_proxy_guard;
    target.system_proxy_bypass = projected.system_proxy_bypass.clone();
    target.proxy_guard_interval = projected.proxy_guard_interval;
    target.theme_color = projected.theme_color.clone();
    target.clash_core = projected.clash_core;
    target.enable_builtin_enhanced = projected.enable_builtin_enhanced;
    target.max_log_files = projected.max_log_files;
    target.enable_auto_check_update = projected.enable_auto_check_update;
    target.clash_tray_selector = projected.clash_tray_selector;
    target.always_on_top = projected.always_on_top;
    target.window_type = projected.window_type;
}

pub(crate) struct LegacyVergeBridge {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for LegacyVergeBridge {
    fn default() -> Self {
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl LegacyVergeBridge {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

struct PreparedVergeMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedVergeMirror {
    fn apply(self: Box<Self>) {
        let _guard = self.legacy_lock.lock();
        let mut next = Config::verge().latest().clone();
        apply_prepared_app_projection(&mut next, &self.projected);
        *Config::verge().draft() = next;
        Config::verge().apply();
    }
}

impl VergeLegacyBridgeTrait for LegacyVergeBridge {
    fn prepare(&self, snap: &ChimeraAppConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        let _guard = self.legacy_lock.lock();
        let mut projected = Config::verge().latest().clone();
        apply_app_config_to_legacy_verge(&mut projected, snap)?;
        Ok(Box::new(PreparedVergeMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            projected,
        }))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
        let _guard = self.legacy_lock.lock();
        application_from_legacy(&Config::verge().latest())
    }
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
    fn bridge_uses_injected_legacy_lock() {
        let lock = Arc::new(parking_lot::Mutex::new(()));
        let bridge = LegacyVergeBridge::new(lock.clone());
        assert!(Arc::ptr_eq(&bridge.legacy_lock, &lock));
    }

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
