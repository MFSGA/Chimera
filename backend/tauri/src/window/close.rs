//! Compatibility re-export until the window module is fully reorganized
//! as ref's `window/mod.rs`. The policy is owned by the typed config crate.
pub use chimera_config::application::{
    WindowCloseBehavior, WindowCloseOverride, WindowCloseSettings,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_closing_matches_the_existing_destroy_behavior() {
        let settings = WindowCloseSettings::default();
        assert_eq!(
            settings.main.resolve(settings.global),
            WindowCloseBehavior::Destroy
        );
        assert_eq!(
            settings.editor.resolve(settings.global),
            WindowCloseBehavior::Destroy
        );
        assert_eq!(
            WindowCloseOverride::Hide.resolve(settings.global),
            WindowCloseBehavior::Hide
        );
    }
}
