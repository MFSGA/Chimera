//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use tauri::Manager;

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ChimeraClient, ClientSetupArgs, LegacyBridgeSet, LegacyUiEventSink, OsSystemDnsCache,
        RuntimePaths,
    },
    utils::path::PathResolver,
};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::client::hotkey::{
    HotkeyArgs, HotkeyClient,
    adapters::{
        ChannelActionSink, PlatformAcceleratorValidator, TauriShortcutRegistrar, TauriWindowControl,
    },
    ports::{HotkeyAction, WindowControl},
};

#[cfg(test)]
use crate::client::{LegacyProfileFsPort, LegacyProfilesReadPort};

struct LegacySelfProxyPort;

impl crate::service::profile_file::SelfProxyPortSource for LegacySelfProxyPort {
    fn mixed_port(&self) -> Option<u16> {
        Some(
            crate::config::core::Config::clash()
                .latest()
                .get_mixed_port(),
        )
    }
}

pub fn setup(app: &mut tauri::App) -> anyhow::Result<()> {
    let paths = PathResolver::from_env().context("failed to resolve app paths")?;
    let profile_service = Arc::new(crate::service::profile_file::ProfileFileService::new(
        paths.clone(),
        Arc::new(LegacySelfProxyPort),
    ));

    let runtime_paths = RuntimePaths::from_resolver(&paths)?;
    let mut migrations = crate::core::migration::Runner::with_paths(paths.clone(), false)
        .context("failed to setup config migrations")?;
    migrations
        .run_pending()
        .context("failed to run config migrations before client setup")?;

    let legacy_lock = Arc::new(parking_lot::Mutex::new(()));
    let bridges = LegacyBridgeSet {
        verge: Arc::new(LegacyVergeBridge::new(legacy_lock.clone())),
        window: Arc::new(LegacyWindowBridge::new(legacy_lock.clone())),
        clash: Arc::new(LegacyClashBridge::new(legacy_lock)),
    };
    app.manage(tokio::sync::RwLock::new(
        crate::core::updater::UpdaterManager::new(),
    ));
    let local_control =
        tauri::async_runtime::block_on(crate::core::actor_v2::local_host::build(&paths))
            .context("failed to initialize the local core control plane")?;
    let core_facade = Arc::new(crate::core::actor_v2::CoreFacade::new_local_with_control(
        local_control,
        runtime_paths.clone(),
    ));
    let core_facade_monitor = core_facade.clone();

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let (hotkey_tx, hotkey_rx) = tokio::sync::mpsc::unbounded_channel();
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let hotkeys = tauri::async_runtime::block_on(HotkeyClient::spawn(HotkeyArgs {
        registrar: Arc::new(TauriShortcutRegistrar::new(app.handle().clone())),
        sink: Arc::new(ChannelActionSink::new(hotkey_tx)),
    }))
    .context("failed to start the global shortcut owner")?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let accelerators = Arc::new(PlatformAcceleratorValidator);
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let effects = Arc::new(crate::client::effects::executor::HotkeyEffectExecutor::new(
        hotkeys,
        accelerators.clone(),
    ));
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    let window: Arc<dyn WindowControl> = Arc::new(TauriWindowControl::new(app.handle().clone()));

    let client = ChimeraClient::try_new_with_args(ClientSetupArgs {
        paths,
        runtime_paths: runtime_paths.clone(),
        bridges,
        core: Arc::new(crate::client::core_lifecycle::LegacyCoreBridge::new(
            core_facade.clone(),
        )),
        service: Arc::new(crate::client::core_lifecycle::LegacyServiceBridge::new(
            core_facade,
        )),
        #[cfg(test)]
        profiles: Arc::new(LegacyProfilesReadPort),
        #[cfg(test)]
        profile_files: Arc::new(LegacyProfileFsPort),
        profile_service,
        system_dns: Arc::new(OsSystemDnsCache),
        ui_sink: Arc::new(LegacyUiEventSink),
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        effects,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        accelerators,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        window,
    })?;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    tauri::async_runtime::spawn(hotkey_action_pump(hotkey_rx, client.clone()));
    app.manage(client);
    core_facade_monitor.start_service_api_monitor();
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn hotkey_action_pump(
    mut actions: tokio::sync::mpsc::UnboundedReceiver<HotkeyAction>,
    client: ChimeraClient,
) {
    while let Some(action) = actions.recv().await {
        if let Err(error) = client.dispatch_hotkey_action(action).await {
            tracing::warn!(%error, %action, "hotkey action failed");
        }
    }
}
