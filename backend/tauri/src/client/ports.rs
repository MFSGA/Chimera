//! Session-scoped network port resolution helpers.
//!
//! Chimera still stores port strategies in the legacy config model, so this
//! module centralizes the existing mixed-port and external-controller
//! selection behavior behind the same client boundary used by REF.

use std::net::TcpListener;

use anyhow::{Result, anyhow, bail};

use crate::config::{
    chimera::{ExternalControllerPortStrategy, IVerge},
    core::Config,
};

fn find_unused_port() -> Result<u16> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => Ok(listener.local_addr()?.port()),
        Err(_) => {
            let port = Config::verge()
                .latest()
                .verge_mixed_port
                .unwrap_or(Config::clash().data().get_mixed_port());
            log::warn!(target: "app", "use default mixed port: {port}");
            Ok(port)
        }
    }
}

pub(crate) fn resolve_random_mixed_port() {
    let enable_random_port = Config::verge().latest().enable_random_port.unwrap_or(false);

    if !enable_random_port {
        return;
    }

    let fallback_port = Config::verge()
        .latest()
        .verge_mixed_port
        .unwrap_or(Config::clash().data().get_mixed_port());
    let port = find_unused_port().unwrap_or(fallback_port);

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
