//! Owns the lifecycle of every window: creates hidden, shows after the
//! frontend reports ready, and retires/destroys from the window state table.
//! Follows ref/backend/tauri/src/window/manager.rs at 59724e85c.

use super::{
    AppWindow, WindowHooks, WindowParams,
    close::WindowCloseBehavior,
    engine,
    table::{Opened, WindowTable},
};
use anyhow::Result;
use parking_lot::Mutex;
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager, WebviewWindow, WindowEvent};

const READY_FALLBACK: Duration = Duration::from_secs(10);

type Table = WindowTable<Arc<dyn WindowHooks>>;

/// This lock guards only lifecycle facts; never hold it while invoking Tauri.
pub struct WindowManager {
    app_handle: AppHandle,
    table: Mutex<Table>,
}

impl WindowManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            table: Mutex::new(Table::default()),
        }
    }

    pub fn open<K: AppWindow>(&self, kind: &K, params: Option<WindowParams>) -> Result<String> {
        let hooks: Arc<dyn WindowHooks> = Arc::new(kind.clone());
        let opened = self
            .table
            .lock()
            .open(kind.label(), kind.config().singleton, hooks);
        match opened {
            Opened::Existing(label) => {
                tracing::debug!("{} window already opened; focus it if ready", label);
                self.queue_apply(&label);
                Ok(label)
            }
            Opened::Build(label) => {
                self.create(kind, &label, params)?;
                Ok(label)
            }
        }
    }

    pub fn report_ready(&self, label: &str) {
        let first = self.table.lock().ready(label);
        match first {
            Some(true) => {
                tracing::debug!("Window '{label}' is ready");
                self.queue_apply(label);
            }
            Some(false) => {}
            None => tracing::warn!("Window '{label}' reported ready but is not known"),
        }
    }

    pub fn close(&self, label: &str) {
        if self.table.lock().unwant(label) {
            self.queue_apply(label);
        }
    }

    pub fn is_wanted(&self, label: &str) -> bool {
        self.table.lock().wanted(label)
    }

    pub fn instances(&self, base_label: &str) -> Vec<String> {
        self.table.lock().instances(base_label)
    }

    fn queue_apply(&self, label: &str) {
        let handle = self.app_handle.clone();
        let label = label.to_owned();
        tauri::async_runtime::spawn(async move {
            let on_main = handle.clone();
            crate::log_err!(handle.run_on_main_thread(move || apply(&on_main, &label)));
        });
    }

    fn create<K: AppWindow>(
        &self,
        kind: &K,
        label: &str,
        params: Option<WindowParams>,
    ) -> Result<()> {
        let app_handle = &self.app_handle;
        let config = kind.config();
        let window_state = kind.get_window_state(app_handle);
        let mut builder = tauri::WebviewWindowBuilder::new(
            app_handle,
            label,
            tauri::WebviewUrl::App(kind.url_with_params(params.as_ref()).into()),
        )
        .title(kind.title())
        .fullscreen(false)
        .always_on_top(super::resolve_always_on_top(&config))
        .resizable(config.resizable)
        .skip_taskbar(config.skip_taskbar)
        .focused(config.focused_on_create)
        .disable_drag_drop_handler()
        // WindowManager reveals the window after its frontend reports ready.
        .visible(false);

        if let Some((width, height)) = config.min_size {
            builder = builder.min_inner_size(width, height);
        }
        if let Some((width, height)) = config.max_size {
            builder = builder.max_inner_size(width, height);
        }

        if window_state.is_some() {
            builder = builder
                .inner_size(
                    super::RESTORE_PLACEHOLDER_SIZE.0,
                    super::RESTORE_PLACEHOLDER_SIZE.1,
                )
                .position(0.0, 0.0);
        } else {
            let (width, height) = super::default_inner_size(config.default_size);
            builder = builder.inner_size(width, height);

            if config.center {
                builder = builder.center();
            }
        }

        let builder = engine::configure_builder(builder);

        #[cfg(windows)]
        let window = builder.decorations(false).transparent(true).build();

        #[cfg(target_os = "macos")]
        let window = {
            let decorations = config.decorations.unwrap_or(true);
            if decorations {
                builder
                    .decorations(true)
                    .hidden_title(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .build()
            } else {
                builder.decorations(false).build()
            }
        };

        #[cfg(all(not(windows), not(target_os = "macos")))]
        let window = builder.build();

        let window = match window {
            Ok(window) => window,
            Err(err) => {
                log::error!(target: "app", "failed to create window, {err:?}");
                self.table.lock().remove(label);
                if let Some(stale) = app_handle.get_webview_window(label) {
                    crate::log_err!(stale.destroy(), "cleanup window after build failure");
                }
                return Err(err.into());
            }
        };

        // Register listeners before geometry changes can re-enter the window.
        self.watch(kind, label, &window);

        super::restore_window_state(&window, window_state.as_ref(), config.min_size);

        #[cfg(windows)]
        crate::trace_err!(window.set_shadow(true), "set win shadow");

        if super::should_center_window(&window, window_state.as_ref()).unwrap_or(true) {
            crate::trace_err!(window.center(), "set win center");
        }

        #[cfg(target_os = "macos")]
        if config.decorations.unwrap_or(true) {
            let mtm = objc2_foundation::MainThreadMarker::new().unwrap();
            super::macos::setup_traffic_lights_pos(window.clone(), (18.0, 22.0), mtm);
        }

        kind.on_created(&window);

        // The frontend may have reported ready during construction.
        // Drop the non-reentrant table lock before start_fallback(), which
        // takes that same lock to register its watchdog. Match ref's lock
        // boundary: compute the transition first, then perform side effects.
        let awaiting_ready = self.table.lock().built(label);
        match awaiting_ready {
            Some(true) => {
                self.start_fallback(label);
                self.queue_apply(label);
            }
            Some(false) => self.queue_apply(label),
            None => {}
        }
        Ok(())
    }

    fn watch<K: AppWindow>(&self, kind: &K, label: &str, window: &WebviewWindow) {
        let kind = kind.clone();
        let event_window = window.clone();
        let handle = self.app_handle.clone();
        let event_label = label.to_owned();

        window.on_window_event(move |event| {
            kind.on_window_event(&event_window, event);
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    if let Some(manager) = handle.try_state::<WindowManager>() {
                        api.prevent_close();
                        manager.close(&event_label);
                    }
                }
                WindowEvent::Resized(_) => follow_minimized(&event_window),
                WindowEvent::Destroyed => {
                    if let Some(manager) = handle.try_state::<WindowManager>() {
                        manager.table.lock().remove(&event_label);
                    }
                }
                _ => {}
            }
        });
    }

    fn start_fallback(&self, label: &str) {
        let handle = self.app_handle.clone();
        let fallback_label = label.to_owned();
        let fallback = tauri::async_runtime::spawn(async move {
            tokio::time::sleep(READY_FALLBACK).await;
            let Some(manager) = handle.try_state::<WindowManager>() else {
                return;
            };
            if manager.table.lock().ready(&fallback_label) == Some(true) {
                tracing::warn!(
                    "Window '{}' did not report ready within {}s, treating it as ready",
                    fallback_label,
                    READY_FALLBACK.as_secs()
                );
                manager.queue_apply(&fallback_label);
            }
        });
        // Tauri JoinHandle wraps Tokio's JoinHandle.
        let abort = fallback.inner().abort_handle();
        if !self.table.lock().set_watchdog(label, &abort) {
            fallback.abort();
        }
    }
}

