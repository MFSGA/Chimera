//! Platform adapters used by the application effect dispatcher.
//!
//! These implementations bridge the typed effect plan to the remaining
//! Chimera locale, logger, widget and tray boundaries. System proxy, PAC and
//! auto-launch are owned by the reference-aligned `SystemProxyClient`.

use chimera_config::application::{I18nLanguage, NetworkStatisticWidgetConfig};

use super::plan::{TrayRefresh, TrayView, WidgetLogoDesired};
use super::widget::{TauriWidgetController, WidgetController};
use std::sync::Arc;

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
    async fn set_widget(
        &self,
        config: NetworkStatisticWidgetConfig,
    ) -> Result<(), EffectAdapterError>;
    async fn set_widget_logo(&self, logo: WidgetLogoDesired) -> Result<(), EffectAdapterError>;
    async fn refresh_tray(
        &self,
        refresh: TrayRefresh,
        view: TrayView,
    ) -> Result<(), EffectAdapterError>;
}

/// Tauri adapters for the effects that do not have a dedicated owner yet.
pub(crate) struct TauriApplicationEffectAdapters {
    app_handle: tauri::AppHandle<tauri::Wry>,
    widget: Arc<TauriWidgetController>,
}

impl TauriApplicationEffectAdapters {
    pub(crate) fn new(
        app_handle: tauri::AppHandle<tauri::Wry>,
        widget: Arc<TauriWidgetController>,
    ) -> Self {
        Self { app_handle, widget }
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

    async fn set_widget(
        &self,
        config: NetworkStatisticWidgetConfig,
    ) -> Result<(), EffectAdapterError> {
        self.widget.apply(config).await.map_err(widget_error)
    }

    async fn set_widget_logo(&self, logo: WidgetLogoDesired) -> Result<(), EffectAdapterError> {
        self.widget
            .show_logo(logo.source, logo.state)
            .await
            .map_err(widget_error)
    }

    async fn refresh_tray(
        &self,
        refresh: TrayRefresh,
        view: TrayView,
    ) -> Result<(), EffectAdapterError> {
        crate::core::tray::Tray::store_view(&self.app_handle, view);
        crate::core::tray::Tray::request_refresh(&self.app_handle, refresh)
            .map_err(|error| EffectAdapterError::failed("tray_refresh_failed", error))
    }
}

// Startup-ordering failures are retryable, as in the reference presentation executor.
fn widget_error(error: crate::widget::WidgetError) -> EffectAdapterError {
    let code = match error.code() {
        chimera_runtime_values::configuration::EffectFailureCode::WidgetUnavailable => {
            "widget_unavailable"
        }
        _ => "widget_apply_failed",
    };
    EffectAdapterError::failed(code, format!("{:#}", anyhow::Error::new(error)))
}
