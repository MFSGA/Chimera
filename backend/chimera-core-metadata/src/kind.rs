use constcat::concat;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Type, JsonSchema, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CoreKind {
    Clash(ClashCoreKind),
    SingBox,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Type, JsonSchema, Serialize, Deserialize)]
#[repr(u8)]
/// Supported Clash core kinds. This is used to gate the launch arguments and api favors.
///
/// For example:
/// for modern core, such as Mihomo, we prefer the unix socket or named pipe for ipc,
/// while for legacy core, such as Clash Premium, we prefer the http api.
pub enum ClashCoreKind {
    #[serde(rename = "mihomo")]
    Mihomo,
    #[serde(rename = "clash-rs")]
    ClashRust,
    #[serde(rename = "clash")]
    ClashPremium,
    #[serde(rename = "meow")]
    Meow,
    /// Chimera's separately distributed Clash-rs-compatible core. Keep it
    /// distinct from upstream Clash-rs in serialized identity and diagnostics.
    #[serde(rename = "chimera-client", alias = "chimera", alias = "chimera_client")]
    ChimeraClient,
}

impl AsRef<str> for ClashCoreKind {
    fn as_ref(&self) -> &str {
        match self {
            ClashCoreKind::Mihomo => "mihomo",
            ClashCoreKind::ClashRust => "clash-rs",
            ClashCoreKind::ClashPremium => "clash",
            ClashCoreKind::Meow => "meow",
            ClashCoreKind::ChimeraClient => "chimera-client",
        }
    }
}

impl std::fmt::Display for ClashCoreKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_ref())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Type, Serialize, Deserialize)]
#[repr(u8)]
/// The resource variant of a Clash core. This is used to determine which resource to download for a given core kind.
pub enum ClashCoreResourceVariant {
    #[serde(rename = "mihomo")]
    Mihomo,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs")]
    ClashRust,
    #[serde(rename = "clash-rs-alpha")]
    ClashRustAlpha,
    #[serde(rename = "clash")]
    ClashPremium,
    #[serde(rename = "meow")]
    Meow,
}

impl ClashCoreResourceVariant {
    #[inline]
    pub fn binary_name(&self) -> &'static str {
        use std::env::consts::*;
        match self {
            ClashCoreResourceVariant::Mihomo => concat!("mihomo", EXE_SUFFIX),
            ClashCoreResourceVariant::MihomoAlpha => {
                concat!("mihomo-alpha", EXE_SUFFIX)
            }
            ClashCoreResourceVariant::ClashRust => {
                concat!("clash-rs", EXE_SUFFIX)
            }
            ClashCoreResourceVariant::ClashRustAlpha => {
                concat!("clash-rs-alpha", EXE_SUFFIX)
            }
            ClashCoreResourceVariant::ClashPremium => {
                concat!("clash", EXE_SUFFIX)
            }
            ClashCoreResourceVariant::Meow => concat!("meow", EXE_SUFFIX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClashCoreKind;

    #[test]
    fn chimera_client_keeps_a_canonical_brand_wire_name_and_reads_legacy_names() {
        // The current product identifier is the canonical wire value; stored
        // legacy aliases remain readable and normalize on serialization.
        let kind = ClashCoreKind::ChimeraClient;
        assert_eq!(kind.as_ref(), "chimera-client");
        assert_eq!(kind.to_string(), "chimera-client");
        assert_eq!(serde_json::to_string(&kind).unwrap(), "\"chimera-client\"");

        for legacy_name in ["chimera", "chimera_client"] {
            let encoded = format!("\"{legacy_name}\"");
            let decoded: ClashCoreKind = serde_json::from_str(&encoded).unwrap();
            assert_eq!(decoded, ClashCoreKind::ChimeraClient);
        }
    }
}
