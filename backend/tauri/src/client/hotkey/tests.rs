//! Platform shortcut parser integration.
use super::{
    adapters::PlatformAcceleratorValidator,
    ports::{AcceleratorValidator, HotkeyBindings, HotkeyParseError},
};
fn entries(raw: &[&str]) -> Vec<String> {
    raw.iter().map(ToString::to_string).collect()
}

#[test]
fn parser_accepts_cmd_and_normalizes_the_plus_key_for_the_platform() {
    let cmd = PlatformAcceleratorValidator
        .canonical("CMD+K")
        .expect("CMD is a supported modifier on the current platform parser");
    assert!(!cmd.is_empty());

    let shifted_plus = PlatformAcceleratorValidator
        .canonical("SHIFT+PLUS")
        .expect("the saved PLUS spelling maps to the platform Equal key");
    let shifted_equal = PlatformAcceleratorValidator
        .canonical("SHIFT+EQUAL")
        .expect("the platform key spelling should be accepted");
    assert_eq!(shifted_plus, shifted_equal);
}

#[test]
fn equivalent_platform_spellings_cannot_be_bound_to_different_actions() {
    let error = HotkeyBindings::parse(
        &entries(&["enable_tun_mode,Ctrl+Q", "disable_tun_mode,Control+Q"]),
        &PlatformAcceleratorValidator,
    )
    .expect_err("the platform maps both spellings to one global shortcut");

    assert!(matches!(
        error,
        HotkeyParseError::DuplicateAccelerator { .. }
    ));
}

#[test]
fn transparent_capture_switch_preserves_mode_and_existing_listener() {
    use chimera_config::clash::config::{
        ClashConfig, TransparentProxyMode,
        clash_strategy::port::{PortStrategy, PortStrategyKind},
    };
    let mut config = ClashConfig::default();
    config.tproxy_port = None;
    let patch = super::switch_patch(&config, true);
    assert_eq!(
        patch.transparent_proxy.mode,
        Some(TransparentProxyMode::Tproxy)
    );
    assert_eq!(
        patch.tproxy_port,
        Some(Some(PortStrategy {
            kind: PortStrategyKind::Fixed,
            start_port: 7894
        }))
    );
    config.tproxy_port = Some(PortStrategy::new_allow_fallback(7895));
    assert!(super::switch_patch(&config, true).tproxy_port.is_none());
    config.transparent_proxy.mode = TransparentProxyMode::Redir;
    let keep = super::switch_patch(&config, true);
    assert_eq!(
        keep.transparent_proxy.mode,
        Some(TransparentProxyMode::Redir)
    );
    assert!(keep.tproxy_port.is_none());
    let disable = super::switch_patch(&config, false);
    assert_eq!(
        disable.transparent_proxy.mode,
        Some(TransparentProxyMode::Disabled)
    );
    assert!(disable.tproxy_port.is_none());
}
