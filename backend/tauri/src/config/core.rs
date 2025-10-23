use anyhow::Result;
use once_cell::sync::OnceCell;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp, draft::Draft, profile::profiles::Profiles},
    core::state::ManagedState,
    enhance,
};

/// whole config
pub struct Config {
    profiles_config: ManagedState<Profiles>,
    verge_config: Draft<IVerge>,
    /// 3
    clash_config: Draft<IClashTemp>,
}

impl Config {
    pub fn global() -> &'static Config {
        static CONFIG: OnceCell<Config> = OnceCell::new();

        CONFIG.get_or_init(|| Config {
            profiles_config: ManagedState::from(Profiles::new()),
            verge_config: Draft::from(IVerge::new()),
            clash_config: Draft::from(IClashTemp::new()),
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
        todo!()
        /*  *Config::runtime().draft() = IRuntime {
            config: Some(config),
            exists_keys,
            postprocessing_output: postprocessing_outputs,
        };

        Ok(()) */
    }

    pub fn clash() -> Draft<IClashTemp> {
        Self::global().clash_config.clone()
    }
}
