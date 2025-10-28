// use super::tray::Tray;
use crate::log_err;
use anyhow::{Result, bail};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Wry};

#[derive(Debug, Default, Clone)]
pub struct Handle {
    pub app_handle: Arc<Mutex<Option<AppHandle>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles,
    Proxies,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Message {
    SetConfig(Result<(), String>),
}

// todo
const STATE_CHANGED_URI: &str = "nyanpasu://mutation";
const NOTIFY_MESSAGE_URI: &str = "nyanpasu://notice-message";

impl Handle {
    pub fn global() -> &'static Handle {
        static HANDLE: OnceCell<Handle> = OnceCell::new();

        HANDLE.get_or_init(|| Handle {
            app_handle: Arc::new(Mutex::new(None)),
        })
    }

    pub fn get_window(&self) -> Option<WebviewWindow<Wry>> {
        self.app_handle
            .lock()
            .as_ref()
            .and_then(|a| a.get_webview_window("main"))
    }

    pub fn notice_message(message: &Message) {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(NOTIFY_MESSAGE_URI, message));
        }
    }

    pub fn refresh_clash() {
        if let Some(window) = Self::global().get_window() {
            log_err!(window.emit(STATE_CHANGED_URI, StateChanged::ClashConfig));
        }
    }
}
