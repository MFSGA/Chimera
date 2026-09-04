pub mod clash;
pub mod verge;
pub mod window;

use chimera_config::{
    application::{ChimeraAppConfig, ChimeraAppConfigPatch},
    clash::config::{ClashConfig, ClashConfigPatch},
    state::{PersistentState, PersistentStatePatch},
};
use serde::{Serialize, de::DeserializeOwned};
use struct_patch::Patch;

use crate::{
    config::{chimera::IVerge, clash::IClashTemp},
    state::TypedConfigPatchPlan,
};

pub(crate) fn typed_patches_from_legacy_patch(
    mut base: IVerge,
    patch: &IVerge,
    legacy_clash: &IClashTemp,
) -> anyhow::Result<TypedConfigPatchPlan> {
    base.patch_config(patch.clone());
    let next_application = verge::application_from_legacy(&base)?;
    let next_session = window::persistent_state_from_legacy(&base)?;
    let next_clash = clash::clash_config_from_legacy(&base, &legacy_clash.0)?;

    Ok(TypedConfigPatchPlan {
        application: application_patch_from_legacy_patch(patch, next_application),
        session_state: session_patch_from_legacy_patch(patch, next_session),
        clash_config: clash_patch_from_legacy_patch(patch, next_clash),
    })
}

fn application_patch_from_legacy_patch(
    patch: &IVerge,
    next: ChimeraAppConfig,
) -> Option<ChimeraAppConfigPatch> {
    let mut application = ChimeraAppConfig::new_empty_patch();
    let mut touched = false;

    macro_rules! set_if_some {
        ($legacy:ident, $target:ident) => {
            if patch.$legacy.is_some() {
                application.$target = Some(next.$target);
                touched = true;
            }
        };
    }

    set_if_some!(app_log_level, app_log_level);
    set_if_some!(language, language);
    set_if_some!(theme_mode, theme_mode);
    set_if_some!(lighten_animation_effects, lighten_animation_effects);
    set_if_some!(enable_service_mode, enable_service_mode);
    set_if_some!(enable_auto_launch, enable_auto_launch);
    set_if_some!(enable_silent_start, enable_silent_start);
    set_if_some!(enable_system_proxy, enable_system_proxy);
    set_if_some!(enable_proxy_guard, enable_proxy_guard);
    set_if_some!(system_proxy_bypass, system_proxy_bypass);
    set_if_some!(proxy_guard_interval, proxy_guard_interval);
    set_if_some!(theme_color, theme_color);
    set_if_some!(enable_builtin_enhanced, enable_builtin_enhanced);
    set_if_some!(max_log_files, max_log_files);
    set_if_some!(enable_auto_check_update, enable_auto_check_update);
    set_if_some!(always_on_top, always_on_top);

    if patch.clash_core.is_some() {
        application.core = Some(next.core);
        touched = true;
    }
    if patch.clash_tray_selector.is_some() {
        application.tray_selector_mode = Some(next.tray_selector_mode);
        touched = true;
    }
    if patch.window_type.is_some() {
        application.window_type = Some(next.window_type);
        touched = true;
    }

    touched.then_some(application)
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

        let plan = typed_patches_from_legacy_patch(base, &patch, &IClashTemp::template())
            .expect("legacy patch should split");

        let application = plan.application.expect("application patch should exist");
        assert_eq!(
            application.theme_mode,
            Some(chimera_config::application::ThemeMode::Dark)
        );

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

        let plan =
            typed_patches_from_legacy_patch(IVerge::template(), &patch, &IClashTemp::template())
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

        let plan =
            typed_patches_from_legacy_patch(IVerge::template(), &patch, &IClashTemp::template())
                .expect("application patch should split");

        assert!(plan.clash_config.is_none());
        let application = plan.application.expect("application patch should exist");
        assert_eq!(
            application.theme_mode,
            Some(chimera_config::application::ThemeMode::Light)
        );
        assert_eq!(application.enable_auto_launch, Some(true));
    }
}