/// Read the latest facts on the main thread, so stale queued applies cannot
/// resurrect closed windows. No table lock is held during Webview operations.
fn apply(handle: &AppHandle, label: &str) {
    let Some(manager) = handle.try_state::<WindowManager>() else {
        return;
    };
    let Some(facts) = manager.table.lock().facts(label) else {
        return;
    };
    let Some(window) = handle.get_webview_window(label) else {
        return;
    };

    let presented = window.is_visible().unwrap_or(false) || window.is_minimized().unwrap_or(false);
    let actions = facts.actions(presented);

    if actions.first_ready {
        engine::on_first_ready(&window);
    }

    if actions.show {
        engine::set_in_sight(&window, true);
        crate::trace_err!(window.unminimize(), "set win unminimize");
        crate::trace_err!(window.show(), "set win visible");
        crate::trace_err!(window.set_focus(), "set win focus");
    } else if actions.retire {
        if actions.dismiss {
            facts.hooks.on_dismissed(&window);
        }
        // Read the actor-owned, persisted application snapshot, exactly as
        // ref's WindowManager reads its application client. Missing/unavailable
        // settings keep the existing Destroy default.
        let settings = handle
            .try_state::<crate::client::ChimeraClient>()
            .and_then(|client| client.get_app_config().ok())
            .map(|config| config.window_close)
            .unwrap_or_default();
        match facts
            .hooks
            .close_override(&settings)
            .resolve(settings.global)
        {
            WindowCloseBehavior::Destroy => {
                crate::trace_err!(window.destroy(), "destroy window");
            }
            WindowCloseBehavior::Hide if actions.dismiss => {
                crate::trace_err!(window.hide(), "hide window");
                engine::set_in_sight(&window, false);
            }
            WindowCloseBehavior::Hide => {}
        }
    }
}

/// Webview2 remains visible independently of a native minimized window.
/// The monitor's synchronous controller call does not dispatch window events.
fn follow_minimized(window: &WebviewWindow) {
    if window.is_minimized().unwrap_or(false) {
        engine::set_in_sight(window, false);
    } else if window.is_visible().unwrap_or(false) {
        engine::set_in_sight(window, true);
    }
}
