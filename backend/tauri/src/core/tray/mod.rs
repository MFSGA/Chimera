mod display;
mod executor;
pub(crate) mod icon;
mod proxies;

pub use self::executor::TrayWork;
use self::{
    display::{Paint, ProxySection, Publication, Shown, TrayDisplay},
    executor::{Step, TrayQueue, TrayTarget},
    proxies::SystemTrayMenuProxiesExt,
};

use std::{borrow::Cow, ops::ControlFlow};

use anyhow::Result;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use chimera_config::{application::ClashCore, clash::config::overrides::Mode};

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
    view: Mutex<TrayView>,
    queue: Mutex<TrayQueue>,
    display: Mutex<TrayDisplay<Attached<R>>>,
}

struct Attached<R: Runtime> {
    menu: Menu<R>,
}

impl<R: Runtime> Clone for Attached<R> {
    fn clone(&self) -> Self {
        Self {
            menu: self.menu.clone(),
        }
    }
}

impl<R: Runtime> TrayState<R> {
    fn new(view: TrayView) -> Self {
        Self {
            view: Mutex::new(view),
            queue: Mutex::new(TrayQueue::default()),
            display: Mutex::new(TrayDisplay::new()),
        }
    }
}

pub struct Tray;

#[derive(Clone, Copy)]
struct ClashModeMenuItem {
    id: &'static str,
    mode: Mode,
}

const CLASH_MODE_MENU_ITEMS: [ClashModeMenuItem; 3] = [
    ClashModeMenuItem {
        id: "rule_mode",
        mode: Mode::Rule,
    },
    ClashModeMenuItem {
        id: "global_mode",
        mode: Mode::Global,
    },
    ClashModeMenuItem {
        id: "direct_mode",
        mode: Mode::Direct,
    },
];

const CLASH_PREMIUM_MODE_MENU_ITEMS: [ClashModeMenuItem; 4] = [
    CLASH_MODE_MENU_ITEMS[0],
    CLASH_MODE_MENU_ITEMS[1],
    CLASH_MODE_MENU_ITEMS[2],
    ClashModeMenuItem {
        id: "script_mode",
        mode: Mode::Script,
    },
];

fn mode_menu_items(core: ClashCore) -> &'static [ClashModeMenuItem] {
    if core == ClashCore::ClashPremium {
        &CLASH_PREMIUM_MODE_MENU_ITEMS
    } else {
        &CLASH_MODE_MENU_ITEMS
    }
}

fn mode_menu_label(mode: Mode) -> Cow<'static, str> {
    match mode {
        Mode::Rule => t!("tray.rule_mode"),
        Mode::Global => t!("tray.global_mode"),
        Mode::Direct => t!("tray.direct_mode"),
        Mode::Script => t!("tray.script_mode"),
    }
}

fn mode_menu_checks(core: ClashCore, mode: Mode) -> impl Iterator<Item = (&'static str, bool)> {
    mode_menu_items(core)
        .iter()
        .map(move |item| (item.id, mode == item.mode))
}

impl Tray {
    fn tray_menu<R: Runtime>(
        app_handle: &AppHandle<R>,
        view: TrayView,
    ) -> Result<(Menu<R>, ProxySection)> {
        let mut menu = MenuBuilder::new(app_handle)
            .text("open_window", t!("tray.dashboard"))
            .separator();

        for item in mode_menu_items(view.menu.core) {
            menu = menu.check(item.id, mode_menu_label(item.mode));
        }

        let (menu, section) = menu.setup_proxies(app_handle, &view)?;
        let menu = menu
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

        Ok((menu, section))
    }

    pub fn update_systray(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        #[cfg(any(windows, target_os = "linux"))]
        if let Some(paths) = app_handle.try_state::<chimera_paths::PathResolver>() {
            icon::resize_images(&paths, Self::max_scale_factor(app_handle));
        }
        Self::request_refresh(app_handle, TrayRefresh::Full)
    }

    pub(crate) fn store_view<R: Runtime>(app_handle: &AppHandle<R>, view: TrayView) {
        if app_handle.try_state::<TrayState<R>>().is_none() {
            let _ = app_handle.manage(TrayState::<R>::new(view));
        }
        if let Some(state) = app_handle.try_state::<TrayState<R>>() {
            *state.view.lock() = view;
        }
    }

    pub(crate) fn request_refresh(
        app_handle: &AppHandle<tauri::Wry>,
        refresh: TrayRefresh,
    ) -> Result<()> {
        Self::request(
            app_handle,
            match refresh {
                TrayRefresh::Full => TrayWork::REBUILD,
                TrayRefresh::Part => TrayWork::PART,
            },
        )
    }

