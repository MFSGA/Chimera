use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    str::FromStr,
};

use anyhow::Result;
use serde_yaml::{Mapping, Value};
use tracing::instrument;

use crate::{
    config::core::Config,
    utils::{
        dirs,
        help::{self, get_clash_external_port},
    },
};

#[derive(Default, Debug, Clone)]
pub struct IClashTemp(pub Mapping);

impl IClashTemp {
    pub fn template() -> Self {
        let mut map = Mapping::new();

        map.insert("mixed-port".into(), 7890.into());
        map.insert("log-level".into(), "info".into());
        map.insert("allow-lan".into(), false.into());
        map.insert("mode".into(), "rule".into());
        #[cfg(debug_assertions)]
        map.insert("external-controller".into(), "127.0.0.1:9872".into());
        #[cfg(not(debug_assertions))]
        map.insert("external-controller".into(), "127.0.0.1:17650".into());
        map.insert(
            "secret".into(),
            // todo: logging uuid::Uuid::new_v4().to_string().to_lowercase().into(), // generate a uuid v4 as default secret to secure the communication between clash and the client
            "chimera".into(),
        );
        #[cfg(feature = "default-meta")]
        map.insert("unified-delay".into(), true.into());
        #[cfg(feature = "default-meta")]
        map.insert("tcp-concurrent".into(), true.into());
        map.insert("ipv6".into(), false.into());

        Self(map)
    }

    pub fn new() -> Self {
        match dirs::clash_guard_overrides_path().and_then(|path| help::read_merge_mapping(&path)) {
            Ok(map) => Self(Self::guard(map)),
            Err(err) => {
                log::error!(target: "app", "{err:?}");
                Self::template()
            }
        }
    }

    fn guard(mut config: Mapping) -> Mapping {
        let port = Self::guard_mixed_port(&config);
        let ctrl = Self::guard_server_ctrl(&config);

        config.insert("mixed-port".into(), port.into());
        config.insert("external-controller".into(), ctrl.into());
        config
    }

    pub fn guard_mixed_port(config: &Mapping) -> u16 {
        let mut port = config
            .get("mixed-port")
            .and_then(|value| match value {
                Value::String(val_str) => val_str.parse().ok(),
                Value::Number(val_num) => val_num.as_u64().map(|u| u as u16),
                _ => None,
            })
            .unwrap_or(7890);
        if port == 0 {
            port = 7890;
        }
        port
    }

    pub fn guard_server_ctrl(config: &Mapping) -> String {
        config
            .get("external-controller")
            .and_then(|value| match value.as_str() {
                Some(val_str) => {
                    let val_str = val_str.trim();

                    let val = match val_str.starts_with(':') {
                        true => format!("127.0.0.1{val_str}"),
                        false => val_str.to_owned(),
                    };

                    SocketAddr::from_str(val.as_str())
                        .ok()
                        .map(|s| s.to_string())
                }
                None => None,
            })
            .unwrap_or("127.0.0.1:9090".into())
    }

    #[instrument]
    pub fn prepare_external_controller_port(&mut self) -> Result<()> {
        let strategy = Config::verge()
            .latest()
            .get_external_controller_port_strategy();

        let server = self.get_client_info().server;
        let (server_ip, server_port) = server.split_once(':').unwrap_or(("127.0.0.1", "9090"));
        let server_port = server_port.parse::<u16>().unwrap_or(9090);
        let port = get_clash_external_port(&strategy, server_port)?;

        if port != server_port {
            let new_server = format!("{server_ip}:{port}");
            log::warn!("The external controller port has been changed to {new_server}");
            let mut map = Mapping::new();
            map.insert("external-controller".into(), new_server.into());
            todo!()
            // self.patch_config(map);
        }
        Ok(())
    }

    pub fn get_client_info(&self) -> ClashInfo {
        let config = &self.0;

        ClashInfo {
            port: Self::guard_mixed_port(config),
            server: Self::guard_client_ctrl(config),
            secret: config.get("secret").and_then(|value| match value {
                Value::String(val_str) => Some(val_str.clone()),
                Value::Bool(val_bool) => Some(val_bool.to_string()),
                Value::Number(val_num) => Some(val_num.to_string()),
                _ => None,
            }),
        }
    }

    pub fn guard_client_ctrl(config: &Mapping) -> String {
        let value = Self::guard_server_ctrl(config);
        match SocketAddr::from_str(value.as_str()) {
            Ok(mut socket) => {
                if socket.ip().is_unspecified() {
                    socket.set_ip(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
                }
                socket.to_string()
            }
            Err(_) => "127.0.0.1:9090".into(),
        }
    }

    pub fn get_mixed_port(&self) -> u16 {
        Self::guard_mixed_port(&self.0)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Eq, specta::Type)]
pub struct ClashInfo {
    /// clash core port
    pub port: u16,
    /// same as `external-controller`
    pub server: String,
    /// clash secret
    pub secret: Option<String>,
}
