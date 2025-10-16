use once_cell::sync::OnceCell;

use crate::{config::profile::profiles::Profiles, core::state::ManagedState};

pub struct Config {
    profiles_config: ManagedState<Profiles>,
}

impl Config {
    pub fn global() -> &'static Config {
        static CONFIG: OnceCell<Config> = OnceCell::new();

        CONFIG.get_or_init(|| Config {
            profiles_config: ManagedState::from(Profiles::new()),
        })
    }

    pub fn profiles() -> &'static ManagedState<Profiles> {
        &Self::global().profiles_config
    }
}
