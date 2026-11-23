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
        core: Some(control.clone()),
        ..StartupOwners::default()
    };

    owners.request_shutdown();
    runtime.block_on(owners.wait_shutdown());

    assert_eq!(
        runtime.block_on(control.until_closed()),
        ExecutorExit::Clean
    );
}
