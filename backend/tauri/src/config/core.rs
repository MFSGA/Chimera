use serde_yaml::Mapping;

use anyhow::Result;
use chimera_config::clash::config::ClashConfig;
use once_cell::sync::OnceCell;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp, draft::Draft, runtime::IRuntime},
    enhance::{self, PostProcessingOutput},
};

#[cfg(test)]
use crate::{config::profile::profiles::Profiles, core::state::ManagedState};

pub(crate) struct RuntimeInputOutput {
    pub(crate) config: Mapping,
    pub(crate) exists_keys: Vec<String>,
    pub(crate) postprocessing_output: PostProcessingOutput,
    pub(crate) inspection: crate::client::runtime_inspection::RuntimeInspectionData,
}

/// whole config
pub struct Config {
    #[cfg(test)]
    profiles_config: ManagedState<Profiles>,
    verge_config: Draft<IVerge>,
    /// 3
    clash_config: Draft<IClashTemp>,
    /// 4
    runtime_config: Draft<IRuntime>,
}

impl Config {
    pub fn global() -> &'static Config {
        static CONFIG: OnceCell<Config> = OnceCell::new();

        CONFIG.get_or_init(|| Config {
            #[cfg(test)]
            profiles_config: ManagedState::from(Profiles::new()),
            verge_config: Draft::from(IVerge::new()),
            clash_config: Draft::from(IClashTemp::new()),
            runtime_config: Draft::from(IRuntime::new()),
        })
    }

    #[cfg(test)]
    pub fn profiles() -> &'static ManagedState<Profiles> {
        &Self::global().profiles_config
    }

    pub fn verge() -> Draft<IVerge> {
        Self::global().verge_config.clone()
    }

    /// Generate the runtime mapping and transform output from one typed Clash snapshot.
    pub async fn generate_runtime_input_with(
        clash: &ClashConfig,
        core: crate::config::chimera::ClashCore,
    ) -> Result<(Mapping, PostProcessingOutput)> {
        Self::generate_runtime_output_with(clash, core)
            .await
            .map(|output| (output.config, output.postprocessing_output))
    }

    pub(crate) async fn generate_runtime_output_with(
        clash: &ClashConfig,
        core: crate::config::chimera::ClashCore,
    ) -> Result<RuntimeInputOutput> {
        let client_info = Config::clash().latest().get_client_info();
        let resolved_ports = chimera_config::runtime::executor::ResolvedPortBindings {
            mixed_port: client_info.port,
            external_controller: Some(client_info.server),
            ..Default::default()
        };
        Self::generate_runtime_output_with_ports(clash, core, resolved_ports).await
    }

    pub(crate) async fn generate_runtime_output_with_ports(
        clash: &ClashConfig,
        core: crate::config::chimera::ClashCore,
        resolved_ports: chimera_config::runtime::executor::ResolvedPortBindings,
    ) -> Result<RuntimeInputOutput> {
        let _ = (clash, core, resolved_ports);
        anyhow::bail!("a typed Profile snapshot is required to build runtime configuration")
    }

    /// Generate runtime output from the Profile domain snapshot. The normal
    /// runtime path and Profile transaction candidates use the same builder;
    /// only the staged content overlay differs during a file-first mutation.
    pub(crate) async fn generate_runtime_output_from_profiles(
        clash: &ClashConfig,
        core: crate::config::chimera::ClashCore,
        app: chimera_config::application::ChimeraAppConfig,
        profiles: std::sync::Arc<chimera_config::profile::Profiles>,
        resolved_ports: chimera_config::runtime::executor::ResolvedPortBindings,
        staged_content: std::collections::BTreeMap<String, String>,
        strict_transforms: bool,
    ) -> Result<RuntimeInputOutput> {
        let (config, exists_keys, postprocessing_output, inspection) =
            enhance::build_from_profiles_with_inspection(
                clash,
                core,
                profiles,
                app,
                resolved_ports,
                staged_content,
                strict_transforms,
            )
            .await?;

        *Config::runtime().draft() = IRuntime {
            config: Some(config.clone()),
        };

        Ok(RuntimeInputOutput {
            config,
            exists_keys,
            postprocessing_output,
            inspection,
        })
    }

    /// Legacy compatibility entry point for callers that do not own typed config snapshots yet.
    pub async fn generate_runtime_input() -> Result<(Mapping, PostProcessingOutput)> {
        let verge = Self::verge().latest().clone();
        let core = verge.clash_core.unwrap_or_default();
        let clash =
            crate::bridge::clash::clash_config_from_legacy(&verge, &Self::clash().latest().0)?;
        Self::generate_runtime_input_with(&clash, core).await
    }

    /// Generate the runtime mapping once and retain the exact draft used by the product pipeline.
    pub async fn generate_runtime_mapping() -> Result<Mapping> {
        Self::generate_runtime_input()
            .await
            .map(|(config, _postprocessing_output)| config)
    }

    /// Legacy compatibility entry point for callers that only need the generated draft.
    pub async fn generate() -> Result<()> {
        Self::generate_runtime_mapping().await.map(drop)
    }

    pub fn clash() -> Draft<IClashTemp> {
        Self::global().clash_config.clone()
    }

    pub fn runtime() -> Draft<IRuntime> {
        Self::global().runtime_config.clone()
    }

    /// Serialize the generated mapping exactly once for candidate/check/promote workflows.
    pub fn render_runtime_bytes(config: &Mapping) -> Result<Vec<u8>> {
        let yaml = serde_yaml::to_string(config)?;
        Ok(format!("# Generated by Clash Chimera\n\n{yaml}").into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use serde_yaml::{Mapping, Value};

    use super::Config;

    #[test]
    fn runtime_render_is_deterministic_and_has_the_product_header() {
        let mut config = Mapping::new();
        config.insert(Value::String("mode".into()), Value::String("rule".into()));

        let first = Config::render_runtime_bytes(&config).unwrap();
        let second = Config::render_runtime_bytes(&config).unwrap();

        assert_eq!(first, second);
        assert!(first.starts_with(b"# Generated by Clash Chimera\n\n"));
        let body = std::str::from_utf8(&first).unwrap();
        assert!(body.contains("mode: rule"));
    }
}
