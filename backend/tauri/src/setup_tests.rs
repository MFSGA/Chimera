use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use crate::startup_failure::{FailureKind, Report, StartupFailure, StartupStage};
use chimera_core_manager::{
    ControlOptions, CoreControl, CoreManager, ExecutorExit, ManagerOptions,
};

use super::{StartupFailureHost, StartupOwners};

#[derive(Default)]
struct Host {
    events: RefCell<Vec<&'static str>>,
    report: RefCell<Option<Report>>,
    owners: RefCell<Option<StartupOwners>>,
}

impl StartupFailureHost for Host {
    fn show(&self, report: Report) {
        self.events.borrow_mut().push("dialog");
        self.report.replace(Some(report));
    }

    fn retain(&self, owners: StartupOwners) {
        self.events.borrow_mut().push("retain");
        self.owners.replace(Some(owners));
    }

    fn exit(&self, code: i32) {
        assert_eq!(code, 1);
        assert!(
            self.owners
                .borrow()
                .as_ref()
                .is_some_and(|owners| !owners.shutdown.is_cancelled())
        );
        self.events.borrow_mut().push("exit");
    }
}

fn failure_report() -> Report {
    Report {
        failure: StartupFailure {
            stage: StartupStage::Setup,
            kind: FailureKind::Internal,
            path: None,
            detail: "injected setup failure".to_owned(),
            revision: None,
        },
        version: "test".to_owned(),
        logs_dir: std::path::PathBuf::from("/test/logs"),
        backups_dir: None,
    }
}

#[test]
fn setup_failure_retains_partial_owners_for_the_exit_boundary() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _entered = runtime.enter();
    let host = Host::default();
    let task_finished = Arc::new(AtomicBool::new(false));
    let task_finished_in_owner = task_finished.clone();

    // Initial: the failing setup retains an active owner for the event loop.
    // Operation: exit boundary requests shutdown after the dialog and retain.
    // Expected: the tracked task drains; eager cancellation in setup would
    // cause the ownership assertion in Host::exit to fail.
    let result = super::run_setup(
        StartupOwners::default(),
        |owners| {
            let shutdown = owners.shutdown.clone();
            owners.tasks.spawn(async move {
                shutdown.cancelled().await;
                task_finished_in_owner.store(true, Ordering::SeqCst);
            });
            Err(anyhow::anyhow!("injected setup failure"))
        },
        |_| failure_report(),
        &host,
    );

    assert!(result.is_ok());
    assert_eq!(*host.events.borrow(), ["dialog", "retain", "exit"]);
    assert_eq!(
        host.report.borrow().as_ref().unwrap().failure.stage,
        StartupStage::Setup
    );

    let owners = host.owners.borrow_mut().take().unwrap();
    assert!(!owners.shutdown.is_cancelled());
    owners.request_shutdown();
    runtime.block_on(owners.wait_shutdown());
    assert!(task_finished.load(Ordering::SeqCst));
}

#[test]
fn successful_setup_does_not_show_failure_or_request_exit() {
    let host = Host::default();
    let result = super::run_setup(
        StartupOwners::default(),
        |_| Ok(()),
        |_| failure_report(),
        &host,
    );

    assert!(result.is_ok());
    assert!(host.events.borrow().is_empty());
    assert!(host.report.borrow().is_none());
    assert!(host.owners.borrow().is_none());
}

#[test]
fn partial_core_owner_shuts_down_after_tracked_tasks_drain() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let _entered = runtime.enter();
    let directory = tempfile::tempdir().unwrap();
    let runtime_dir = camino::Utf8PathBuf::from_path_buf(directory.path().join("runtime"))
        .expect("test runtime path should be UTF-8");
    let manager = runtime
        .block_on(
            CoreManager::builder(ManagerOptions {
                runtime_dir: Some(runtime_dir.clone()),
                log_sink_enabled: false,
                ..ManagerOptions::default()
            })
            .build(),
        )
        .unwrap();
    let control = CoreControl::spawn(
        manager,
        ControlOptions::new(runtime_dir.join("staging"), runtime_dir.join("working")),
    );
    // Initial: the control executor is alive but idle. Operation: request the
    // partial owner shutdown and drain it. Expected: the independent executor
    // completion signal reports a clean exit; skipping core shutdown leaves it
    // pending and fails the test.
    let owners = StartupOwners {
        legacy_core: Some(control.clone()),
        ..StartupOwners::default()
    };

    owners.request_shutdown();
    runtime.block_on(owners.wait_shutdown());

    assert_eq!(
        runtime.block_on(control.until_closed()),
        ExecutorExit::Clean
    );
}

