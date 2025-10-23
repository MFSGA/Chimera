use std::{net::SocketAddr, str::FromStr};

use serde_yaml::{Mapping, Value};

use crate::utils::{dirs, help};

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
}
