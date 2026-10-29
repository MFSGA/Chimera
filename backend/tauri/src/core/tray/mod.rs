use anyhow::Result;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use chimera_config::clash::config::overrides::Mode;

use crate::{
    client::{
        ChimeraClient,
        effects::plan::{ApplicationEffectInputs, TrayRefresh, TrayView},
    },
    feat, ipc,
    utils::{help, resolve},
};

const TRAY_ID: &str = "main-tray";

struct TrayState<R: Runtime> {
    menu: Mutex<Menu<R>>,
}

#[derive(Default)]
struct PendingTrayRefresh {
    full: bool,
    part: bool,
}

impl PendingTrayRefresh {
    fn merge(&mut self, refresh: TrayRefresh) {
        match refresh {
            TrayRefresh::Full => self.full = true,
            TrayRefresh::Part => self.part = true,
        }
    }

    fn take(&mut self) -> Option<TrayRefresh> {
        if std::mem::take(&mut self.full) {
            self.part = false;
            Some(TrayRefresh::Full)
        } else if std::mem::take(&mut self.part) {
            Some(TrayRefresh::Part)
        } else {
            None
        }
    }
}

struct TrayViewQueue {
    view: TrayView,
    pending: PendingTrayRefresh,
    /// A drain is scheduled or running. New requests merge until it finishes.
    scheduled: bool,
}

struct TrayViewState {
    queue: Mutex<TrayViewQueue>,
}

impl TrayViewState {
    fn new(view: TrayView) -> Self {
        Self {
            queue: Mutex::new(TrayViewQueue {
                view,
                pending: PendingTrayRefresh::default(),
                scheduled: false,
            }),
        }
    }

    fn store(&self, view: TrayView) {
        self.queue.lock().view = view;
    }

    /// Merges work and returns whether this caller must schedule the drain.
    fn request(&self, refresh: TrayRefresh) -> bool {
        let mut queue = self.queue.lock();
        queue.pending.merge(refresh);
        !std::mem::replace(&mut queue.scheduled, true)
    }

    /// Returns pending work with the latest snapshot, or marks the drain idle.
    fn take(&self) -> Option<(TrayRefresh, TrayView)> {
        let mut queue = self.queue.lock();
        let Some(refresh) = queue.pending.take() else {
            queue.scheduled = false;
            return None;
        };
        Some((refresh, queue.view))
    }

    fn schedule_failed(&self) {
        self.queue.lock().scheduled = false;
    }
}

pub struct Tray;

impl Tray {
    pub fn tray_menu<R: Runtime>(app_handle: &AppHandle<R>) -> Result<Menu<R>> {
        let menu = MenuBuilder::new(app_handle)
            .text("open_window", t!("tray.dashboard"))
            .separator()
            .check("rule_mode", t!("tray.rule_mode"))
            .check("global_mode", t!("tray.global_mode"))
            .check("direct_mode", t!("tray.direct_mode"))
            .separator()
            .check("system_proxy", t!("tray.system_proxy"))
            .check("tun_mode", t!("tray.tun_mode"))
            .separator()
            .text("restart_clash", t!("tray.more.restart_clash"))
            .text("restart_app", t!("tray.more.restart_app"))
            .separator()
            .text("open_app_config_dir", t!("tray.open_dir.app_config_dir"))
            .text("open_app_data_dir", t!("tray.open_dir.app_data_dir"))
            .text("open_core_dir", t!("tray.open_dir.core_dir"))
            .text("open_logs_dir", t!("tray.open_dir.log_dir"))
            .separator()
            .text("quit", t!("tray.quit"))
            .build()?;

        Ok(menu)
    }

    pub fn update_systray(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        Self::request_refresh(app_handle, TrayRefresh::Full)
    }

    pub(crate) fn store_view<R: Runtime>(app_handle: &AppHandle<R>, view: TrayView) {
        if let Some(state) = app_handle.try_state::<TrayViewState>() {
            state.store(view);
        } else {
            let _ = app_handle.manage(TrayViewState::new(view));
            if let Some(state) = app_handle.try_state::<TrayViewState>() {
                state.store(view);
            }
        }
    }

    pub(crate) fn request_refresh(
        app_handle: &AppHandle<tauri::Wry>,
        refresh: TrayRefresh,
    ) -> Result<()> {
        if app_handle.try_state::<TrayViewState>().is_none() {
            let view = Self::current_view(app_handle)?;
            let _ = app_handle.manage(TrayViewState::new(view));
        }
        let state = app_handle
            .try_state::<TrayViewState>()
            .ok_or_else(|| anyhow::anyhow!("tray view state is not managed"))?;
        if !state.request(refresh) {
            return Ok(());
        }

        let handle = app_handle.clone();
        if let Err(error) = app_handle.run_on_main_thread(move || Self::drain(&handle)) {
            state.schedule_failed();
            return Err(error.into());
        }
        Ok(())
    }

    fn drain(app_handle: &AppHandle<tauri::Wry>) {
        let Some(state) = app_handle.try_state::<TrayViewState>() else {
            return;
        };
        while let Some((refresh, view)) = state.take() {
            let result = match refresh {
                TrayRefresh::Full => Self::render_systray_with_view(app_handle, view),
                TrayRefresh::Part => Self::render_part_with_view(app_handle, view),
            };
            if let Err(error) = result {
                tracing::warn!(%error, ?refresh, "failed to refresh the system tray");
            }
        }
    }

