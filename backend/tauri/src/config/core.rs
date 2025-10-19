use once_cell::sync::OnceCell;

use crate::{
    config::{chimera::IVerge, draft::Draft, profile::profiles::Profiles},
    core::state::ManagedState,
};

pub struct Config {
    profiles_config: ManagedState<Profiles>,
    verge_config: Draft<IVerge>,
}

impl Config {
    pub fn global() -> &'static Config {
        static CONFIG: OnceCell<Config> = OnceCell::new();

        CONFIG.get_or_init(|| Config {
            profiles_config: ManagedState::from(Profiles::new()),
            verge_config: Draft::from(IVerge::new()),
        })
    }

    pub fn profiles() -> &'static ManagedState<Profiles> {
        &Self::global().profiles_config
    }

    pub fn verge() -> Draft<IVerge> {
        Self::global().verge_config.clone()
    }
}
