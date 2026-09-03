//! Session-scoped network port resolution helpers.
//!
//! Chimera still stores port strategies in the legacy config model, so this
//! module centralizes the existing mixed-port and external-controller
//! selection behavior behind the same client boundary used by REF.

use std::net::TcpListener;

use anyhow::{Result, anyhow, bail};
use chimera_config::clash::config::clash_strategy::PortStrategyKind;

use crate::{
    client::ChimeraClient,
    config::{
        chimera::{ExternalControllerPortStrategy, IVerge},
        core::Config,
    },
};

fn find_unused_port(fallback_port: u16) -> Result<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => Ok(listener.local_addr()?.port()),
        Err(_) => {
            log::warn!(target: "app", "use default mixed port: {fallback_port}");
            Ok(fallback_port)
        }
    }
}

pub(crate) fn resolve_random_mixed_port(client: &ChimeraClient) -> Result<()> {
    let clash = client.get_clash_config()?;
    if clash.mixed_port.kind != PortStrategyKind::Random {
        return Ok(());
    }

    let fallback_port = clash.mixed_port.start_port;
    let port = find_unused_port(fallback_port).unwrap_or(fallback_port);

    Config::verge().data().patch_config(IVerge {
        verge_mixed_port: Some(port),
        ..IVerge::default()
    });
    let _ = Config::verge().data().save_file();

    let mut mapping = serde_yaml::Mapping::new();
    mapping.insert("mixed-port".into(), port.into());
    Config::clash().data().patch_config(mapping);
    let _ = Config::clash().latest().prepare_external_controller_port();
    let _ = Config::clash().data().save_config();
    Ok(())
}

pub(crate) fn get_clash_external_port(
    strategy: &ExternalControllerPortStrategy,
    port: u16,
) -> Result<u16> {
    match strategy {
        ExternalControllerPortStrategy::Fixed => {
            if !port_scanner::local_port_available(port) {
                bail!("Port {} is not available", port);
            }
        }
        ExternalControllerPortStrategy::Random | ExternalControllerPortStrategy::AllowFallback => {
            if ExternalControllerPortStrategy::AllowFallback == *strategy
                && port_scanner::local_port_available(port)
            {
                return Ok(port);
            }
            let new_port = port_scanner::request_open_port()
                .ok_or_else(|| anyhow!("Can't find an open port"))?;
            return Ok(new_port);
        }
    }
    Ok(port)
}