    fn render_systray_with_view(app_handle: &AppHandle<tauri::Wry>, view: TrayView) -> Result<()> {
        let menu = Self::tray_menu(app_handle)?;
        match app_handle.tray_by_id(TRAY_ID) {
            Some(tray) => {
                tray.set_menu(Some(menu.clone()))?;
                tray.set_visible(true)?;
            }
            None => {
                let mut builder = TrayIconBuilder::with_id(TRAY_ID);
                if let Some(icon) = app_handle.default_window_icon().cloned() {
                    builder = builder.icon(icon);
                }
                builder
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        Self::on_menu_item_event(app, event);
                    })
                    .on_tray_icon_event(|tray_icon, event| {
                        Self::on_system_tray_event(tray_icon, event);
                    })
                    .build(app_handle)?;
            }
        }
        match app_handle.try_state::<TrayState<tauri::Wry>>() {
            Some(state) => {
                *state.menu.lock() = menu;
            }
            None => {
                app_handle.manage(TrayState {
                    menu: Mutex::new(menu),
                });
            }
        }

        Self::render_part_with_view(app_handle, view)?;
        Ok(())
    }

    pub fn update_part(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        Self::request_refresh(app_handle, TrayRefresh::Part)
    }

    fn render_part_with_view(app_handle: &AppHandle<tauri::Wry>, view: TrayView) -> Result<()> {
        if app_handle.tray_by_id(TRAY_ID).is_none() {
            return Ok(());
        }

        let state = match app_handle.try_state::<TrayState<tauri::Wry>>() {
            Some(state) => state,
            None => return Ok(()),
        };
        let menu = state.menu.lock();
        let _ = menu.get("rule_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(view.part.mode == Mode::Rule)
                .ok()
        });
        let _ = menu.get("global_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(view.part.mode == Mode::Global)
                .ok()
        });
        let _ = menu.get("direct_mode").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(view.part.mode == Mode::Direct)
                .ok()
        });
        let _ = menu.get("system_proxy").and_then(|item| {
            item.as_check_menuitem()?
                .set_checked(view.part.system_proxy)
                .ok()
        });
        let _ = menu
            .get("tun_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(view.part.tun).ok());

        Ok(())
    }

    fn current_view<R: Runtime>(app_handle: &AppHandle<R>) -> Result<TrayView> {
        let client = app_handle
            .try_state::<ChimeraClient>()
            .ok_or_else(|| anyhow::anyhow!("chimera client is not managed"))?;
        let app = client.get_app_config()?;
        let clash = client.get_clash_config()?;

        Ok(ApplicationEffectInputs::project(&app, &clash, None).tray_view())
    }

    pub fn on_menu_item_event(app_handle: &AppHandle, event: MenuEvent) {
        match event.id().0.as_str() {
            "open_window" => resolve::create_window(app_handle),
            "rule_mode" => feat::change_clash_mode(app_handle, "rule".to_string()),
            "global_mode" => feat::change_clash_mode(app_handle, "global".to_string()),
            "direct_mode" => feat::change_clash_mode(app_handle, "direct".to_string()),
            "system_proxy" => feat::toggle_system_proxy(app_handle),
            "tun_mode" => feat::toggle_tun_mode(app_handle),
            "open_app_config_dir" => crate::log_err!(ipc::open_app_config_dir()),
            "open_app_data_dir" => crate::log_err!(ipc::open_app_data_dir()),
            "open_core_dir" => crate::log_err!(ipc::open_core_dir()),
            "open_logs_dir" => crate::log_err!(ipc::open_logs_dir()),
            "restart_clash" => feat::restart_clash_core(app_handle),
            "restart_app" => help::restart_application(app_handle),
            "quit" => {
                help::quit_application(app_handle);
            }
            _ => {}
        }
    }

    pub fn on_system_tray_event(tray_icon: &TrayIcon, event: TrayIconEvent) {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            ..
        } = event
        {
            resolve::create_window(tray_icon.app_handle());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::application::{ClashCore, ProxiesSelectorMode, TrayMenuMode};

    fn tray_view(mode: Mode, tun: bool) -> TrayView {
        TrayView {
            menu: crate::client::effects::plan::TrayMenuDesired {
                menu_mode: TrayMenuMode::Native,
                selector_mode: ProxiesSelectorMode::Normal,
                core: ClashCore::default(),
            },
            part: crate::client::effects::plan::TrayPartDesired {
                mode,
                system_proxy: false,
                tun,
                text: false,
                traffic: false,
            },
        }
    }

    #[test]
    fn queued_refresh_uses_the_latest_view_and_serializes_new_requests() {
        let old = tray_view(Mode::Rule, false);
        let latest = tray_view(Mode::Global, true);
        let state = TrayViewState::new(old);

        assert!(state.request(TrayRefresh::Part));
        let (refresh, view) = state.take().unwrap();
        assert_eq!(refresh, TrayRefresh::Part);
        assert_eq!(view, old);

        state.store(latest);
        assert!(!state.request(TrayRefresh::Part));
        assert_eq!(state.take(), Some((TrayRefresh::Part, latest)));
        assert_eq!(state.take(), None);
        assert!(state.request(TrayRefresh::Part));
    }

    #[test]
    fn full_refresh_supersedes_queued_partial_refresh() {
        let latest = tray_view(Mode::Global, true);
        let state = TrayViewState::new(tray_view(Mode::Rule, false));

        assert!(state.request(TrayRefresh::Part));
        assert!(!state.request(TrayRefresh::Full));
        state.store(latest);

        assert_eq!(state.take(), Some((TrayRefresh::Full, latest)));
        assert_eq!(state.take(), None);
    }
}
