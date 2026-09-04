use super::{
    Ctx, MigrationAdvice, MigrationState, MigrationStep, current_version, registry,
    store::MigrationStore,
};
use crate::utils::path::PathResolver;
use anyhow::Context;
use semver::Version;

#[derive(Debug)]
pub struct Runner {
    target: Version,
    force: bool,
    ctx: Ctx,
    store: MigrationStore,
}

impl Runner {
    pub fn new(force: bool) -> anyhow::Result<Self> {
        Self::with_target(current_version()?, force)
    }

    pub fn with_target(target: Version, force: bool) -> anyhow::Result<Self> {
        Self::with_context(target, force, Ctx::from_app_dirs()?)
    }

    pub fn with_paths(paths: PathResolver, force: bool) -> anyhow::Result<Self> {
        Self::with_context(current_version()?, force, Ctx::from_paths(paths))
    }

    #[cfg(test)]
    pub fn with_target_and_paths(
        target: Version,
        paths: PathResolver,
        force: bool,
    ) -> anyhow::Result<Self> {
        Self::with_context(target, force, Ctx::from_paths(paths))
    }

    pub fn advice_step(&self, step: &dyn MigrationStep) -> MigrationAdvice {
        if self.force {
            return MigrationAdvice::Pending;
        }

        match self.store.task_state(step.id()) {
            Some(MigrationState::Completed) => return MigrationAdvice::Done,
            Some(
                MigrationState::Failed | MigrationState::InProgress | MigrationState::NotStarted,
            ) => return MigrationAdvice::Pending,
            None => {}
        }

        let module_state = self.store.module_state(step.module());
        if step.revision() > module_state.applied_revision
            && introduced_in_reached(step.introduced_in(), &self.target)
        {
            MigrationAdvice::Pending
        } else {
            MigrationAdvice::Ignored
        }
    }

    pub fn run_pending(&mut self) -> anyhow::Result<()> {
        let mut first_error = None;

        for module in registry::modules() {
            for step in module.steps() {
                if self.advice_step(*step) != MigrationAdvice::Pending {
                    continue;
                }

                if let Err(error) = self.run_step(*step) {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                    break;
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        self.store.set_last_succeeded(self.target.clone());
        self.store
            .flush_atomic(&self.ctx.state_path())
            .context("failed to persist successful migration state")?;
        Ok(())
    }

    fn with_context(target: Version, force: bool, ctx: Ctx) -> anyhow::Result<Self> {
        let state_path = ctx.state_path();
        let store = MigrationStore::load(&state_path)?;
        let mut runner = Self {
            target,
            force,
            ctx,
            store,
        };
        runner.ensure_baselines()?;
        Ok(runner)
    }

    fn ensure_baselines(&mut self) -> anyhow::Result<()> {
        let mut changed = false;
        for module in registry::modules() {
            changed |= self.store.ensure_module(module, &self.ctx)?;
        }

        if changed {
            self.store
                .flush_atomic(&self.ctx.state_path())
                .context("failed to persist migration baselines")?;
        }
        Ok(())
    }

    fn run_step(&mut self, step: &dyn MigrationStep) -> anyhow::Result<()> {
        self.store.mark_in_progress(step);
        self.store
            .flush_atomic(&self.ctx.state_path())
            .with_context(|| format!("failed to persist {} in-progress state", step.id()))?;

        match step.run(&mut self.ctx) {
            Ok(()) => {
                self.store.mark_completed(step);
                self.store.bump_module(step.module());
                self.store
                    .flush_atomic(&self.ctx.state_path())
                    .with_context(|| format!("failed to persist {} completed state", step.id()))?;
                Ok(())
            }
            Err(error) => {
                let _ = step.rollback(&mut self.ctx);
                self.store.mark_failed(step, &error);
                if let Err(flush_error) = self.store.flush_atomic(&self.ctx.state_path()) {
                    return Err(error.context(format!(
                        "failed to persist {} failed state: {flush_error:#}",
                        step.id()
                    )));
                }
                Err(error)
            }
        }
    }
}

fn introduced_in_reached(introduced_in: &Version, target: &Version) -> bool {
    (target.major, target.minor, target.patch)
        >= (
            introduced_in.major,
            introduced_in.minor,
            introduced_in.patch,
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_target_reaches_same_release_migration() {
        assert!(introduced_in_reached(
            &Version::parse("0.23.0").unwrap(),
            &Version::parse("0.23.0-rc.1").unwrap()
        ));
    }

    #[test]
    fn runner_migrates_once_and_persists_completion() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("config");
        let data = temp.path().join("data");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&data).unwrap();

        let paths = PathResolver::with_base_dirs(config.clone(), data);
        let mut runner =
            Runner::with_target_and_paths(Version::parse("0.23.0").unwrap(), paths.clone(), false)
                .unwrap();
        runner.run_pending().unwrap();

        assert!(config.join("application.yaml").exists());
        assert!(config.join("session-state.yaml").exists());
        assert!(config.join("clash-config.yaml").exists());
        assert!(config.join("migration-state.yaml").exists());

        let before = std::fs::read(config.join("clash-config.yaml")).unwrap();
        let mut rerun =
            Runner::with_target_and_paths(Version::parse("0.23.0").unwrap(), paths, false).unwrap();
        rerun.run_pending().unwrap();
        assert_eq!(
            std::fs::read(config.join("clash-config.yaml")).unwrap(),
            before
        );
    }
}
