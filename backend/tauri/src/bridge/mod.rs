pub mod clash;

use chimera_config::clash::config::{ClashConfig, ClashConfigPatch};
use serde::{Serialize, de::DeserializeOwned};
use struct_patch::Patch;

use crate::config::{chimera::IVerge, clash::IClashTemp};

pub(crate) struct LegacyVergePatchPlan {
    pub application: IVerge,
    pub clash_config: Option<ClashConfigPatch>,
}

pub(crate) fn split_legacy_verge_patch(
    base: &IVerge,
    patch: &IVerge,
    legacy_clash: &IClashTemp,
) -> anyhow::Result<LegacyVergePatchPlan> {
    let mut projected = base.clone();
    projected.patch_config(patch.clone());
    let next_clash = clash::clash_config_from_legacy(&projected, &legacy_clash.0)?;

    Ok(LegacyVergePatchPlan {
        application: application_patch_from_legacy_patch(patch),
        clash_config: clash_patch_from_legacy_patch(patch, next_clash),
    })
}

fn application_patch_from_legacy_patch(patch: &IVerge) -> IVerge {
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

    application
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

        assert_eq!(plan.application.theme_mode.as_deref(), Some("dark"));
        assert_eq!(plan.application.enable_tun_mode, None);
        assert_eq!(plan.application.enable_random_port, None);
        assert_eq!(plan.application.verge_mixed_port, None);
        assert_eq!(plan.application.break_when_mode_change, None);

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
    fn application_only_patch_does_not_create_clash_patch() {
        let patch = IVerge {
            theme_mode: Some("light".into()),
            enable_auto_launch: Some(true),
            ..IVerge::default()
        };

        let plan = split_legacy_verge_patch(&IVerge::template(), &patch, &IClashTemp::template())
            .expect("application patch should split");

        assert!(plan.clash_config.is_none());
        assert_eq!(plan.application.theme_mode.as_deref(), Some("light"));
        assert_eq!(plan.application.enable_auto_launch, Some(true));
    }
}
