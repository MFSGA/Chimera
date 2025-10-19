use serde_yaml::{Mapping, Value};

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
}
