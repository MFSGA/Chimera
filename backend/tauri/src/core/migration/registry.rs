use super::{ModuleMigrator, modules};
use once_cell::sync::Lazy;

pub static MODULES: Lazy<Vec<&'static dyn ModuleMigrator>> =
    Lazy::new(|| vec![&modules::typed_config::MIGRATOR]);

pub fn modules() -> impl Iterator<Item = &'static dyn ModuleMigrator> {
    MODULES.iter().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_steps_are_sorted_by_revision() {
        for module in modules() {
            let mut previous = 0;
            for step in module.steps() {
                assert!(
                    step.revision() > previous,
                    "{} revisions must be strictly ascending",
                    module.module()
                );
                previous = step.revision();
            }
        }
    }
}
