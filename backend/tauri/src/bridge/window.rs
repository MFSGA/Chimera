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
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl LegacyWindowBridge {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

struct PreparedWindowMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    projected: IVerge,
}

impl PreparedLegacyMirror for PreparedWindowMirror {
    #[allow(deprecated)]
    fn apply(self: Box<Self>) {
        let _guard = self.legacy_lock.lock();
        Config::verge().apply_update(|target| {
            target.window_size_state = self.projected.window_size_state.clone();
            target.window_size_position = self.projected.window_size_position.clone();
        });
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

#[allow(deprecated)]
pub(crate) fn persistent_state_from_legacy(legacy: &IVerge) -> anyhow::Result<PersistentState> {
    let state = if let Some(window_state) = legacy.window_size_state.as_ref() {
        super::yaml_convert::<_, WindowState>(window_state)?
    } else if let Some(position) = legacy.window_size_position.as_ref() {
        window_state_from_position(position)
    } else {
        return Ok(PersistentState::default());
    };

    Ok(PersistentState {
        window_state: BTreeMap::from([(WindowLabel(MAIN_WINDOW_LABEL.into()), state)]),
    })
}

fn window_state_from_position(position: &[f64]) -> WindowState {
    WindowState {
        width: position.first().copied().unwrap_or_default().max(0.0) as u32,
        height: position.get(1).copied().unwrap_or_default().max(0.0) as u32,
        x: position.get(2).copied().unwrap_or_default() as i32,
        y: position.get(3).copied().unwrap_or_default() as i32,
        maximized: false,
        fullscreen: false,
    }
}

#[allow(deprecated)]
pub(crate) fn apply_session_state_to_legacy_verge(
    draft: &mut IVerge,
    snap: &PersistentState,
) -> anyhow::Result<()> {
    draft.window_size_state = snap
        .window_state
        .get(&WindowLabel(MAIN_WINDOW_LABEL.into()))
        .map(super::yaml_convert::<_, LegacyWindowState>)
        .transpose()?;
    draft.window_size_position = draft.window_size_state.as_ref().map(|state| {
        vec![
            state.width as f64,
            state.height as f64,
            state.x as f64,
            state.y as f64,
        ]
    });
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
        #[allow(deprecated)]
        let position = projected
            .window_size_position
            .expect("legacy position should also be projected");
        assert_eq!(position, vec![900.0, 700.0, 20.0, 30.0]);
    }

    #[test]
    #[allow(deprecated)]
    fn legacy_window_position_seeds_typed_session_state() {
        let legacy = IVerge {
            window_size_position: Some(vec![1024.0, 768.0, 40.0, 50.0]),
            ..IVerge::default()
        };

        let typed = persistent_state_from_legacy(&legacy).expect("legacy position should map");
        let state = typed
            .window_state
            .get(&WindowLabel(MAIN_WINDOW_LABEL.into()))
            .expect("typed main window state should exist");

        assert_eq!(state.width, 1024);
        assert_eq!(state.height, 768);
        assert_eq!(state.x, 40);
        assert_eq!(state.y, 50);
        assert!(!state.maximized);
        assert!(!state.fullscreen);
    }
}
