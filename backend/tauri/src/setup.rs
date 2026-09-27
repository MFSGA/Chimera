//! Application composition root.

use std::sync::Arc;

use anyhow::Context as _;
use tauri::{Manager, Runtime};

use crate::{
    bridge::{clash::LegacyClashBridge, verge::LegacyVergeBridge, window::LegacyWindowBridge},
    client::{
        ChimeraClient, ClientSetupArgs, LegacyBridgeSet, LegacyUiEventSink, OsSystemDnsCache,
        RuntimePaths,
    },
    utils::path::PathResolver,
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

pub fn setup<R: Runtime, M: Manager<R>>(app: &M) -> anyhow::Result<()> {
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
    })?;
    app.manage(client);
    core_facade_monitor.start_service_api_monitor();
    Ok(())
}
