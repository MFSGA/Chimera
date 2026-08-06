use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};

#[derive(Default, Debug, Clone)]
pub struct IRuntime {
    pub config: Option<Mapping>,
}

impl IRuntime {
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies only persistent runtime override fields to the generated config.
    pub fn patch_config(&mut self, overrides: &ClashConfigOverrides) {
        tracing::debug!(
            allow_lan = ?overrides.allow_lan,
            ipv6 = ?overrides.ipv6,
            log_level = ?overrides.log_level,
            mode = ?overrides.mode,
            "patch generated runtime config overrides"
        );

        if let Some(config) = self.config.as_mut() {
            overrides.apply_to(config);
        }
    }
}

/// Persistent user overrides that are merged into the generated Clash config.
///
/// This is intentionally separate from `ClashRuntimeConfig`, which represents
/// the running core's `GET /configs` response.
#[derive(Default, Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ClashConfigOverrides {
    #[serde(default, rename = "allow-lan", skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(default, rename = "log-level", skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl ClashConfigOverrides {
    pub fn from_mapping(patch: &Mapping) -> Result<Self> {
        serde_yaml::from_value(Value::Mapping(patch.clone()))
            .context("invalid Clash config overrides")
    }

    pub fn to_mapping(&self) -> Mapping {
        let mut mapping = Mapping::new();
        self.apply_to(&mut mapping);
        mapping
    }

    fn apply_to(&self, mapping: &mut Mapping) {
        if let Some(value) = self.allow_lan {
            mapping.insert("allow-lan".into(), Value::Bool(value));
        }
        if let Some(value) = self.ipv6 {
            mapping.insert("ipv6".into(), Value::Bool(value));
        }
        if let Some(value) = &self.log_level {
            mapping.insert("log-level".into(), Value::String(value.clone()));
        }
        if let Some(value) = &self.mode {
            mapping.insert("mode".into(), Value::String(value.clone()));
        }
    }
}

/// Typed IPC payload for modifying persistent runtime overrides.
#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub struct PatchRuntimeConfig {
    #[serde(default, rename = "allow-lan", skip_serializing_if = "Option::is_none")]
    pub allow_lan: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(default, rename = "log-level", skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

impl From<PatchRuntimeConfig> for ClashConfigOverrides {
    fn from(payload: PatchRuntimeConfig) -> Self {
        Self {
            allow_lan: payload.allow_lan,
            ipv6: payload.ipv6,
            log_level: payload.log_level,
            mode: payload.mode,
        }
    }
}

#[derive(Default, Debug, Clone, Deserialize, Serialize, specta::Type)]
pub struct PatchClashCoreConfig {
    #[serde(
        rename = "mixed-port",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub mixed_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
    #[serde(
        rename = "external-controller",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub external_controller: Option<String>,
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
