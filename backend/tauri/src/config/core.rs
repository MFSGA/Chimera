use anyhow::Result;
use once_cell::sync::OnceCell;

use crate::{
    config::{
        chimera::IVerge, clash::IClashTemp, draft::Draft, profile::profiles::Profiles,
        runtime::IRuntime,
    },
    core::state::ManagedState,
    enhance,
};

/// whole config
pub struct Config {
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
            profiles_config: ManagedState::from(Profiles::new()),
            verge_config: Draft::from(IVerge::new()),
            clash_config: Draft::from(IClashTemp::new()),
            runtime_config: Draft::from(IRuntime::new()),
        })
    }

    pub fn profiles() -> &'static ManagedState<Profiles> {
        &Self::global().profiles_config
    }

    pub fn verge() -> Draft<IVerge> {
        Self::global().verge_config.clone()
    }

    /// 生成配置存好
    pub async fn generate() -> Result<()> {
        let (config, exists_keys, postprocessing_outputs) = enhance::enhance().await;

        *Config::runtime().draft() = IRuntime {};

        Ok(())
    }

    pub fn clash() -> Draft<IClashTemp> {
        Self::global().clash_config.clone()
    }

    pub fn runtime() -> Draft<IRuntime> {
        Self::global().runtime_config.clone()
    }
}
