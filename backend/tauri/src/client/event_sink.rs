//! UI state-change notifications and desktop-owned effects.
//! Shared state changes follow ref's typed-event and post-commit contract.
//! Chimera's notice/tray helpers remain separate desktop presentation effects.

use std::sync::Arc;

use anyhow::Result;
use chimera_config::profile::ProfileId;
use chimera_config::runtime::executor::ResolvedPortBindings;
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, Runtime};
use tauri_specta::Event as _;

use super::effects::{
    plan::{ApplicationEffectFields, ClashEffectFields, EffectKind},
    ports::CommitNotifications,
};
use crate::core::handle::{Handle, Message};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles { files: Vec<ProfileId> },
    Proxies,
    // Chimera-only runtime transform inspector; ref does not expose it.
    RuntimeTransformDiagnostics,
}

/// The backend's authoritative state-change event, delivered to every window.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChangedEvent(pub StateChanged);

pub(crate) trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged) {
        crate::log_err!(Handle::emit_state_changed(state));
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
        self.state_changed(StateChanged::Profiles { files: Vec::new() });
    }

    fn mutate_proxies(&self) {
        self.state_changed(StateChanged::Proxies);
    }
}

/// Every committed domain change notifies the UI independently of the caller.
pub(crate) struct UiCommitNotifications(pub Arc<dyn UiEventSink>);

impl CommitNotifications for UiCommitNotifications {
    fn application_committed(&self, _: ApplicationEffectFields, _: Vec<EffectKind>) {
        self.0.state_changed(StateChanged::NyanpasuConfig);
    }

    fn clash_committed(&self, _: ClashEffectFields) {
        self.0.state_changed(StateChanged::ClashConfig);
    }

    fn profiles_committed(&self, files: Vec<ProfileId>) {
        self.0.state_changed(StateChanged::Profiles { files });
    }

    fn runtime_bound(&self, _: Option<ResolvedPortBindings>, _: bool) {
        self.0.state_changed(StateChanged::ClashConfig);
    }

    fn publish_full(&self, _: Option<ResolvedPortBindings>) {
        self.0.state_changed(StateChanged::ClashConfig);
    }
}

#[derive(Clone)]
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
        crate::log_err!(StateChangedEvent(state).emit(&self.app_handle));
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
        Handle::update_systray_part()
    }
}

/// Used by existing tests and non-desktop adapters.
pub(crate) struct LegacyUiEventSink;

impl UiEventSink for LegacyUiEventSink {}

#[derive(Clone, Default)]
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
    use super::*;
    use std::sync::Mutex;

    struct RecordingSink(Arc<Mutex<Vec<StateChanged>>>);
    impl UiEventSink for RecordingSink {
        fn state_changed(&self, state: StateChanged) {
            self.0.lock().unwrap().push(state);
        }
    }

    #[test]
    fn refresh_helpers_use_typed_state_changes() {
        let states = Arc::new(Mutex::new(Vec::new()));
        let sink = RecordingSink(states.clone());
        sink.refresh_clash();
        sink.refresh_runtime_transform_diagnostics();
        sink.refresh_verge();
        sink.refresh_profiles();
        sink.mutate_proxies();
        assert!(matches!(
            states.lock().unwrap().as_slice(),
            [
                StateChanged::ClashConfig,
                StateChanged::RuntimeTransformDiagnostics,
                StateChanged::NyanpasuConfig,
                StateChanged::Profiles { files },
                StateChanged::Proxies,
            ] if files.is_empty()
        ));
    }
}
