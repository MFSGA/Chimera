//! The concrete boundary implementations. This is the only file in the module
//! allowed to name Tauri or the global-shortcut plugin.

use std::{str::FromStr, sync::Arc};

use anyhow::Context;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::main_thread::{MainThreadError, MainThreadExecutor, MainThreadHandoff, MainThreadTask};
use super::ports::{
    AcceleratorValidator, HotkeyAction, HotkeyActionSink, HotkeyParseError, ShortcutRegistrar,
    WindowControl, has_super_key,
};

/// The accelerator rules the platform actually enforces: the shortcut plugin's
/// own parser, plus the super-key requirement on top.
///
/// Holds nothing, so both the facade's pre-commit check and the registrar can
/// use the same instance of the rule.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlatformAcceleratorValidator;

impl AcceleratorValidator for PlatformAcceleratorValidator {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        self.canonical(accelerator).map(|_| ())
    }

    fn canonical(&self, accelerator: &str) -> Result<String, HotkeyParseError> {
        // The settings UI keeps the main keyboard's `+` key as `PLUS` so it
        // cannot be confused with the accelerator separator. global-hotkey
        // represents that physical key as `Equal` (with Shift when needed).
        let normalized = accelerator
            .split('+')
            .map(|token| {
                if token.eq_ignore_ascii_case("PLUS") {
                    "EQUAL"
                } else {
                    token
                }
            })
            .collect::<Vec<_>>()
            .join("+");
        // The plugin's own parse panics inside `register`, so an accelerator it
        // cannot read has to be rejected before it gets there (issue #287).
        let shortcut = Shortcut::from_str(&normalized)
            .map_err(|_| HotkeyParseError::InvalidAccelerator(accelerator.to_owned()))?;
        // Asked of what the user wrote, not of the canonical form: the
        // canonical spelling of a modifier-less accelerator would still have to
        // be rejected, and the message has to name what they typed.
        if !has_super_key(accelerator) {
            return Err(HotkeyParseError::MissingSuperKey(accelerator.to_owned()));
        }
        // Round-trips: the plugin's parser reads its own output back to the
        // same shortcut, so this is what gets handed to the OS.
        Ok(shortcut.into_string())
    }
}

/// UI-thread dispatcher, injected into OS-effect adapters rather than called
/// synchronously from the hotkey actor. The thread id is captured by Tauri's
/// setup callback, which runs on its event-loop thread.
pub struct TauriMainThread<R: tauri::Runtime> {
    app_handle: tauri::AppHandle<R>,
    handoff: Arc<MainThreadHandoff>,
    ui_thread: std::thread::ThreadId,
}

impl<R: tauri::Runtime> TauriMainThread<R> {
    pub fn new(app_handle: tauri::AppHandle<R>, handoff: Arc<MainThreadHandoff>) -> Self {
        Self {
            app_handle,
            handoff,
            ui_thread: std::thread::current().id(),
        }
    }
}

impl<R: tauri::Runtime> MainThreadExecutor for TauriMainThread<R> {
    fn execute(&self, task: MainThreadTask) -> Result<(), MainThreadError> {
        if std::thread::current().id() == self.ui_thread {
            task();
            return Ok(());
        }
        match self.handoff.hand_over(task) {
            Ok(()) => Ok(()),
            Err(task) => self.app_handle.run_on_main_thread(task).map_err(|source| {
                MainThreadError::HandOffToEventLoop {
                    source: Box::new(source),
                }
            }),
        }
    }

    fn event_loop_lost(&self) -> tokio_util::sync::CancellationToken {
        self.handoff.event_loop_lost()
    }
}

/// Global shortcuts through `tauri_plugin_global_shortcut`.
pub struct TauriShortcutRegistrar<R: tauri::Runtime> {
    app_handle: tauri::AppHandle<R>,
    main_thread: Arc<dyn MainThreadExecutor>,
}

impl<R: tauri::Runtime> TauriShortcutRegistrar<R> {
    pub fn new(app_handle: tauri::AppHandle<R>, main_thread: Arc<dyn MainThreadExecutor>) -> Self {
        Self {
            app_handle,
            main_thread,
        }
    }
}

