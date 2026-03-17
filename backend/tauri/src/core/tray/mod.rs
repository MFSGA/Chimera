use anyhow::Result;
use parking_lot::Mutex;
use rust_i18n::t;
use tauri::{
    AppHandle, Manager, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use crate::{
    config::core::Config,
    feat, ipc,
    utils::{help, resolve},
};

const TRAY_ID: &str = "main-tray";

struct TrayState<R: Runtime> {
    menu: Mutex<Menu<R>>,
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

        Self::update_part(app_handle)?;

        Ok(())
    }

    pub fn update_part<R: Runtime>(app_handle: &AppHandle<R>) -> Result<()> {
        let tray = match app_handle.tray_by_id(TRAY_ID) {
            Some(tray) => tray,
            None => return Ok(()),
        };

        let mode = Config::runtime()
            .latest()
            .config
            .as_ref()
            .and_then(|m| m.get("mode"))
            .and_then(|v| v.as_str())
            .unwrap_or("rule")
            .to_string();
        let system_proxy = Config::verge()
            .latest()
            .enable_system_proxy
            .unwrap_or(false);
        let tun_mode = Config::verge().latest().enable_tun_mode.unwrap_or(false);

        let state = match app_handle.try_state::<TrayState<R>>() {
            Some(state) => state,
            None => return Ok(()),
        };
        let menu = state.menu.lock();
        let _ = menu
            .get("rule_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "rule").ok());
        let _ = menu
            .get("global_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "global").ok());
        let _ = menu
            .get("direct_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(mode == "direct").ok());
        let _ = menu
            .get("system_proxy")
            .and_then(|item| item.as_check_menuitem()?.set_checked(system_proxy).ok());
        let _ = menu
            .get("tun_mode")
            .and_then(|item| item.as_check_menuitem()?.set_checked(tun_mode).ok());

        Ok(())
    }

    pub fn on_menu_item_event(app_handle: &AppHandle, event: MenuEvent) {
        match event.id().0.as_str() {
            "open_window" => resolve::create_window(app_handle),
            "rule_mode" => feat::change_clash_mode("rule".to_string()),
            "global_mode" => feat::change_clash_mode("global".to_string()),
            "direct_mode" => feat::change_clash_mode("direct".to_string()),
            "system_proxy" => feat::toggle_system_proxy(),
            "tun_mode" => feat::toggle_tun_mode(),
            "open_app_config_dir" => crate::log_err!(ipc::open_app_config_dir()),
            "open_app_data_dir" => crate::log_err!(ipc::open_app_data_dir()),
            "open_core_dir" => crate::log_err!(ipc::open_core_dir()),
            "open_logs_dir" => crate::log_err!(ipc::open_logs_dir()),
            "restart_clash" => feat::restart_clash_core(),
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
