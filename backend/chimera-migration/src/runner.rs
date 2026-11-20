//! Pure migration scheduling decisions from the ref Runner path.
//! IO, backup creation, and module execution remain injected by the application adapter.

use crate::{MigrationAdvice, MigrationState, StepSpec, store::MigrationStore};
use semver::Version;

pub fn advice_step(
    store: &MigrationStore,
    step: StepSpec<'_>,
    target: &Version,
    force: bool,
    enabled: bool,
) -> MigrationAdvice {
    if !enabled {
        return MigrationAdvice::Ignored;
    }
    if force {
        return MigrationAdvice::Pending;
    }
    match store.task_state(step.id) {
        Some(MigrationState::Completed | MigrationState::Skipped) => return MigrationAdvice::Done,
        Some(MigrationState::Failed | MigrationState::InProgress | MigrationState::NotStarted) => {
            return MigrationAdvice::Pending;
        }
        None => {}
    }
    if step.revision > store.module_state(step.module).applied_revision
        && introduced_in_reached(step.introduced_in, target)
    {
        MigrationAdvice::Pending
    } else {
        MigrationAdvice::Ignored
    }
}

/// Compare the release triple so an rc build executes migrations for the same release.
pub fn introduced_in_reached(introduced_in: &Version, target: &Version) -> bool {
    (
        introduced_in.major,
        introduced_in.minor,
        introduced_in.patch,
    ) <= (target.major, target.minor, target.patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prerelease_matches_release_but_not_future_release() {
        assert!(introduced_in_reached(
            &Version::parse("0.28.3").unwrap(),
            &Version::parse("0.28.3-rc.1").unwrap()
        ));
        assert!(!introduced_in_reached(
            &Version::parse("0.28.4").unwrap(),
            &Version::parse("0.28.3").unwrap()
        ));
    }

    #[test]
    fn all_durable_step_states_follow_runner_retry_policy() {
        use crate::store::TaskRecord;
        let mut store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        for (state, expected) in [
            (MigrationState::NotStarted, MigrationAdvice::Pending),
            (MigrationState::InProgress, MigrationAdvice::Pending),
            (MigrationState::Failed, MigrationAdvice::Pending),
            (MigrationState::Skipped, MigrationAdvice::Done),
            (MigrationState::Completed, MigrationAdvice::Done),
        ] {
            store.tasks.insert(
                step.id.into(),
                TaskRecord {
                    module: step.module.into(),
                    revision: step.revision,
                    introduced_in: version.clone(),
                    state,
                    started_at: None,
                    finished_at: None,
                    error: None,
                },
            );
            assert_eq!(advice_step(&store, step, &version, false, true), expected);
            assert_eq!(
                advice_step(&store, step, &version, true, true),
                MigrationAdvice::Pending
            );
            assert_eq!(
                advice_step(&store, step, &version, true, false),
                MigrationAdvice::Ignored
            );
        }
    }

    #[test]
    fn a_future_step_is_ignored_until_target_release() {
        let store = MigrationStore::default();
        let introduced = Version::new(0, 29, 0);
        let older = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "future/step",
            module: "future",
            revision: 1,
            introduced_in: &introduced,
        };
        assert_eq!(
            advice_step(&store, step, &older, false, true),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &introduced, false, true),
            MigrationAdvice::Pending
        );
    }

    #[test]
    fn disabled_steps_and_completed_steps_are_not_rerun_unless_forced() {
        let store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        assert_eq!(
            advice_step(&store, step, &version, false, false),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &version, false, true),
            MigrationAdvice::Pending
        );
        assert_eq!(
            advice_step(&store, step, &version, true, true),
            MigrationAdvice::Pending
        );
    }
}
