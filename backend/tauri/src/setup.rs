//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use tauri::{Manager, Runtime};

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ChimeraClient, ClientSetupArgs, LegacyBridgeSet, LegacyProfilesReadPort, LegacyUiEventSink,
        OsSystemDnsCache, RuntimePaths,
    },
    utils::path::PathResolver,
};

pub fn setup<R: Runtime, M: Manager<R>>(app: &M) -> anyhow::Result<()> {
    let paths = PathResolver::from_env().context("failed to resolve app paths")?;

    let runtime_paths = RuntimePaths::from_resolver(&paths)?;
    // Keep the upstream Profile document cutover gated while the client still
    // reads and writes the legacy Profile schema.
    let mut migrations =
        crate::core::migration::Runner::with_paths_before_profile_client_migration(
            paths.clone(),
            false,
        )
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
        profiles: Arc::new(LegacyProfilesReadPort),
        // profile_files: Arc::new(LegacyProfileFsPort),
        // profile_writes: Arc::new(LegacyProfilesWritePort),
        system_dns: Arc::new(OsSystemDnsCache),
        ui_sink: Arc::new(LegacyUiEventSink),
    })?;
    app.manage(client);
    Ok(())
}
