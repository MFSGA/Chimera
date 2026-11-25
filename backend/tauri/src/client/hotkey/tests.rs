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
