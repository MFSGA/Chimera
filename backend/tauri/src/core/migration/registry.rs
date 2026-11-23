use super::{Ctx, MigrationStep, ModuleMigrator, modules};
use once_cell::sync::Lazy;

pub static MODULES: Lazy<chimera_migration::registry::Registry<Ctx>> = Lazy::new(|| {
    chimera_migration::registry::Registry::new(vec![
        &modules::profiles::MIGRATOR,
        &modules::typed_config::MIGRATOR,
        // Match ref: typed application.yaml must exist before moving KV hotkeys.
        &modules::storage::MIGRATOR,
    ])
    .expect("migration registry has stable module and step identities")
});

pub fn modules() -> impl Iterator<Item = &'static dyn ModuleMigrator<Ctx>> {
    MODULES.modules()
}

pub fn find_migration(id: &str) -> Option<&'static dyn MigrationStep<Ctx>> {
    MODULES.find_migration(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_migration_follows_typed_config_and_matches_ref_steps() {
        let registry = modules().collect::<Vec<_>>();
        let typed = registry
            .iter()
            .position(|module| module.module() == "typed_config")
            .unwrap();
        let storage = registry
            .iter()
            .position(|module| module.module() == "storage")
            .unwrap();
        assert!(
            typed < storage,
            "application.yaml must exist before hotkeys move from KV"
        );
        assert!(matches!(
            registry[storage].kind(),
            super::super::ModuleKind::Heuristic
        ));
        let steps = registry[storage].steps();
        assert_eq!(
            steps.len(),
            2,
            "the upstream heuristic storage module is frozen at rev2"
        );
        assert_eq!(steps[0].id(), "storage/hotkeys_to_kv");
        assert_eq!(steps[0].revision(), 1);
        assert_eq!(steps[1].id(), "storage/hotkeys_to_typed_config");
        assert_eq!(steps[1].revision(), 2);
    }

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
