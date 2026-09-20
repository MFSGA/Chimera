//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use tauri::{Manager, Runtime};

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ChimeraClient, ClientSetupArgs, LegacyBridgeSet, LegacyProfileFsPort, LegacyUiEventSink,
        OsSystemDnsCache, ProfilesClient,
    },
    utils::path::PathResolver,
};

pub fn setup<R: Runtime, M: Manager<R>>(app: &M) -> anyhow::Result<()> {
    let paths = PathResolver::from_env().context("failed to resolve app paths")?;
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
    let core_facade = Arc::new(crate::core::actor_v2::CoreFacade::new_local());
    let profile_files = Arc::new(LegacyProfileFsPort);
    let profiles = Arc::new(
        tauri::async_runtime::block_on(ProfilesClient::spawn(profile_files.clone()))
            .context("failed to setup profiles actor")?,
    );
    let client = ChimeraClient::try_new_with_args(ClientSetupArgs {
        paths,
        bridges,
        core: Arc::new(crate::client::core_lifecycle::LegacyCoreBridge::new(
            core_facade.clone(),
        )),
        service: Arc::new(crate::client::core_lifecycle::LegacyServiceBridge::new(
            core_facade,
        )),
        profiles: profiles.clone(),
        profile_files,
        profile_writes: profiles,
        system_dns: Arc::new(OsSystemDnsCache),
        ui_sink: Arc::new(LegacyUiEventSink),
    })?;
    app.manage(client);
    Ok(())
}
