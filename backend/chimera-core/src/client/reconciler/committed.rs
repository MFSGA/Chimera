//! Selection policy and connection interruption for the committed runtime.
use chimera_config::{
    clash::config::{ClashConfig, overrides::Mode},
    profile::{ProfileId, Profiles},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    mode: Mode,
    current: Option<ProfileId>,
}
impl Selection {
    pub fn of(clash: &ClashConfig, profiles: &Profiles) -> Self {
        Self {
            mode: clash.overrides.mode(),
            current: profiles.current.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionChange {
    Mode,
    Profile,
}

impl Selection {
    pub fn interruption(
        &self,
        previous: Option<&Self>,
        clash: &chimera_config::clash::config::ClashConfig,
    ) -> Option<SelectionChange> {
        self.interruption_with_policy(
            previous,
            clash.break_connection.on_mode_change,
            clash.break_connection.on_profile_change,
        )
    }

    pub fn interruption_with_policy(
        &self,
        previous: Option<&Self>,
        on_mode_change: bool,
        on_profile_change: bool,
    ) -> Option<SelectionChange> {
        let previous = previous?;
        if previous.mode != self.mode && on_mode_change {
            Some(SelectionChange::Mode)
        } else if previous.current != self.current && on_profile_change {
            Some(SelectionChange::Profile)
        } else {
            None
        }
    }
}
