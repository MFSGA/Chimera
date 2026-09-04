use chimera_config::clash::config::{
    ClashConfig,
    clash_strategy::{
        BreakConnectionStrategy, PortStrategy, PortStrategyKind, ProxyChangeBreakMode,
    },
};
use serde_yaml::{Mapping, Value};
use std::{net::SocketAddr, sync::Arc};

use crate::{
    config::{
        chimera::{BreakWhenProxyChange, ClashStrategy, IVerge},
        clash::IClashTemp,
        core::Config,
    },
    state::mirror::{ClashLegacyBridge, PreparedLegacyMirror},
};

pub(crate) struct LegacyClashBridge {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
}

impl Default for LegacyClashBridge {
    fn default() -> Self {
        Self::new(Arc::new(parking_lot::Mutex::new(())))
    }
}

impl LegacyClashBridge {
    pub(crate) fn new(legacy_lock: Arc<parking_lot::Mutex<()>>) -> Self {
        Self { legacy_lock }
    }
}

struct PreparedClashMirror {
    legacy_lock: Arc<parking_lot::Mutex<()>>,
    clash_projected: IClashTemp,
    verge_projected: IVerge,
}

impl PreparedLegacyMirror for PreparedClashMirror {
    fn apply(self: Box<Self>) {
        let _guard = self.legacy_lock.lock();
        *Config::clash().data() = self.clash_projected;
        let verge_store = Config::verge();
        let mut verge = verge_store.data();
        apply_prepared_clash_verge_projection(&mut verge, &self.verge_projected);
    }
}

impl LegacyClashBridge {
    fn prepare_mirror(&self, snap: &ClashConfig) -> anyhow::Result<PreparedClashMirror> {
        let _guard = self.legacy_lock.lock();
        let mut clash_projected = Config::clash().data().clone();
        let mut verge_projected = IVerge::default();
        prepare_clash_overrides(&mut clash_projected, snap)?;
        apply_clash_config_to_legacy_verge(&mut verge_projected, snap)?;
        Ok(PreparedClashMirror {
            legacy_lock: Arc::clone(&self.legacy_lock),
            clash_projected,
            verge_projected,
        })
    }
}

impl ClashLegacyBridge for LegacyClashBridge {
    fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
        Ok(Box::new(self.prepare_mirror(snap)?))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
        let _guard = self.legacy_lock.lock();
        clash_config_from_legacy(&Config::verge().data(), &Config::clash().data().0)
    }
}

pub(crate) fn clash_config_from_legacy(
    legacy_verge: &IVerge,
    legacy_clash: &Mapping,
) -> anyhow::Result<ClashConfig> {
    let legacy_clash = normalize_legacy_clash_overrides(legacy_clash);
    let mut next = ClashConfig {
        overrides: super::yaml_convert(&legacy_clash)?,
        ..ClashConfig::default()
    };

    next.enable_tun_mode = legacy_verge.enable_tun_mode.unwrap_or(false);
    next.web_ui_list = legacy_verge.web_ui_list.clone().unwrap_or_default();
    next.enable_clash_fields = legacy_verge.enable_clash_fields.unwrap_or(true);
    if let Some(value) = legacy_verge.tun_stack {
        next.tun_stack = super::yaml_convert(value)?;
    }

    let mixed_port = legacy_verge
        .verge_mixed_port
        .unwrap_or_else(|| IClashTemp::guard_mixed_port(&legacy_clash));
    next.mixed_port = if legacy_verge.enable_random_port.unwrap_or(false) {
        PortStrategy {
            kind: PortStrategyKind::Random,
            start_port: mixed_port,
        }
    } else {
        PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: mixed_port,
        }
    };

    if let Some(controller) = external_controller_from_legacy_clash(&legacy_clash) {
        next.external_controller.host = controller.ip();
        next.external_controller.port.start_port = controller.port();
    }

    if let Some(strategy) = &legacy_verge.clash_strategy {
        next.external_controller.port.kind =
            super::yaml_convert(&strategy.external_controller_port_strategy)?;
    }

    next.break_connection = break_connection_from_legacy(legacy_verge);
    Ok(next)
}