#[async_trait::async_trait]
impl<R: tauri::Runtime> ShortcutRegistrar for TauriShortcutRegistrar<R> {
    fn validate(&self, accelerator: &str) -> Result<(), HotkeyParseError> {
        // One rule, checked twice: what the facade rejects before a commit is
        // exactly what the OS would refuse afterwards.
        PlatformAcceleratorValidator.validate(accelerator)
    }

    async fn register(
        &self,
        accelerator: &str,
        action: HotkeyAction,
        sink: Arc<dyn HotkeyActionSink>,
    ) -> anyhow::Result<()> {
        let handle = self.app_handle.clone();
        let accelerator = accelerator.to_owned();
        self.main_thread
            .run(move || {
                let manager = handle.global_shortcut();
                // Release a previous OS grab before attempting to replace it.
                if manager.is_registered(accelerator.as_str()) {
                    manager
                        .unregister(accelerator.as_str())
                        .with_context(|| format!("failed to release the shortcut {accelerator}"))?;
                }
                manager
                    .on_shortcut(accelerator.as_str(), move |_app_handle, shortcut, event| {
                        if event.state == ShortcutState::Pressed {
                            tracing::info!(%shortcut, %action, "hotkey pressed");
                            sink.dispatch(action);
                        }
                    })
                    .with_context(|| format!("failed to register the shortcut {accelerator}"))
            })
            .await
            .context("failed to dispatch shortcut registration to the main thread")?
    }

    async fn unregister(&self, accelerator: &str) -> anyhow::Result<()> {
        let handle = self.app_handle.clone();
        let accelerator = accelerator.to_owned();
        self.main_thread
            .run(move || {
                handle
                    .global_shortcut()
                    .unregister(accelerator.as_str())
                    .with_context(|| format!("failed to release the shortcut {accelerator}"))
            })
            .await
            .context("failed to dispatch shortcut release to the main thread")?
    }

    async fn unregister_all(&self) -> anyhow::Result<()> {
        let handle = self.app_handle.clone();
        self.main_thread
            .run(move || {
                handle
                    .global_shortcut()
                    .unregister_all()
                    .context("failed to release the registered shortcuts")
            })
            .await
            .context("failed to dispatch shortcut cleanup to the main thread")?
    }
}

/// Hands a pressed shortcut to the pump that drives the facade.
///
/// A channel rather than a direct call: the facade owns the actor that owns
/// this sink, so calling back into it directly would close a cycle.
pub struct ChannelActionSink(tokio::sync::mpsc::UnboundedSender<HotkeyAction>);

impl ChannelActionSink {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<HotkeyAction>) -> Self {
        Self(sender)
    }
}

impl HotkeyActionSink for ChannelActionSink {
    fn dispatch(&self, action: HotkeyAction) {
        // Unbounded and non-blocking: this runs on the OS callback, which must
        // not wait for anything. A closed receiver means the app is exiting.
        if self.0.send(action).is_err() {
            tracing::debug!(%action, "no hotkey pump is listening any more");
        }
    }
}

/// The dashboard window, through the existing window helpers.
pub struct TauriWindowControl {
    app_handle: tauri::AppHandle,
}

impl TauriWindowControl {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self { app_handle }
    }
}

#[async_trait::async_trait]
impl WindowControl for TauriWindowControl {
    async fn toggle_dashboard(&self) -> anyhow::Result<()> {
        let app_handle = self.app_handle.clone();
        let (done, wait) = tokio::sync::oneshot::channel();
        // Window work belongs on the main thread; the caller is the hotkey
        // pump, which runs on the async runtime.
        self.app_handle
            .run_on_main_thread(move || {
                let label = crate::utils::resolve::configured_window_label();
                if let Some(window) = app_handle.get_webview_window(label) {
                    if let Err(error) = window.close() {
                        tracing::warn!(%error, label, "failed to close dashboard window");
                    }
                } else {
                    crate::utils::resolve::create_window(&app_handle);
                }
                let _ = done.send(());
            })
            .context("failed to schedule the dashboard toggle on the main thread")?;
        // Awaited so two quick presses cannot interleave into a no-op.
        wait.await
            .context("the dashboard toggle did not run to completion")
    }
}
