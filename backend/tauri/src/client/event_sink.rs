//! UI event boundary for client-side state and tray effects.
//!
//! The default implementation keeps the legacy global `Handle` path working
//! while exposing the narrower event sink contract used by ref's lifecycle
//! workflow. A Tauri-owned implementation is available for future client
//! composition without coupling core code to a global app handle.

use crate::core::handle::{Handle, Message, StateChanged};
use anyhow::Result;
use tauri::{Emitter, Manager, Runtime};

/// Abstracts the Tauri UI side effects emitted by the client.
#[allow(dead_code)]
pub(crate) trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged) {
        crate::log_err!(Handle::emit("nyanpasu://mutation", state));
    }

    fn notice_message(&self, message: &Message) {
        Handle::notice_message(message);
    }

    fn update_systray(&self) -> Result<()> {
        Handle::update_systray()
    }

    fn update_systray_part(&self) -> Result<()> {
        Handle::update_systray_part()
    }

    fn refresh_clash(&self) {
        self.state_changed(StateChanged::ClashConfig);
    }

    fn refresh_runtime_transform_diagnostics(&self) {
        self.state_changed(StateChanged::RuntimeTransformDiagnostics);
    }

    fn refresh_verge(&self) {
        self.state_changed(StateChanged::NyanpasuConfig);
    }

    fn refresh_profiles(&self) {
        self.state_changed(StateChanged::Profiles);
    }

    fn mutate_proxies(&self) {
        self.state_changed(StateChanged::Proxies);
    }
}

/// Tauri-owned event sink that targets the current main window directly.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct TauriUiEventSink<R: Runtime = tauri::Wry> {
    app_handle: tauri::AppHandle<R>,
}

impl<R: Runtime> TauriUiEventSink<R> {
    pub(crate) fn new(app_handle: tauri::AppHandle<R>) -> Self {
        Self { app_handle }
    }
}

impl<R: Runtime> UiEventSink for TauriUiEventSink<R> {
    fn state_changed(&self, state: StateChanged) {
        if let Some(window) = self
            .app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
        {
            crate::log_err!(window.emit("nyanpasu://mutation", state));
        }
    }

    fn notice_message(&self, message: &Message) {
        if let Some(window) = self
            .app_handle
            .get_webview_window(crate::consts::MAIN_WINDOW_LABEL)
        {
            crate::log_err!(window.emit("nyanpasu://notice-message", message));
        }
    }

    fn update_systray(&self) -> Result<()> {
        self.app_handle.emit("update_systray", ())?;
        Ok(())
    }

    fn update_systray_part(&self) -> Result<()> {
        crate::core::tray::Tray::update_part(&self.app_handle)?;
        Ok(())
    }
}

/// Legacy sink retained for the existing global event broadcast behavior.
pub(crate) struct LegacyUiEventSink;

impl UiEventSink for LegacyUiEventSink {}

/// Test double for [`UiEventSink`] that does not require a Tauri runtime.
#[derive(Clone, Default)]
#[allow(dead_code)]
pub(crate) struct NoopUiEventSink;

impl UiEventSink for NoopUiEventSink {
    fn state_changed(&self, _state: StateChanged) {}

    fn notice_message(&self, _message: &Message) {}

    fn update_systray(&self) -> Result<()> {
        Ok(())
    }

    fn update_systray_part(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct RecordingSink {
        states: Arc<Mutex<Vec<StateChanged>>>,
    }

    impl UiEventSink for RecordingSink {
        fn state_changed(&self, state: StateChanged) {
            self.states.lock().unwrap().push(state);
        }
    }

    #[test]
    fn default_refresh_methods_use_shared_state_changed_contract() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink {
            states: states.clone(),
        };

        sink.refresh_clash();
        sink.refresh_runtime_transform_diagnostics();
        sink.refresh_verge();
        sink.refresh_profiles();
        sink.mutate_proxies();

        let states = states.lock().unwrap();
        assert!(matches!(
            states.as_slice(),
            [
                StateChanged::ClashConfig,
                StateChanged::RuntimeTransformDiagnostics,
                StateChanged::NyanpasuConfig,
                StateChanged::Profiles,
                StateChanged::Proxies,
            ]
        ));
    }
}
