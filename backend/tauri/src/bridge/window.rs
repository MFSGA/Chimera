use std::{collections::BTreeMap, sync::Arc};

use chimera_config::state::{
    PersistentState,
    window::{WindowLabel, WindowState},
};

use crate::{
    config::{
        chimera::{IVerge, WindowState as LegacyWindowState},
        core::Config,
    },
    state::mirror::{PreparedLegacyMirror, WindowLegacyBridge},
};

const MAIN_WINDOW_LABEL: &str = "main";

pub(crate) struct LegacyWindowBridge {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for LegacyWindowBridge {
    fn default() -> Self {
        Self {
            legacy_lock: Arc::new(parking_lot::Mutex::new(())),
        }
    }
}

struct PreparedWindowMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedWindowMirror {
    fn apply(self: Box<Self>) {
        let _guard = self.legacy_lock.lock();
        let store = Config::verge();
        let mut current = store.data();
        current.window_size_state = self.projected.window_size_state;
    }
}

impl WindowLegacyBridge for LegacyWindowBridge {
    fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        let mut projected = {
            let _guard = self.legacy_lock.lock();
            Config::verge().data().clone()
        };
        apply_session_state_to_legacy_verge(&mut projected, snap)?;
        Ok(Box::new(PreparedWindowMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            projected,
        }))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
        let _guard = self.legacy_lock.lock();
        persistent_state_from_legacy(&Config::verge().data())
    }
}

pub(crate) fn persistent_state_from_legacy(legacy: &IVerge) -> anyhow::Result<PersistentState> {
    let Some(window_state) = legacy.window_size_state.as_ref() else {
        return Ok(PersistentState::default());
    };
    let state = super::yaml_convert::<_, WindowState>(window_state)?;

    Ok(PersistentState {
        window_state: BTreeMap::from([(WindowLabel(MAIN_WINDOW_LABEL.into()), state)]),
    })
}

pub(crate) fn apply_session_state_to_legacy_verge(
    draft: &mut IVerge,
    snap: &PersistentState,
) -> anyhow::Result<()> {
    draft.window_size_state = snap
        .window_state
        .get(&WindowLabel(MAIN_WINDOW_LABEL.into()))
        .map(super::yaml_convert::<_, LegacyWindowState>)
        .transpose()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_window_state_roundtrips_through_typed_session_state() {
        let legacy = IVerge {
            window_size_state: Some(LegacyWindowState {
                width: 900,
                height: 700,
                x: 20,
                y: 30,
                maximized: true,
                fullscreen: false,
            }),
            ..IVerge::default()
        };

        let typed = persistent_state_from_legacy(&legacy).expect("legacy window state should map");
        let mut projected = IVerge::default();
        apply_session_state_to_legacy_verge(&mut projected, &typed)
            .expect("typed window state should map back");

        let state = projected
            .window_size_state
            .expect("projected state should exist");
        assert_eq!(state.width, 900);
        assert_eq!(state.height, 700);
        assert_eq!(state.x, 20);
        assert_eq!(state.y, 30);
        assert!(state.maximized);
        assert!(!state.fullscreen);
    }
}