#[test]
fn startup_migrates_a_fresh_install_before_creating_stamped_profile_defaults() {
    use crate::utils::path::PathResolver;
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let paths = PathResolver::with_base_dirs(config, data);
    let capture = parking_lot::Mutex::new(None);
    assert!(!paths.chimera_config_path().exists());
    assert!(!paths.profiles_path().exists());

    let runner = super::migrate_before_legacy_defaults(&paths, &capture).unwrap();
    assert!(
        runner.backup_info().is_none(),
        "no fake pre-step snapshot for a fresh install"
    );
    assert!(capture.lock().is_none());
    assert!(
        paths.chimera_config_path().is_file(),
        "legacy compatibility default is created afterward"
    );
    assert!(paths.clash_guard_overrides_path().is_file());
    let profiles = chimera_migration::document::read_document(&paths.profiles_path(), "profiles")
        .unwrap()
        .expect("post-migration profiles default exists");
    assert_eq!(profiles.schema_revision, Some(4));
    let parsed: chimera_config::profile::Profiles =
        serde_yaml_ng::from_value(serde_yaml_ng::Value::Mapping(profiles.payload)).unwrap();
    assert!(parsed.items.is_empty());
    let first = std::fs::read(paths.profiles_path()).unwrap();
    let first_app = std::fs::read(paths.application_config_path()).unwrap();

    // A later startup must accept the stamped default and leave content unchanged.
    let restarted = super::migrate_before_legacy_defaults(&paths, &capture).unwrap();
    assert!(restarted.backup_info().is_none());
    assert_eq!(std::fs::read(paths.profiles_path()).unwrap(), first);
    assert_eq!(
        std::fs::read(paths.application_config_path()).unwrap(),
        first_app
    );
}

#[test]
fn migration_failure_does_not_generate_missing_legacy_defaults_or_touch_typed_input() {
    use crate::{startup_failure::StartupStage, utils::path::PathResolver};
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let paths = PathResolver::with_base_dirs(config, data);
    let original = b"language: en\n";
    std::fs::write(paths.application_config_path(), original).unwrap();
    let capture = parking_lot::Mutex::new(None);
    let error = super::migrate_before_legacy_defaults(&paths, &capture)
        .expect_err("partial typed files must fail migration before defaults are emitted");
    assert!(
        format!("{error:#}").contains("partial typed config"),
        "{error:#}"
    );
    let report = capture
        .lock()
        .clone()
        .expect("migration preparation failure is reported");
    assert_eq!(report.failure.stage, StartupStage::MigrationPrepare);
    assert!(report.backups_dir.is_none());
    assert_eq!(
        std::fs::read(paths.application_config_path()).unwrap(),
        original
    );
    for absent in [
        paths.chimera_config_path(),
        paths.profiles_path(),
        paths.clash_guard_overrides_path(),
    ] {
        assert!(
            !absent.exists(),
            "startup did not generate default {} before migration",
            absent.display()
        );
    }
}

#[test]
fn migration_run_failure_retains_verified_backup_and_never_initializes_defaults() {
    use crate::{startup_failure::StartupStage, utils::path::PathResolver};
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config");
    let data = temp.path().join("data");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    let paths = PathResolver::with_base_dirs(config, data);
    let invalid = b"enable_system_proxy: [invalid yaml";
    std::fs::write(paths.chimera_config_path(), invalid).unwrap();
    let capture = parking_lot::Mutex::new(None);

    let error = super::migrate_before_legacy_defaults(&paths, &capture)
        .expect_err("invalid legacy YAML must stop client startup");
    assert!(format!("{error:#}").contains("failed to read legacy config"));
    let report = capture
        .lock()
        .clone()
        .expect("migration error report must be captured");
    assert_eq!(report.failure.stage, StartupStage::MigrationRun);
    assert_eq!(report.backups_dir, Some(paths.backups_dir()));
    assert_eq!(std::fs::read(paths.chimera_config_path()).unwrap(), invalid);
    assert!(!paths.clash_guard_overrides_path().exists());
    assert!(!paths.profiles_path().exists());
    assert!(!paths.application_config_path().exists());
    assert!(!paths.session_state_path().exists());
    assert!(!paths.clash_config_path().exists());
    let snapshots = std::fs::read_dir(paths.backups_dir())
        .unwrap()
        .collect::<std::io::Result<Vec<_>>>()
        .unwrap();
    assert_eq!(snapshots.len(), 1);
    let snapshot = snapshots[0].path();
    assert_eq!(
        std::fs::read(snapshot.join("config/chimera-config.yaml")).unwrap(),
        invalid
    );
    assert!(!snapshot.join("config/clash-guard-overrides.yaml").exists());
    assert!(!snapshot.join("config/profiles.yaml").exists());
}