fn normalize_legacy_clash_overrides(legacy_clash: &Mapping) -> Mapping {
    let mut merged = IClashTemp::template().0;
    for (key, value) in legacy_clash {
        if !matches!(value, Value::Null) {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

fn external_controller_from_legacy_clash(legacy_clash: &Mapping) -> Option<SocketAddr> {
    IClashTemp::guard_server_ctrl(legacy_clash).parse().ok()
}

fn prepare_clash_overrides(projected: &mut IClashTemp, snap: &ClashConfig) -> anyhow::Result<()> {
    let mut mapping: Mapping = super::yaml_convert(&snap.overrides)?;
    mapping.insert("mixed-port".into(), snap.mixed_port.start_port.into());
    mapping.insert(
        "external-controller".into(),
        format!(
            "{}:{}",
            snap.external_controller.host, snap.external_controller.port.start_port
        )
        .into(),
    );
    projected.patch_config(mapping);
    Ok(())
}

fn apply_prepared_clash_verge_projection(target: &mut IVerge, projected: &IVerge) {
    target.enable_tun_mode = projected.enable_tun_mode;
    target.web_ui_list = projected.web_ui_list.clone();
    target.enable_clash_fields = projected.enable_clash_fields;
    target.enable_random_port = projected.enable_random_port;
    target.verge_mixed_port = projected.verge_mixed_port;
    target.tun_stack = projected.tun_stack;
    target.clash_strategy = projected.clash_strategy.clone();
    target.break_when_proxy_change = projected.break_when_proxy_change;
    target.break_when_profile_change = projected.break_when_profile_change;
    target.break_when_mode_change = projected.break_when_mode_change;
}

pub(crate) fn apply_clash_config_to_legacy_verge(
    draft: &mut IVerge,
    snap: &ClashConfig,
) -> anyhow::Result<()> {
    draft.enable_tun_mode = Some(snap.enable_tun_mode);
    draft.web_ui_list = Some(snap.web_ui_list.clone());
    draft.enable_clash_fields = Some(snap.enable_clash_fields);
    draft.enable_random_port = Some(matches!(snap.mixed_port.kind, PortStrategyKind::Random));
    draft.verge_mixed_port = Some(snap.mixed_port.start_port);
    draft.tun_stack = Some(super::yaml_convert(snap.tun_stack)?);
    draft.clash_strategy = Some(ClashStrategy {
        external_controller_port_strategy: super::yaml_convert(
            &snap.external_controller.port.kind,
        )?,
    });
    let (proxy_change, profile_change, mode_change) =
        break_connection_to_legacy(&snap.break_connection);
    draft.break_when_proxy_change = Some(proxy_change);
    draft.break_when_profile_change = Some(profile_change);
    draft.break_when_mode_change = Some(mode_change);
    Ok(())
}

#[cfg(test)]
pub(crate) fn apply_clash_patch_to_legacy_verge(
    draft: &mut IVerge,
    patch: &chimera_config::clash::config::ClashConfigPatch,
    snap: &ClashConfig,
) -> anyhow::Result<()> {
    if patch.enable_tun_mode.is_some() {
        draft.enable_tun_mode = Some(snap.enable_tun_mode);
    }
    if patch.web_ui_list.is_some() {
        draft.web_ui_list = Some(snap.web_ui_list.clone());
    }
    if patch.enable_clash_fields.is_some() {
        draft.enable_clash_fields = Some(snap.enable_clash_fields);
    }
    if patch.mixed_port.is_some() {
        draft.enable_random_port = Some(matches!(snap.mixed_port.kind, PortStrategyKind::Random));
        draft.verge_mixed_port = Some(snap.mixed_port.start_port);
    }
    if patch.tun_stack.is_some() {
        draft.tun_stack = Some(super::yaml_convert(snap.tun_stack)?);
    }
    if patch.external_controller.is_some() {
        draft.clash_strategy = Some(ClashStrategy {
            external_controller_port_strategy: super::yaml_convert(
                &snap.external_controller.port.kind,
            )?,
        });
    }
    if patch.break_connection.is_some() {
        let (proxy_change, profile_change, mode_change) =
            break_connection_to_legacy(&snap.break_connection);
        draft.break_when_proxy_change = Some(proxy_change);
        draft.break_when_profile_change = Some(profile_change);
        draft.break_when_mode_change = Some(mode_change);
    }
    Ok(())
}

fn break_connection_from_legacy(legacy: &IVerge) -> BreakConnectionStrategy {
    BreakConnectionStrategy {
        on_proxy_change: legacy
            .break_when_proxy_change
            .as_ref()
            .map(proxy_change_from_legacy)
            .unwrap_or(ProxyChangeBreakMode::Off),
        on_profile_change: legacy.break_when_profile_change.unwrap_or(false),
        on_mode_change: legacy.break_when_mode_change.unwrap_or(false),
    }
}

fn proxy_change_from_legacy(value: &BreakWhenProxyChange) -> ProxyChangeBreakMode {
    match value {
        BreakWhenProxyChange::None => ProxyChangeBreakMode::Off,
        BreakWhenProxyChange::Chain => ProxyChangeBreakMode::ProxyGroup,
        BreakWhenProxyChange::All => ProxyChangeBreakMode::All,
    }
}

fn break_connection_to_legacy(
    value: &BreakConnectionStrategy,
) -> (BreakWhenProxyChange, bool, bool) {
    let proxy_change = match value.on_proxy_change {
        ProxyChangeBreakMode::Off => BreakWhenProxyChange::None,
        ProxyChangeBreakMode::ProxyGroup => BreakWhenProxyChange::Chain,
        ProxyChangeBreakMode::All => BreakWhenProxyChange::All,
    };
    (proxy_change, value.on_profile_change, value.on_mode_change)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::chimera::{ExternalControllerPortStrategy, TunStack};
    use struct_patch::Patch;

    #[test]
    fn bridge_uses_injected_legacy_lock() {
        let lock = Arc::new(parking_lot::Mutex::new(()));
        let bridge = LegacyClashBridge::new(lock.clone());
        assert!(Arc::ptr_eq(&bridge.legacy_lock, &lock));
    }

    #[test]
    fn legacy_projection_preserves_chimera_defaults() {
        let typed = clash_config_from_legacy(&IVerge::default(), &IClashTemp::template().0)
            .expect("legacy config should project");

        assert!(!typed.enable_tun_mode);
        assert!(typed.enable_clash_fields);
        assert_eq!(typed.mixed_port.kind, PortStrategyKind::Fixed);
        assert_eq!(typed.mixed_port.start_port, 7890);
        assert_eq!(
            typed.break_connection.on_proxy_change,
            ProxyChangeBreakMode::Off
        );
        assert!(!typed.break_connection.on_profile_change);
        assert!(!typed.break_connection.on_mode_change);
    }

    #[test]
    fn typed_projection_roundtrips_chimera_clash_fields() {
        let legacy = IVerge {
            enable_tun_mode: Some(true),
            web_ui_list: Some(vec!["yacd".into()]),
            enable_clash_fields: Some(false),
            enable_random_port: Some(true),
            verge_mixed_port: Some(17890),
            tun_stack: Some(TunStack::Mixed),
            clash_strategy: Some(ClashStrategy {
                external_controller_port_strategy: ExternalControllerPortStrategy::Random,
            }),
            break_when_proxy_change: Some(BreakWhenProxyChange::Chain),
            break_when_profile_change: Some(true),
            break_when_mode_change: Some(false),
            ..IVerge::default()
        };
        let typed = clash_config_from_legacy(&legacy, &IClashTemp::template().0)
            .expect("legacy config should project");

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(typed.enable_tun_mode);
        patch.web_ui_list = Some(typed.web_ui_list.clone());
        patch.enable_clash_fields = Some(typed.enable_clash_fields);
        patch.mixed_port = Some(typed.mixed_port.clone());
        patch.tun_stack = Some(typed.tun_stack);
        patch.external_controller = Some(typed.external_controller.clone());
        patch.break_connection = Some(typed.break_connection.clone());

        let mut projected = IVerge::default();
        apply_clash_patch_to_legacy_verge(&mut projected, &patch, &typed)
            .expect("typed config should project back");

        assert_eq!(projected.enable_tun_mode, legacy.enable_tun_mode);
        assert_eq!(projected.web_ui_list, legacy.web_ui_list);
        assert_eq!(projected.enable_clash_fields, legacy.enable_clash_fields);
        assert_eq!(projected.enable_random_port, legacy.enable_random_port);
        assert_eq!(projected.verge_mixed_port, legacy.verge_mixed_port);
        assert_eq!(projected.tun_stack, legacy.tun_stack);
        assert_eq!(
            projected
                .clash_strategy
                .as_ref()
                .map(|strategy| &strategy.external_controller_port_strategy),
            legacy
                .clash_strategy
                .as_ref()
                .map(|strategy| &strategy.external_controller_port_strategy),
        );
        assert_eq!(
            projected.break_when_proxy_change,
            legacy.break_when_proxy_change
        );
        assert_eq!(
            projected.break_when_profile_change,
            legacy.break_when_profile_change
        );
        assert_eq!(
            projected.break_when_mode_change,
            legacy.break_when_mode_change
        );
    }

    #[test]
    fn sparse_projection_only_updates_touched_legacy_fields() {
        let mut typed = clash_config_from_legacy(&IVerge::default(), &IClashTemp::template().0)
            .expect("legacy config should project");
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        typed.apply(patch.clone());

        let mut projected = IVerge {
            theme_mode: Some("dark".into()),
            ..IVerge::default()
        };
        apply_clash_patch_to_legacy_verge(&mut projected, &patch, &typed)
            .expect("sparse projection should succeed");

        assert_eq!(projected.enable_tun_mode, Some(true));
        assert_eq!(projected.theme_mode.as_deref(), Some("dark"));
        assert_eq!(projected.web_ui_list, None);
        assert_eq!(projected.enable_clash_fields, None);
        assert_eq!(projected.verge_mixed_port, None);
        assert!(projected.clash_strategy.is_none());
        assert_eq!(projected.break_when_mode_change, None);
    }
}