    pub(crate) fn request(app_handle: &AppHandle<tauri::Wry>, work: TrayWork) -> Result<()> {
        if app_handle.try_state::<TrayState<tauri::Wry>>().is_none() {
            Self::store_view(app_handle, Self::current_view(app_handle)?);
        }
        let state = app_handle.state::<TrayState<tauri::Wry>>();
        let handle = app_handle.clone();
        executor::request(&state.queue, work, || {
            app_handle.run_on_main_thread(move || {
                if let Some(state) = handle.try_state::<TrayState<tauri::Wry>>() {
                    executor::drain(
                        &state.queue,
                        &mut LiveTray {
                            app_handle: &handle,
                            state: &state,
                        },
                    );
                }
            })
        })?;
        Ok(())
    }

    fn rebuild(app_handle: &AppHandle<tauri::Wry>, state: &TrayState<tauri::Wry>) -> Result<()> {
        let view = *state.view.lock();
        let (menu, section) = match Self::tray_menu(app_handle, view) {
            Ok(built) => built,
            Err(error) => {
                state.display.lock().published(Publication::NotStarted);
                return Err(error);
            }
        };
        let mut attached = state.display.lock().attached();
        let result = Self::attach(app_handle, menu, &mut attached);
        state.display.lock().published(match (&result, attached) {
            (Ok(()), Some(attached)) => Publication::Complete { attached, section },
            (_, attached) => Publication::Interrupted { attached },
        });
        result
    }

    fn attach(
        app_handle: &AppHandle<tauri::Wry>,
        menu: Menu<tauri::Wry>,
        attached: &mut Option<Attached<tauri::Wry>>,
    ) -> Result<()> {
        let tray = match app_handle.tray_by_id(TRAY_ID) {
            Some(tray) => {
                if cfg!(target_os = "linux") {
                    if let Some(previous) = attached.as_ref() {
                        refill_menu(&previous.menu, &menu)?;
                    } else {
                        tray.set_menu(Some(menu.clone()))?;
                        *attached = Some(Attached { menu });
                    }
                } else {
                    tray.set_menu(Some(menu.clone()))?;
                    *attached = Some(Attached { menu });
                }
                tray
            }
            None => {
                *attached = None;
                let mut builder = TrayIconBuilder::with_id(TRAY_ID);
                #[cfg(all(windows, feature = "e2e"))]
                {
                    builder = builder.tooltip(format!("Chimera E2E {}", std::process::id()));
                }
                if let Some(icon) = app_handle.default_window_icon().cloned() {
                    builder = builder.icon(icon);
                }
                let tray = builder
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(Self::on_menu_item_event)
                    .on_tray_icon_event(Self::on_system_tray_event)
                    .build(app_handle)?;
                *attached = Some(Attached { menu });
                tray
            }
        };
        tray.set_visible(true)?;
        Ok(())
    }

    pub fn update_part(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        Self::request_refresh(app_handle, TrayRefresh::Part)
    }

    pub(crate) fn max_scale_factor(app_handle: &AppHandle<tauri::Wry>) -> f64 {
        app_handle
            .available_monitors()
            .map(|monitors| {
                monitors
                    .iter()
                    .map(|monitor| monitor.scale_factor())
                    .fold(1.0, f64::max)
            })
            .unwrap_or(1.0)
    }

    #[cfg(any(windows, target_os = "linux"))]
    pub(crate) fn on_scale_factor_changed(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        if let Some(paths) = app_handle.try_state::<chimera_paths::PathResolver>() {
            icon::resize_images(&paths, Self::max_scale_factor(app_handle));
        }
        Self::update_part(app_handle)
    }

