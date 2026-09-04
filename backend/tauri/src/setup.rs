//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use tauri::{Manager, Runtime};

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ChimeraClient, ClientSetupArgs, LegacyBridgeSet, LegacyCoreBridge, LegacyProfileFsPort,
        LegacyProfilesReadPort, LegacyProfilesWritePort, LegacyUiEventSink, OsSystemDnsCache,
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
    let client = ChimeraClient::try_new_with_args(ClientSetupArgs {
        paths,
        bridges,
        core: Arc::new(LegacyCoreBridge),
        profiles: Arc::new(LegacyProfilesReadPort),
        profile_files: Arc::new(LegacyProfileFsPort),
        profile_writes: Arc::new(LegacyProfilesWritePort),
        system_dns: Arc::new(OsSystemDnsCache),
        ui_sink: Arc::new(LegacyUiEventSink),
    })?;
    app.manage(client);
    Ok(())
}
