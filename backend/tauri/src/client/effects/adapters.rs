//! Platform adapters used by the application effect dispatcher.
//!
//! These implementations bridge the typed effect plan to the existing
//! Chimera sysopt, logger and tray boundaries. System proxy ownership remains
//! transitional until the reference `SystemProxyClient` is migrated.

use chimera_config::application::{I18nLanguage, LoggingLevel, NetworkStatisticWidgetConfig};

use super::plan::{ProxyGuardDesired, SystemProxyDesired, TrayRefresh};

#[derive(Debug)]
pub(crate) enum EffectAdapterError {
    Failed {
        code: &'static str,
        message: String,
        retryable: bool,
    },
    Unsupported {
        code: &'static str,
    },
}

impl EffectAdapterError {
    fn failed(code: &'static str, error: impl std::fmt::Display) -> Self {
        Self::Failed {
            code,
            message: error.to_string(),
            retryable: true,
        }
    }
}

#[async_trait::async_trait]
pub(crate) trait ApplicationEffectAdapters: Send + Sync + 'static {
    fn set_locale(&self, language: I18nLanguage) -> Result<(), EffectAdapterError>;
    fn refresh_logger(
        &self,
        desired: &super::plan::LoggerDesired,
    ) -> Result<(), EffectAdapterError>;
    async fn set_auto_launch(&self, enabled: bool) -> Result<(), EffectAdapterError>;
    async fn set_system_proxy(
        &self,
        desired: &SystemProxyDesired,
    ) -> Result<(), EffectAdapterError>;
    fn set_proxy_guard(&self, desired: ProxyGuardDesired) -> Result<(), EffectAdapterError>;
    fn set_widget(&self, config: NetworkStatisticWidgetConfig) -> Result<(), EffectAdapterError>;
    async fn refresh_tray(&self, refresh: TrayRefresh) -> Result<(), EffectAdapterError>;
    async fn restore_system_proxy(&self) -> Result<(), EffectAdapterError>;
}

/// Tauri and legacy sysopt adapters for the effects that do not have a
/// dedicated Chimera actor yet.
pub(crate) struct TauriApplicationEffectAdapters {
    app_handle: tauri::AppHandle<tauri::Wry>,
}

impl TauriApplicationEffectAdapters {
    pub(crate) fn new(app_handle: tauri::AppHandle<tauri::Wry>) -> Self {
        Self { app_handle }
    }
}

#[async_trait::async_trait]
impl ApplicationEffectAdapters for TauriApplicationEffectAdapters {
    fn set_locale(&self, language: I18nLanguage) -> Result<(), EffectAdapterError> {
        rust_i18n::set_locale(match language {
            I18nLanguage::English => "en-US",
            I18nLanguage::Korean => "ko",
            I18nLanguage::Russian => "ru",
            I18nLanguage::SimplifiedChinese => "zh-CN",
            I18nLanguage::TraditionalChinese => "zh-TW",
        });
        Ok(())
    }

    fn refresh_logger(
        &self,
        desired: &super::plan::LoggerDesired,
    ) -> Result<(), EffectAdapterError> {
        crate::utils::init::refresh_logger((
            Some(legacy_logging_level(&desired.level)),
            Some(desired.max_files),
        ))
        .map_err(|error| EffectAdapterError::failed("logger_refresh_failed", error))
    }

    async fn set_auto_launch(&self, enabled: bool) -> Result<(), EffectAdapterError> {
        tokio::task::spawn_blocking(move || {
            crate::core::sysopt::Sysopt::global().update_launch_to(enabled)
        })
        .await
        .map_err(|error| EffectAdapterError::failed("auto_launch_failed", error))?
        .map_err(|error| EffectAdapterError::failed("auto_launch_failed", error))
    }

    async fn set_system_proxy(
        &self,
        desired: &SystemProxyDesired,
    ) -> Result<(), EffectAdapterError> {
        let desired = desired.clone();
        tokio::task::spawn_blocking(move || {
            crate::core::sysopt::Sysopt::global().reconcile_system_proxy(
                desired.enabled,
                desired.port,
                &desired.bypass,
            )
        })
        .await
        .map_err(|error| EffectAdapterError::failed("system_proxy_apply_failed", error))?
        .map_err(|error| EffectAdapterError::failed("system_proxy_apply_failed", error))
    }

    fn set_proxy_guard(&self, desired: ProxyGuardDesired) -> Result<(), EffectAdapterError> {
        if desired.enabled {
            // Sysopt's compatibility loop reads the committed typed mirror,
            // including the interval, and checks that system proxy remains on.
            crate::core::sysopt::Sysopt::global().guard_proxy();
        }
        Ok(())
    }

    fn set_widget(&self, config: NetworkStatisticWidgetConfig) -> Result<(), EffectAdapterError> {
        match config {
            NetworkStatisticWidgetConfig::Disabled => Ok(()),
            NetworkStatisticWidgetConfig::Enabled(_) => Err(EffectAdapterError::Unsupported {
                code: "widget_runtime_unavailable",
            }),
        }
    }

    async fn refresh_tray(&self, refresh: TrayRefresh) -> Result<(), EffectAdapterError> {
        let app_handle = self.app_handle.clone();
        self.app_handle
            .run_on_main_thread(move || {
                let result = match refresh {
                    TrayRefresh::Full => crate::core::tray::Tray::update_systray(&app_handle),
                    TrayRefresh::Part => crate::core::tray::Tray::update_part(&app_handle),
                };
                if let Err(error) = result {
                    tracing::warn!(%error, ?refresh, "failed to refresh the system tray");
                }
            })
            .map_err(|error| EffectAdapterError::failed("tray_refresh_failed", error))
    }

    async fn restore_system_proxy(&self) -> Result<(), EffectAdapterError> {
        tokio::task::spawn_blocking(|| crate::core::sysopt::Sysopt::global().reset_sysproxy())
            .await
            .map_err(|error| EffectAdapterError::failed("system_proxy_restore_failed", error))?
            .map_err(|error| EffectAdapterError::failed("system_proxy_restore_failed", error))
    }
}

fn legacy_logging_level(level: &LoggingLevel) -> crate::config::chimera::LoggingLevel {
    match level {
        LoggingLevel::Silent => crate::config::chimera::LoggingLevel::Silent,
        LoggingLevel::Trace => crate::config::chimera::LoggingLevel::Trace,
        LoggingLevel::Debug => crate::config::chimera::LoggingLevel::Debug,
        LoggingLevel::Info => crate::config::chimera::LoggingLevel::Info,
        LoggingLevel::Warn => crate::config::chimera::LoggingLevel::Warn,
        LoggingLevel::Error => crate::config::chimera::LoggingLevel::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_log_levels_keep_the_legacy_reload_mapping() {
        for (current, expected) in [
            (
                LoggingLevel::Silent,
                crate::config::chimera::LoggingLevel::Silent,
            ),
            (
                LoggingLevel::Trace,
                crate::config::chimera::LoggingLevel::Trace,
            ),
            (
                LoggingLevel::Debug,
                crate::config::chimera::LoggingLevel::Debug,
            ),
            (
                LoggingLevel::Info,
                crate::config::chimera::LoggingLevel::Info,
            ),
            (
                LoggingLevel::Warn,
                crate::config::chimera::LoggingLevel::Warn,
            ),
            (
                LoggingLevel::Error,
                crate::config::chimera::LoggingLevel::Error,
            ),
        ] {
            let converted = legacy_logging_level(&current);
            assert_eq!(converted.to_string(), expected.to_string());
        }
    }
}
