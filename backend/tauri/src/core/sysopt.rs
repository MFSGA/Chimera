use std::sync::Arc;

use anyhow::Result;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use sysproxy::Sysproxy;
use tauri::async_runtime::Mutex as TokioMutex;

use crate::{config::core::Config, log_err};

#[cfg(target_os = "windows")]
static DEFAULT_BYPASS: &str = "localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>";
#[cfg(target_os = "linux")]
static DEFAULT_BYPASS: &str = "localhost,127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,::1";
#[cfg(target_os = "macos")]
static DEFAULT_BYPASS: &str =
    "127.0.0.1,192.168.0.0/16,10.0.0.0/8,172.16.0.0/12,localhost,*.local,*.crashlytics.com,<local>";

pub struct Sysopt {
    /// current system proxy setting
    cur_sysproxy: Arc<Mutex<Option<Sysproxy>>>,

    /// record the original system proxy
    /// recover it when exit
    old_sysproxy: Arc<Mutex<Option<Sysproxy>>>,

    /// record whether the guard async is running or not
    guard_state: Arc<TokioMutex<bool>>,
}

impl Sysopt {
    pub fn global() -> &'static Sysopt {
        static SYSOPT: OnceCell<Sysopt> = OnceCell::new();

        SYSOPT.get_or_init(|| Sysopt {
            cur_sysproxy: Arc::new(Mutex::new(None)),
            old_sysproxy: Arc::new(Mutex::new(None)),
            guard_state: Arc::new(TokioMutex::new(false)),
            /* auto_launch: Arc::new(Mutex::new(None)),
             */
        })
    }

    /// update the system proxy
    pub fn update_sysproxy(&self) -> Result<()> {
        log::debug!("todo update_sysproxy");

        let mut cur_sysproxy = self.cur_sysproxy.lock();
        let old_sysproxy = self.old_sysproxy.lock();

        if cur_sysproxy.is_none() || old_sysproxy.is_none() {
            drop(cur_sysproxy);
            drop(old_sysproxy);
            return self.init_sysproxy();
        }

        let (enable, bypass) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                verge.enable_system_proxy.unwrap_or(false),
                verge.system_proxy_bypass.clone(),
            )
        };
        let mut sysproxy = cur_sysproxy.take().unwrap();

        sysproxy.enable = enable;
        sysproxy.bypass = bypass.unwrap_or(DEFAULT_BYPASS.into());

        sysproxy.set_system_proxy()?;
        *cur_sysproxy = Some(sysproxy);

        Ok(())
    }

    /// launch a system proxy guard
    /// read config from file directly
    pub fn guard_proxy(&self) {
        use tokio::time::{Duration, sleep};

        let guard_state = self.guard_state.clone();

        tauri::async_runtime::spawn(async move {
            // if it is running, exit
            let mut state = guard_state.lock().await;
            if *state {
                return;
            }
            *state = true;
            drop(state);

            // default duration is 10s
            let mut wait_secs = 10u64;

            loop {
                sleep(Duration::from_secs(wait_secs)).await;

                let (enable, guard, guard_interval, bypass) = {
                    let verge = Config::verge();
                    let verge = verge.latest();
                    (
                        verge.enable_system_proxy.unwrap_or(false),
                        verge.enable_proxy_guard.unwrap_or(false),
                        verge.proxy_guard_interval.unwrap_or(10),
                        verge.system_proxy_bypass.clone(),
                    )
                };

                // stop loop
                if !enable || !guard {
                    break;
                }

                // update duration
                wait_secs = guard_interval;

                log::debug!(target: "app", "try to guard the system proxy");

                let port = {
                    Config::verge()
                        .latest()
                        .verge_mixed_port
                        .unwrap_or(Config::clash().data().get_mixed_port())
                };

                let sysproxy = Sysproxy {
                    // todo: changed to true
                    enable: false,
                    host: "127.0.0.1".into(),
                    port,
                    bypass: bypass.unwrap_or(DEFAULT_BYPASS.into()),
                };

                log_err!(sysproxy.set_system_proxy());
            }

            let mut state = guard_state.lock().await;
            *state = false;
            drop(state);
        });
    }

    /// init the sysproxy
    pub fn init_sysproxy(&self) -> Result<()> {
        let port = Config::verge()
            .latest()
            .verge_mixed_port
            .unwrap_or(Config::clash().data().get_mixed_port());

        let (enable, bypass) = {
            let verge = Config::verge();
            let verge = verge.latest();
            (
                verge.enable_system_proxy.unwrap_or(false),
                verge.system_proxy_bypass.clone(),
            )
        };

        let current = Sysproxy {
            enable,
            host: String::from("127.0.0.1"),
            port,
            bypass: bypass.unwrap_or(DEFAULT_BYPASS.into()),
        };

        if enable {
            let old = Sysproxy::get_system_proxy().ok();
            if let Err(e) = current.set_system_proxy() {
                log::error!(target: "app", "Failed to set system proxy: {}", e);
                return Err(e.into()); // Convert sysproxy::Error to anyhow::Error
            }

            *self.old_sysproxy.lock() = old;
            *self.cur_sysproxy.lock() = Some(current);
        }

        // run the system proxy guard
        self.guard_proxy();
        Ok(())
    }
}
