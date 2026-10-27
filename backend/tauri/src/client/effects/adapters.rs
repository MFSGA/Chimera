//! Platform adapters used by the application effect dispatcher.
//!
//! These implementations bridge the typed effect plan to the remaining
//! Chimera locale, logger, widget and tray boundaries. System proxy, PAC and
//! auto-launch are owned by the reference-aligned `SystemProxyClient`.

use chimera_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};

use super::plan::TrayRefresh;

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
    fn set_widget(&self, config: NetworkStatisticWidgetConfig) -> Result<(), EffectAdapterError>;
    async fn refresh_tray(&self, refresh: TrayRefresh) -> Result<(), EffectAdapterError>;
}

/// Tauri adapters for the effects that do not have a dedicated owner yet.
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
        rust_i18n::set_locale(language.as_str());
        Ok(())
    }

    fn refresh_logger(
        &self,
        desired: &super::plan::LoggerDesired,
    ) -> Result<(), EffectAdapterError> {
        crate::utils::init::refresh_logger((Some(desired.level.clone()), Some(desired.max_files)))
            .map_err(|error| EffectAdapterError::failed("logger_refresh_failed", error))
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
}
