use std::borrow::Borrow;

use anyhow::Result;
use serde_yaml::Mapping;
use tauri::{AppHandle, Manager};

use crate::{
    client::ChimeraClient,
    config::{
        chimera::IVerge, profile::item::remote::RemoteProfileOptionsBuilder,
        runtime::ClashConfigOverrides,
    },
    core::{clash::transaction::TransactionOutcome, handle},
    log_err,
};

/// Applies typed overrides to the running core and desired state through the
/// shared transaction coordinator used by IPC and non-window entry points.
pub async fn patch_running_clash_overrides(
    client: &ChimeraClient,
    overrides: ClashConfigOverrides,
) -> TransactionOutcome {
    client.patch_running_clash_overrides(overrides).await
}

/// Applies a general Clash mapping while extracting only supported persistent
/// runtime overrides for the generated config.
pub async fn patch_clash(client: &ChimeraClient, patch: Mapping) -> Result<()> {
    client.patch_clash(patch).await
}

fn managed_client() -> Result<ChimeraClient> {
    let app_handle = handle::Handle::app_handle()
        .ok_or_else(|| anyhow::anyhow!("app handle is not initialized"))?;
    app_handle
        .try_state::<ChimeraClient>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| anyhow::anyhow!("nyanpasu client is not managed"))
}

/// 修改verge的配置
/// 一般都是一个个的修改
pub async fn patch_verge(patch: IVerge) -> Result<()> {
    managed_client()?.patch_verge(patch).await
}

/// 更新某个profile
/// 如果更新当前配置就激活配置
pub async fn update_profile<T: Borrow<String>>(
    uid: T,
    opts: Option<RemoteProfileOptionsBuilder>,
) -> Result<crate::client::MutationOutcome<()>> {
    managed_client()?
        .refresh_profile(uid.borrow().clone(), opts)
        .await
}

pub fn update_proxies_buff(rx: Option<tokio::sync::oneshot::Receiver<()>>) {
    use crate::core::clash::proxies::{ProxiesGuard, ProxiesGuardExt};

    tauri::async_runtime::spawn(async move {
        if let Some(rx) = rx
            && let Err(e) = rx.await
        {
            log::error!(target: "app::clash::proxies", "update proxies buff by rx failed: {e}");
        }
        match ProxiesGuard::global().update().await {
            Ok(_) => {
                log::debug!(target: "app::clash::proxies", "update proxies buff success");
                handle::Handle::mutate_proxies();
            }
            Err(e) => {
                log::error!(target: "app::clash::proxies", "update proxies buff failed: {e}");
            }
        }
    });
}

pub fn change_clash_mode(app_handle: &AppHandle, mode: String) {
    let app_handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let Some(client) = app_handle.try_state::<ChimeraClient>() else {
            log::error!(target: "app", "nyanpasu client is not managed");
            return;
        };
        let overrides = ClashConfigOverrides {
            mode: Some(mode),
            ..ClashConfigOverrides::default()
        };

        if let Err(error) = patch_running_clash_overrides(&client, overrides)
            .await
            .into_result()
        {
            log::error!(target: "app", "failed to change clash mode transactionally: {error:#}");
        }
    });
}

pub fn toggle_system_proxy() {
    let client = match managed_client() {
        Ok(client) => client,
        Err(err) => {
            log::error!(target: "app", "failed to resolve client for system proxy toggle: {err:?}");
            return;
        }
    };
    let enabled = match client.get_app_config() {
        Ok(config) => config.enable_system_proxy,
        Err(err) => {
            log::error!(target: "app", "failed to read typed app config for system proxy toggle: {err:?}");
            return;
        }
    };

    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_system_proxy: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = client.patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle system proxy: {err:?}");
        }
    });
}

pub fn toggle_tun_mode() {
    let client = match managed_client() {
        Ok(client) => client,
        Err(err) => {
            log::error!(target: "app", "failed to resolve client for tun toggle: {err:?}");
            return;
        }
    };
    let enabled = match client.get_clash_config() {
        Ok(config) => config.enable_tun_mode,
        Err(err) => {
            log::error!(target: "app", "failed to read typed clash config for tun toggle: {err:?}");
            return;
        }
    };

    tauri::async_runtime::spawn(async move {
        let patch = IVerge {
            enable_tun_mode: Some(!enabled),
            ..IVerge::default()
        };
        if let Err(err) = client.patch_verge(patch).await {
            log::error!(target: "app", "failed to toggle tun mode: {err:?}");
        }
    });
}

pub fn restart_clash_core() {
    let client = match managed_client() {
        Ok(client) => client,
        Err(err) => {
            log::error!(target: "app", "failed to resolve client for core restart: {err:?}");
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        if let Err(err) = client.rebuild_running_config().await {
            log::error!(target: "app", "failed to restart clash core: {err:?}");
            return;
        }
        log_err!(handle::Handle::update_systray_part());
    });
}
