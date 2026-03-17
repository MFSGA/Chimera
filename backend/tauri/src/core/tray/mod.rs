use anyhow::Result;
use tauri::{
    AppHandle, Runtime,
    menu::{Menu, MenuBuilder, MenuEvent},
    tray::{MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent},
};

use crate::utils::resolve;

const TRAY_ID: &str = "main-tray";

pub struct Tray;

impl Tray {
    pub fn tray_menu<R: Runtime>(app_handle: &AppHandle<R>) -> Result<Menu<R>> {
        let menu = MenuBuilder::new(app_handle)
            .text("open_window", "Open")
            .separator()
            .text("quit", "Quit")
            .build()?;

        Ok(menu)
    }

    pub fn update_systray(app_handle: &AppHandle<tauri::Wry>) -> Result<()> {
        let menu = Self::tray_menu(app_handle)?;
        match app_handle.tray_by_id(TRAY_ID) {
            Some(tray) => {
                tray.set_menu(Some(menu))?;
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

        Ok(())
    }

    pub fn update_part<R: Runtime>(_app_handle: &AppHandle<R>) -> Result<()> {
        Ok(())
    }

    pub fn on_menu_item_event(app_handle: &AppHandle, event: MenuEvent) {
        match event.id().0.as_str() {
            "open_window" => resolve::create_window(app_handle),
            "quit" => app_handle.exit(0),
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
