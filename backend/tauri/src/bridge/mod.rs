pub mod clash;
pub mod verge;
pub mod window;

use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, ClashConfigPatch},
    state::{PersistentState, PersistentStatePatch},
};
use serde::{Serialize, de::DeserializeOwned};
use struct_patch::Patch;

use crate::config::{chimera::IVerge, clash::IClashTemp};

pub(crate) fn typed_config_from_legacy_parts(
    legacy: &IVerge,
    legacy_clash: &serde_yaml::Mapping,
) -> anyhow::Result<(ChimeraAppConfig, PersistentState, ClashConfig)> {
    Ok((
        verge::application_from_legacy(legacy)?,
        window::persistent_state_from_legacy(legacy)?,
        clash::clash_config_from_legacy(legacy, legacy_clash)?,
    ))
}

pub(crate) struct LegacyVergePatchPlan {
    pub application: Option<IVerge>,
    pub session_state: Option<PersistentStatePatch>,
    pub clash_config: Option<ClashConfigPatch>,
}

pub(crate) fn split_legacy_verge_patch(
    base: &IVerge,
    patch: &IVerge,
    legacy_clash: &IClashTemp,
) -> anyhow::Result<LegacyVergePatchPlan> {
    let mut projected = base.clone();
    projected.patch_config(patch.clone());
    let next_session = window::persistent_state_from_legacy(&projected)?;
    let next_clash = clash::clash_config_from_legacy(&projected, &legacy_clash.0)?;

    Ok(LegacyVergePatchPlan {
        application: application_patch_from_legacy_patch(patch)?,
        session_state: session_patch_from_legacy_patch(patch, next_session),
        clash_config: clash_patch_from_legacy_patch(patch, next_clash),
    })
}

fn application_patch_from_legacy_patch(patch: &IVerge) -> anyhow::Result<Option<IVerge>> {
    let mut application = patch.clone();

    application.enable_tun_mode = None;
    application.web_ui_list = None;
    application.enable_clash_fields = None;
    application.enable_random_port = None;
    application.verge_mixed_port = None;
    application.tun_stack = None;
    application.clash_strategy = None;
    application.break_when_proxy_change = None;
    application.break_when_profile_change = None;
    application.break_when_mode_change = None;
    application.window_size_state = None;

    let touched = serde_yaml::to_value(&application)?
        .as_mapping()
        .is_some_and(|mapping| mapping.values().any(|value| !value.is_null()));
    Ok(touched.then_some(application))
}

fn session_patch_from_legacy_patch(
    patch: &IVerge,
    next: PersistentState,
) -> Option<PersistentStatePatch> {
    patch.window_size_state.as_ref()?;
    let mut session = PersistentState::new_empty_patch();
    session.window_state = Some(next.window_state);
    Some(session)
}

fn clash_patch_from_legacy_patch(patch: &IVerge, next: ClashConfig) -> Option<ClashConfigPatch> {
    let mut clash = ClashConfig::new_empty_patch();
    let mut touched = false;

    if patch.enable_tun_mode.is_some() {
        clash.enable_tun_mode = Some(next.enable_tun_mode);
        touched = true;
    }
    if patch.web_ui_list.is_some() {
        clash.web_ui_list = Some(next.web_ui_list);
        touched = true;
    }
    if patch.enable_clash_fields.is_some() {
        clash.enable_clash_fields = Some(next.enable_clash_fields);
        touched = true;
    }
    if patch.tun_stack.is_some() {
        clash.tun_stack = Some(next.tun_stack);
        touched = true;
    }
    if patch.enable_random_port.is_some() || patch.verge_mixed_port.is_some() {
        clash.mixed_port = Some(next.mixed_port);
        touched = true;
    }
    if patch.clash_strategy.is_some() {
        clash.external_controller = Some(next.external_controller);
        touched = true;
    }
    if patch.break_when_proxy_change.is_some()
        || patch.break_when_profile_change.is_some()
        || patch.break_when_mode_change.is_some()
    {
        clash.break_connection = Some(next.break_connection);
        touched = true;
    }

    touched.then_some(clash)
}

pub(super) fn yaml_convert<T, U>(value: T) -> anyhow::Result<U>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_yaml::to_value(value)?;
    Ok(serde_yaml::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_verge_patch_splits_application_and_clash_domains() {
        let base = IVerge::template();
        let patch = IVerge {
            theme_mode: Some("dark".into()),
            enable_tun_mode: Some(true),
            enable_random_port: Some(true),
            verge_mixed_port: Some(17890),
            break_when_mode_change: Some(true),
            ..IVerge::default()
        };

        let plan = split_legacy_verge_patch(&base, &patch, &IClashTemp::template())
            .expect("legacy patch should split");

        let application = plan.application.expect("application patch should exist");
        assert_eq!(application.theme_mode.as_deref(), Some("dark"));
        assert_eq!(application.enable_tun_mode, None);
        assert_eq!(application.enable_random_port, None);
        assert_eq!(application.verge_mixed_port, None);
        assert_eq!(application.break_when_mode_change, None);

        let clash = plan.clash_config.expect("clash patch should exist");
        assert_eq!(clash.enable_tun_mode, Some(true));
        assert_eq!(
            clash.mixed_port.as_ref().map(|port| port.start_port),
            Some(17890)
        );
        assert_eq!(
            clash.mixed_port.as_ref().map(|port| &port.kind),
            Some(&chimera_config::clash::config::clash_strategy::PortStrategyKind::Random)
        );
        assert_eq!(
            clash
                .break_connection
                .as_ref()
                .map(|strategy| strategy.on_mode_change),
            Some(true)
        );
    }

    #[test]
    fn session_only_patch_does_not_create_application_or_clash_patch() {
        let patch = IVerge {
            window_size_state: Some(crate::config::chimera::WindowState {
                width: 960,
                height: 720,
                x: 12,
                y: 24,
                maximized: false,
                fullscreen: false,
            }),
            ..IVerge::default()
        };

        let plan = split_legacy_verge_patch(&IVerge::template(), &patch, &IClashTemp::template())
            .expect("session patch should split");

        assert!(plan.application.is_none());
        assert!(plan.clash_config.is_none());
        let session = plan.session_state.expect("session patch should exist");
        let window_state = session.window_state.expect("window patch should exist");
        let state = window_state
            .get(&chimera_config::state::window::WindowLabel("main".into()))
            .expect("main window state should exist");
        assert_eq!(state.width, 960);
        assert_eq!(state.height, 720);
    }

    #[test]
    fn application_only_patch_does_not_create_clash_patch() {
        let patch = IVerge {
            theme_mode: Some("light".into()),
            enable_auto_launch: Some(true),
            ..IVerge::default()
        };

        let plan = split_legacy_verge_patch(&IVerge::template(), &patch, &IClashTemp::template())
            .expect("application patch should split");

        assert!(plan.clash_config.is_none());
        let application = plan.application.expect("application patch should exist");
        assert_eq!(application.theme_mode.as_deref(), Some("light"));
        assert_eq!(application.enable_auto_launch, Some(true));
    }
}
