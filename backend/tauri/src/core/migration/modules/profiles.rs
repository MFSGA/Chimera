//! Tauri compatibility re-export for the shared Profiles Migrator.
//! The complete Profiles document conversion and schema migration is owned
//! by `chimera-migration`; only the real Tauri startup regression remains here.

pub use chimera_migration::modules::profiles::{
    CLEAN_SCHEMA, CleanSchemaError, MIGRATOR, MigrateProfileScriptNewtype,
    MigrateProfilesCleanSchema, MigrateProfilesNullValue, MigrateProfilesRepairSchema, NULL_VALUE,
    ProfilesDocument, ProfilesFormat, ProfilesMigrator, REPAIR_SCHEMA, SCRIPT_NEWTYPE,
    VERSION_0_24_1,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::migration::MigrationStep;
    use chimera_persistence::format::StampedDocument;

    #[test]
    fn completed_clean_schema_state_can_repair_legacy_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let config_dir = temp.path().join("config");
        let data_dir = temp.path().join("data");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        let path = config_dir.join("profiles.yaml");
        let original = "current: [a]\nitems:\n- {uid: a, type: local, name: A, file: a.yaml}\n";
        std::fs::write(&path, original).unwrap();
        let mut state = crate::core::migration::store::MigrationStore::default();
        state.modules.insert(
            "profiles".to_owned(),
            crate::core::migration::store::ModuleState {
                applied_revision: 3,
                baseline_revision: 2,
                stamped: false,
            },
        );
        state.tasks.insert(
            "profiles/clean_schema".to_owned(),
            crate::core::migration::store::TaskRecord {
                module: "profiles".to_owned(),
                revision: 3,
                introduced_in: (*VERSION_0_24_1).clone(),
                state: crate::core::migration::MigrationState::Completed,
                started_at: None,
                finished_at: None,
                error: None,
            },
        );
        state
            .flush_atomic(&config_dir.join("migration-state.yaml"))
            .unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(config_dir, data_dir);
        let mut runner = crate::core::migration::Runner::with_paths(paths, false).unwrap();

        assert_eq!(
            runner.advice_step(&REPAIR_SCHEMA),
            crate::core::migration::MigrationAdvice::Pending
        );
        runner.run_migration(&REPAIR_SCHEMA).unwrap();
        let state = crate::core::migration::store::MigrationStore::load(
            &path.parent().unwrap().join("migration-state.yaml"),
        )
        .unwrap();
        assert_eq!(state.module_state("profiles").applied_revision, 4);

        let raw = std::fs::read_to_string(&path).unwrap();
        let profiles: chimera_config::profile::Profiles = serde_yaml_ng::from_str(&raw).unwrap();
        assert_eq!(profiles.current.unwrap().0, "a");
        assert_eq!(
            std::fs::read_to_string(path.with_extension("yaml.recovery.bak")).unwrap(),
            original
        );
    }
}