    fn repaint_part(
        app_handle: &AppHandle<tauri::Wry>,
        state: &TrayState<tauri::Wry>,
    ) -> Result<ControlFlow<()>> {
        let tray = app_handle.tray_by_id(TRAY_ID);
        let paint = state.display.lock().paint();
        if executor::needs_rebuild(&paint, tray.is_some()) {
            Self::request(app_handle, TrayWork::REBUILD)?;
            return Ok(ControlFlow::Break(()));
        }
        let (Paint::Menu(attached), Some(_tray)) = (paint, tray) else {
            return Ok(ControlFlow::Break(()));
        };
        let view = *state.view.lock();
        let menu = attached.menu;
        let check = |id: &str, checked: bool| {
            menu.get(id)
                .and_then(|item| item.as_check_menuitem()?.set_checked(checked).ok())
                .is_some()
        };
        let mut whole = true;
        for (id, checked) in mode_menu_checks(view.menu.core, view.part.mode) {
            whole &= check(id, checked);
        }
        whole &= check("system_proxy", view.part.system_proxy);
        whole &= check("tun_mode", view.part.tun);
        let shown = if whole { Shown::Whole } else { Shown::Partly };
        state.display.lock().repainted(None, shown);

        #[cfg(any(windows, target_os = "linux"))]
        if let Some(paths) = app_handle.try_state::<chimera_paths::PathResolver>() {
            use chimera_rpc_protocol::values::TrayIcon;
            let mode = if view.part.tun {
                TrayIcon::Tun
            } else if view.part.system_proxy {
                TrayIcon::SystemProxy
            } else {
                TrayIcon::Normal
            };
            let bytes = icon::get_icon(&paths, &mode);
            _tray.set_icon(Some(tauri::image::Image::from_bytes(&bytes)?))?;
        }

        Ok(if whole {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        })
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
            "script_mode" => feat::change_clash_mode(app_handle, "script".to_string()),
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
            id => proxies::on_system_tray_event(app_handle, id),
        }
    }

    pub fn on_system_tray_event(tray_icon: &TrayIcon, event: TrayIconEvent) {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: tauri::tray::MouseButtonState::Up,
            ..
        } = event
        {
            resolve::create_window(tray_icon.app_handle());
        }
    }
}

struct LiveTray<'a> {
    app_handle: &'a AppHandle<tauri::Wry>,
    state: &'a TrayState<tauri::Wry>,
}

impl TrayTarget for LiveTray<'_> {
    type Menu = Attached<tauri::Wry>;
    fn observe(&self) -> (Paint<Self::Menu>, bool) {
        (
            self.state.display.lock().paint(),
            self.app_handle.tray_by_id(TRAY_ID).is_some(),
        )
    }
    fn run(&mut self, step: Step) -> ControlFlow<()> {
        let result = match step {
            Step::Rebuild => Tray::rebuild(self.app_handle, self.state).map(ControlFlow::Continue),
            Step::Part => Tray::repaint_part(self.app_handle, self.state),
            Step::Proxies => proxies::repaint_proxies(self.app_handle, self.state),
        };
        result.unwrap_or_else(|error| {
            tracing::warn!(%error, ?step, "failed to refresh the system tray");
            ControlFlow::Break(())
        })
    }
}

fn refill_menu(previous_menu: &Menu<tauri::Wry>, menu: &Menu<tauri::Wry>) -> Result<()> {
    let mut whole = true;
    if let Ok(items) = previous_menu.items() {
        tracing::debug!("removing previous tray menu items");
        for item in items {
            let removed = previous_menu.remove(&item);
            whole &= removed.is_ok();
            crate::log_err!(removed, "failed to remove menu item");
        }
    } else {
        whole = false;
    }
    // migrate the menu items
    if let Ok(items) = menu.items() {
        tracing::debug!("migrating new tray menu items");
        for item in items {
            let appended = previous_menu.append(&item);
            whole &= appended.is_ok();
            crate::log_err!(appended, "failed to append menu item");
        }
    } else {
        whole = false;
    }
    anyhow::ensure!(whole, "the tray menu was only partly replaced");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::application::ClashCore;

    #[test]
    fn script_mode_menu_item_is_only_defined_for_the_clash_premium_core() {
        assert_eq!(
            mode_menu_items(ClashCore::ClashPremium)
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            ["rule_mode", "global_mode", "direct_mode", "script_mode"]
        );

        for core in [
            ClashCore::ClashRs,
            ClashCore::ClashRsAlpha,
            ClashCore::Mihomo,
            ClashCore::MihomoAlpha,
            ClashCore::ChimeraClient,
            ClashCore::ChimeraClientAlpha,
        ] {
            assert!(
                mode_menu_items(core)
                    .iter()
                    .all(|item| item.id != "script_mode"),
                "{core:?}"
            );
        }
    }

    #[test]
    fn mode_menu_check_tracks_script_and_other_modes() {
        assert_eq!(
            mode_menu_checks(ClashCore::ClashPremium, Mode::Script)
                .filter_map(|(id, checked)| checked.then_some(id))
                .collect::<Vec<_>>(),
            ["script_mode"]
        );
        assert_eq!(
            mode_menu_checks(ClashCore::ClashPremium, Mode::Direct)
                .filter_map(|(id, checked)| checked.then_some(id))
                .collect::<Vec<_>>(),
            ["direct_mode"]
        );
        assert!(mode_menu_checks(ClashCore::Mihomo, Mode::Script).all(|(_, checked)| !checked));
    }
}
