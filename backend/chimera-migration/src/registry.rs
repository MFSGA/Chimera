//! Reference-aligned migration registry: module order and step validation.
//! The desktop adapter provides concrete module implementations without
//! duplicating lookup, revision ordering, or identity checks.

use crate::module::{MigrationStep, ModuleMigrator};
use std::collections::HashSet;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RegistryError {
    #[error("duplicate migration module: {0}")]
    DuplicateModule(String),
    #[error("duplicate migration step: {0}")]
    DuplicateStep(String),
    #[error("step {step} belongs to {found}, not {expected}")]
    WrongModule {
        step: String,
        expected: String,
        found: String,
    },
    #[error("module {module} has invalid or non-increasing revision {revision} after {previous}")]
    InvalidRevision {
        module: String,
        revision: u64,
        previous: u64,
    },
}

/// The ordered, validated set of modules owned by a migration runner.
/// A shell supplies the module implementations; the shared crate enforces
/// stable ids and strictly ascending revisions across each module.
pub struct Registry<Ctx: 'static> {
    modules: Vec<&'static dyn ModuleMigrator<Ctx>>,
}

impl<Ctx: 'static> Registry<Ctx> {
    pub fn new(modules: Vec<&'static dyn ModuleMigrator<Ctx>>) -> Result<Self, RegistryError> {
        let mut names = HashSet::new();
        let mut ids = HashSet::new();
        for module in &modules {
            let name = module.module();
            if !names.insert(name) {
                return Err(RegistryError::DuplicateModule(name.to_owned()));
            }
            let mut previous = 0;
            for step in module.steps() {
                if step.module() != name {
                    return Err(RegistryError::WrongModule {
                        step: step.id().to_owned(),
                        expected: name.to_owned(),
                        found: step.module().to_owned(),
                    });
                }
                let revision = step.revision();
                if revision <= previous {
                    return Err(RegistryError::InvalidRevision {
                        module: name.to_owned(),
                        revision,
                        previous,
                    });
                }
                previous = revision;
                if !ids.insert(step.id()) {
                    return Err(RegistryError::DuplicateStep(step.id().to_owned()));
                }
            }
        }
        Ok(Self { modules })
    }

    pub fn modules(&self) -> impl Iterator<Item = &'static dyn ModuleMigrator<Ctx>> + '_ {
        self.modules.iter().copied()
    }

    pub fn steps(&self) -> impl Iterator<Item = &'static dyn MigrationStep<Ctx>> + '_ {
        self.modules()
            .flat_map(|module| module.steps().iter().copied())
    }

    pub fn find_migration(&self, id: &str) -> Option<&'static dyn MigrationStep<Ctx>> {
        self.steps().find(|step| step.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::{ModuleKind, StepCheck};
    use semver::Version;

    struct DummyCtx;
    struct FakeStep {
        id: &'static str,
        module: &'static str,
        revision: u64,
    }
    static VERSION: Version = Version::new(0, 28, 3);
    impl MigrationStep<DummyCtx> for FakeStep {
        fn id(&self) -> &'static str {
            self.id
        }
        fn module(&self) -> &'static str {
            self.module
        }
        fn revision(&self) -> u64 {
            self.revision
        }
        fn introduced_in(&self) -> &'static Version {
            &VERSION
        }
        fn name(&self) -> &'static str {
            "test"
        }
        fn check(
            &self,
            _: &DummyCtx,
        ) -> Result<Option<StepCheck>, crate::module::MigrationCheckError> {
            Ok(Some(StepCheck::Satisfied))
        }
        fn run(&self, _: &mut DummyCtx) -> anyhow::Result<()> {
            Ok(())
        }
    }
    struct FakeModule {
        name: &'static str,
        steps: &'static [&'static dyn MigrationStep<DummyCtx>],
    }
    impl ModuleMigrator<DummyCtx> for FakeModule {
        fn module(&self) -> &'static str {
            self.name
        }
        fn kind(&self) -> ModuleKind<DummyCtx> {
            ModuleKind::Heuristic
        }
        fn detect_baseline(&self, _: &DummyCtx) -> anyhow::Result<u64> {
            Ok(0)
        }
        fn steps(&self) -> &'static [&'static dyn MigrationStep<DummyCtx>] {
            self.steps
        }
    }

    static FIRST: FakeStep = FakeStep {
        id: "storage/hotkeys_to_kv",
        module: "storage",
        revision: 1,
    };
    static SECOND: FakeStep = FakeStep {
        id: "storage/hotkeys_to_typed_config",
        module: "storage",
        revision: 2,
    };
    static BAD_ORDER: FakeStep = FakeStep {
        id: "storage/out_of_order",
        module: "storage",
        revision: 1,
    };
    static BAD_MODULE: FakeStep = FakeStep {
        id: "other/mismatch",
        module: "other",
        revision: 2,
    };
    static STEPS: [&dyn MigrationStep<DummyCtx>; 2] = [&FIRST, &SECOND];
    static OUT_OF_ORDER: [&dyn MigrationStep<DummyCtx>; 2] = [&SECOND, &BAD_ORDER];
    static WRONG_MODULE_STEPS: [&dyn MigrationStep<DummyCtx>; 2] = [&FIRST, &BAD_MODULE];
    static STORAGE: FakeModule = FakeModule {
        name: "storage",
        steps: &STEPS,
    };
    static STORAGE_AGAIN: FakeModule = FakeModule {
        name: "storage",
        steps: &STEPS,
    };
    static UNORDERED: FakeModule = FakeModule {
        name: "storage",
        steps: &OUT_OF_ORDER,
    };
    static WRONG: FakeModule = FakeModule {
        name: "storage",
        steps: &WRONG_MODULE_STEPS,
    };

    #[test]
    fn preserves_module_step_order_and_lookup() {
        let registry = Registry::new(vec![&STORAGE as &dyn ModuleMigrator<DummyCtx>]).unwrap();
        assert_eq!(
            registry
                .modules()
                .map(|module| module.module())
                .collect::<Vec<_>>(),
            ["storage"]
        );
        assert_eq!(
            registry.steps().map(|step| step.id()).collect::<Vec<_>>(),
            [FIRST.id, SECOND.id]
        );
        assert_eq!(registry.find_migration(SECOND.id).unwrap().revision(), 2);
        assert!(registry.find_migration("unknown").is_none());
    }

    #[test]
    fn rejects_duplicate_modules_non_increasing_revisions_and_wrong_ownership() {
        assert!(matches!(
            Registry::new(vec![
                &STORAGE as &dyn ModuleMigrator<DummyCtx>,
                &STORAGE_AGAIN
            ]),
            Err(RegistryError::DuplicateModule(_))
        ));
        assert!(matches!(
            Registry::new(vec![&UNORDERED as &dyn ModuleMigrator<DummyCtx>]),
            Err(RegistryError::InvalidRevision { .. })
        ));
        assert!(matches!(
            Registry::new(vec![&WRONG as &dyn ModuleMigrator<DummyCtx>]),
            Err(RegistryError::WrongModule { .. })
        ));
    }
}
