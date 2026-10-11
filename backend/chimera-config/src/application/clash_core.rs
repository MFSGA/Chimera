use enumflags2::bitflags;
use serde::{Deserialize, Serialize};
use specta::Type;
use strum::{AsRefStr, Display, EnumString, IntoStaticStr};

#[bitflags]
#[repr(u8)]
#[derive(
    Debug,
    Clone,
    Copy,
    Deserialize,
    Serialize,
    PartialEq,
    Eq,
    Type,
    Display,
    AsRefStr,
    EnumString,
    IntoStaticStr,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ClashCore {
    #[serde(rename = "clash", alias = "clash-premium")]
    ClashPremium = 0b0001,
    #[serde(rename = "clash-rs")]
    ClashRs,
    #[serde(rename = "mihomo", alias = "clash-meta")]
    Mihomo,
    #[serde(rename = "chimera-client", alias = "chimera", alias = "chimera_client")]
    ChimeraClient,
    #[serde(rename = "mihomo-alpha")]
    MihomoAlpha,
    #[serde(rename = "clash-rs-alpha")]
    ClashRsAlpha,
    #[serde(rename = "chimera-client-alpha")]
    ChimeraClientAlpha,
}

impl Default for ClashCore {
    fn default() -> Self {
        match cfg!(feature = "default-meta") {
            false => Self::ClashPremium,
            true => Self::Mihomo,
        }
    }
}

impl From<&ClashCore> for chimera_utils::core::CoreType {
    fn from(core: &ClashCore) -> Self {
        use chimera_utils::core::{ClashCoreType, CoreType};
        CoreType::Clash(match core {
            ClashCore::ClashPremium => ClashCoreType::ClashPremium,
            ClashCore::ClashRs => ClashCoreType::ClashRust,
            ClashCore::Mihomo => ClashCoreType::Mihomo,
            ClashCore::ChimeraClient => ClashCoreType::ChimeraClient,
            // Alpha is a separate distributed binary but keeps the same
            // Chimera Client behavior and config contract.
            ClashCore::ChimeraClientAlpha => ClashCoreType::ChimeraClient,
            ClashCore::MihomoAlpha => ClashCoreType::MihomoAlpha,
            ClashCore::ClashRsAlpha => ClashCoreType::ClashRustAlpha,
        })
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported core type: {0}")]
pub struct UnsupportedCoreTypeError(pub chimera_utils::core::CoreType);

impl TryFrom<&chimera_utils::core::CoreType> for ClashCore {
    type Error = UnsupportedCoreTypeError;

    fn try_from(core: &chimera_utils::core::CoreType) -> Result<Self, Self::Error> {
        use chimera_utils::core::{ClashCoreType, CoreType};
        match core {
            CoreType::Clash(clash) => match clash {
                ClashCoreType::ClashPremium => Ok(Self::ClashPremium),
                ClashCoreType::ClashRust => Ok(Self::ClashRs),
                ClashCoreType::ClashRustAlpha => Ok(Self::ClashRsAlpha),
                ClashCoreType::Mihomo => Ok(Self::Mihomo),
                ClashCoreType::MihomoAlpha => Ok(Self::MihomoAlpha),
                ClashCoreType::ChimeraClient => Ok(Self::ChimeraClient),
            },
            _ => Err(UnsupportedCoreTypeError(core.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClashCore;
    use chimera_utils::core::{ClashCoreType, CoreType};

    #[test]
    fn chimera_client_alpha_keeps_its_channel_in_the_application_contract() {
        let core = ClashCore::ChimeraClientAlpha;

        assert_eq!(
            serde_json::to_string(&core).unwrap(),
            "\"chimera-client-alpha\""
        );
        assert_eq!(
            serde_json::from_str::<ClashCore>("\"chimera-client-alpha\"").unwrap(),
            core
        );
        assert_eq!(
            CoreType::from(&core),
            CoreType::Clash(ClashCoreType::ChimeraClient)
        );
    }
}
