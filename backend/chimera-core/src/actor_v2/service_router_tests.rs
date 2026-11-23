use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use super::*;

fn service_command_error(reason: &'static str) -> chimera_contracts::service::ServiceCommandError {
    chimera_contracts::service::ServiceCommandError::RunServiceStatus {
        source: std::io::Error::other(reason),
    }
}
use crate::actor_v2::endpoint::{
    ControlEndpoint, CoreStatusSnapshot, CoreSubmission, ExecutionHost,
};
use chimera_ipc::api::status::{CoreInfos, CoreState, StatusResBody};
use std::borrow::Cow;

/// A scriptable daemon: `state` drives what probe answers; commands
/// mutate it the way the real SCM would.
struct FakeDaemon {
    /// (installed, running, version)
    state: Mutex<(bool, bool, String)>,
    installs: AtomicUsize,
    starts: AtomicUsize,
    updates: AtomicUsize,
    uninstalls: AtomicUsize,
    restarts: AtomicUsize,
    /// What `/status` publishes as the core's detail. `None` is a daemon
    /// that answered without one -- the pre-detail wire.
    core_detail: Mutex<Option<CoreStateDetail>>,
    /// A running daemon whose `/status` does not come back at all.
    probe_blind: AtomicBool,
    command_failure: Mutex<Option<ServiceCommandError>>,
    fail_install: AtomicBool,
    fail_start: AtomicBool,
    /// F5: the probe call itself fails -- not "answered Stopped", not
    /// "answered Running without a server body", but no answer at all.
    probe_fail: AtomicBool,
    session_ending: AtomicBool,
    /// F6: `install` never resolves, so only the actor's own bound can
    /// end it.
    hang_install: AtomicBool,
    hang_start: AtomicBool,
    /// `install` and `update` held at a barrier the test releases, the
    /// shape of an elevated command that outlives the actor's bound and
    /// then finishes on its own.
    park_install: AtomicBool,
    park_update: AtomicBool,
    parked: tokio::sync::Notify,
    release: tokio::sync::Notify,
    stop_succeeds: AtomicBool,
    calls: Mutex<Vec<&'static str>>,
}

impl FakeDaemon {
    fn new(installed: bool, running: bool, version: &str) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new((installed, running, version.to_owned())),
            installs: AtomicUsize::new(0),
            starts: AtomicUsize::new(0),
            updates: AtomicUsize::new(0),
            uninstalls: AtomicUsize::new(0),
            restarts: AtomicUsize::new(0),
            core_detail: Mutex::new(Some(CoreStateDetail::Stopped { reason: None })),
            probe_blind: AtomicBool::new(false),
            command_failure: Mutex::new(None),
            fail_install: AtomicBool::new(false),
            fail_start: AtomicBool::new(false),
            probe_fail: AtomicBool::new(false),
            session_ending: AtomicBool::new(false),
            hang_install: AtomicBool::new(false),
            hang_start: AtomicBool::new(false),
            park_install: AtomicBool::new(false),
            park_update: AtomicBool::new(false),
            parked: tokio::sync::Notify::new(),
            release: tokio::sync::Notify::new(),
            stop_succeeds: AtomicBool::new(true),
            calls: Mutex::new(Vec::new()),
        })
    }

    fn set_detail(&self, detail: Option<CoreStateDetail>) {
        *self.core_detail.lock().unwrap() = detail;
    }
}

struct NullEndpoint;
#[async_trait::async_trait]
impl ControlEndpoint for NullEndpoint {
    fn host(&self) -> ExecutionHost {
        ExecutionHost::Service
    }
    async fn submit(
        &self,
        _submission: CoreSubmission,
    ) -> Result<chimera_ipc::api::core::v2::OperationInfo, CoreError> {
        unimplemented!("routing is the CoreActor business")
    }
    async fn wait_operation(
        &self,
        _id: chimera_core_manager::OperationId,
        _timeout: std::time::Duration,
    ) -> Option<chimera_ipc::api::core::v2::OperationInfo> {
        unimplemented!()
    }
    async fn status(&self) -> Result<CoreStatusSnapshot, CoreError> {
        unimplemented!()
    }
}

#[async_trait::async_trait]
impl ServiceProcessControl for FakeDaemon {
    async fn probe(&self) -> Result<String, chimera_contracts::service::ServiceCommandError> {
        self.calls.lock().unwrap().push("probe");
        if self.session_ending.load(Ordering::SeqCst) {
            return Err(ServiceCommandError::SessionEnding);
        }
        if self.probe_fail.load(Ordering::SeqCst) {
            return Err(service_command_error("probe unreachable"));
        }
        let (installed, running, version) = self.state.lock().unwrap().clone();
        let status = match (installed, running) {
            (false, _) => ServiceStatus::NotInstalled,
            (true, false) => ServiceStatus::Stopped,
            (true, true) => ServiceStatus::Running,
        };
        let detail = self.core_detail.lock().unwrap().clone();
        let blind = self.probe_blind.load(Ordering::SeqCst);
        let server = (installed && running && !blind).then(|| StatusResBody {
            version: Cow::Owned(version.clone()),
            core_infos: CoreInfos {
                instance_id: None,
                r#type: None,
                // The coarse projection a real daemon publishes: every
                // transitional detail collapses into one of these two
                // shapes, which is exactly why it cannot be a guard.
                state: match detail {
                    Some(CoreStateDetail::Running { .. })
                    | Some(CoreStateDetail::Switching { .. })
                    | Some(CoreStateDetail::Stopping { .. }) => CoreState::Running,
                    _ => CoreState::Stopped(None),
                },
                state_changed_at: 0,
                config_path: None,
                controller: None,
                health: None,
                revision: None,
                detail,
            },
            runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                service_data_dir: Cow::Owned(std::path::PathBuf::new()),
                service_config_dir: Cow::Owned(std::path::PathBuf::new()),
                nyanpasu_config_dir: Cow::Owned(std::path::PathBuf::new()),
                nyanpasu_data_dir: Cow::Owned(std::path::PathBuf::new()),
            },
        });
        Ok(serde_json::to_string(&StatusInfo {
            name: Cow::Borrowed("chimera-service"),
            version: Cow::Borrowed("test"),
            status,
            server,
        })
        .expect("test service status serializes"))
    }
    async fn install(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        if self.hang_install.load(Ordering::SeqCst) {
            // Never resolves: only the actor's own `command_timeout`
            // bound (F6) can end this call.
            return std::future::pending::<
                Result<(), chimera_contracts::service::ServiceCommandError>,
            >()
            .await;
        }
        if self.park_install.load(Ordering::SeqCst) {
            self.parked.notify_one();
            self.release.notified().await;
        }

        self.installs.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.command_failure.lock().unwrap().take() {
            return Err(error);
        }
        if self.fail_install.load(Ordering::SeqCst) {
            return Err(service_command_error("install refused"));
        }
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        state.1 = true; // install auto-starts, like most platforms
        Ok(())
    }
    async fn uninstall(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        self.calls.lock().unwrap().push("uninstall");
        self.uninstalls.fetch_add(1, Ordering::SeqCst);
        *self.state.lock().unwrap() = (false, false, String::new());
        Ok(())
    }
    async fn start_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = self.command_failure.lock().unwrap().take() {
            return Err(error);
        }
        if self.hang_start.load(Ordering::SeqCst) {
            return std::future::pending().await;
        }
        if self.fail_start.load(Ordering::SeqCst) {
            return Err(service_command_error("start refused"));
        }
        self.state.lock().unwrap().1 = true;
        Ok(())
    }
    async fn stop_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        self.calls.lock().unwrap().push("stop_daemon");
        if let Some(error) = self.command_failure.lock().unwrap().take() {
            return Err(error);
        }
        // Like `nyanpasu-service stop`, which exits with an error for a
        // service that is already stopped.
        if !self.state.lock().unwrap().1 {
            return Err(service_command_error("service already stopped"));
        }
        if self.stop_succeeds.load(Ordering::SeqCst) {
            self.state.lock().unwrap().1 = false;
        }
        Ok(())
    }
    async fn update(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        if self.park_update.load(Ordering::SeqCst) {
            self.parked.notify_one();
            self.release.notified().await;
        }
        self.updates.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().2 = "1.10.1".to_owned();
        Ok(())
    }
    async fn restart_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
        self.calls.lock().unwrap().push("restart_daemon");
        self.restarts.fetch_add(1, Ordering::SeqCst);
        let mut state = self.state.lock().unwrap();
        if !state.0 {
            return Err(service_command_error("service not installed"));
        }
        state.1 = true;
        Ok(())
    }
}

impl crate::actor_v2::service_router::ServiceEndpointFactory for FakeDaemon {
    fn endpoint(&self) -> EndpointHandle {
        Arc::new(NullEndpoint)
    }
}

fn probes(daemon: &FakeDaemon) -> usize {
    daemon
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|call| **call == "probe")
        .count()
}

/// Retries `adopt_if_ready` until the released helper has ended and is
/// reaped. The helper ends on its own task, so the first tries can race
/// it; each is a real mailbox round trip, not a timed guess. A refusal
/// probes nothing, and the message that reaps probes only its own leg, so
/// the whole loop costs exactly one probe (F6).
async fn adopt_once_reaped(client: &ServiceClient, daemon: &FakeDaemon) -> EndpointHandle {
    let before = probes(daemon);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match client.adopt_if_ready().await {
                Err(error) if error.kind == Some(CoreErrorKind::OperationConflict) => {
                    tokio::task::yield_now().await;
                }
                result => break result,
            }
        }
    })
    .await
    .expect("the released helper must end");
    assert_eq!(
        probes(daemon) - before,
        1,
        "reaping a finished helper adds no leg to the message that reaps it"
    );
    result.unwrap_or_else(|error| panic!("the reaped daemon is adoptable: {error}"))
}

/// N3 (T10 §1.11): an install that outlives its bound is still running,
/// and a probe that finds the daemon `Ready` meanwhile proves nothing
/// about it. Until the helper ends nothing may hand the endpoint out or
/// start another elevated command.
#[tokio::test]
async fn a_helper_that_outlives_its_bound_blocks_endpoint_handing_until_it_ends() {
    let daemon = FakeDaemon::new(false, false, "1.10.1");
    let client = ServiceClient::spawn_with_bounds(
        daemon.clone(),
        daemon.clone(),
        2,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();

    daemon.park_install.store(true, Ordering::SeqCst);
    let error = client.install().await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert!(error.message.contains("still running"), "{}", error.message);
    daemon.parked.notified().await;

    // The elevated install has already taken effect; its helper has not
    // returned. A probe now reads a stable `Ready`.
    *daemon.state.lock().unwrap() = (true, true, "1.10.1".to_owned());
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    let conflicts = [
        client.adopt_if_ready().await.err().map(|error| error.kind),
        client.ensure_ready().await.err().map(|error| error.kind),
        client
            .recover_endpoint()
            .await
            .err()
            .map(|error| error.kind),
        client.install().await.err().map(|error| error.kind),
        client.update().await.err().map(|error| error.kind),
        client.uninstall().await.err().map(|error| error.kind),
        client.start_daemon().await.err().map(|error| error.kind),
        client.stop_daemon().await.err().map(|error| error.kind),
    ];
    assert_eq!(
        conflicts,
        [Some(Some(CoreErrorKind::OperationConflict)); 8],
        "nothing is served past a helper that is still running"
    );
    assert_eq!(daemon.installs.load(Ordering::SeqCst), 0);
    assert_eq!(daemon.updates.load(Ordering::SeqCst), 0);
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);

    daemon.release.notify_one();
    let endpoint = adopt_once_reaped(&client, &daemon).await;
    assert_eq!(endpoint.host(), ExecutionHost::Service);
    assert_eq!(daemon.installs.load(Ordering::SeqCst), 1);
}

/// S21 (T10 §1.3): the startup auto-update runs before any facade exists,
/// so the actor alone knows it outlived its bound. Adopting the daemon on
/// the strength of a `Ready` probe would race the update that is still
/// replacing it.
#[tokio::test]
async fn a_startup_update_that_outlives_its_bound_blocks_adoption_until_it_ends() {
    let daemon = FakeDaemon::new(true, true, "1.4.5");
    daemon.park_update.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn_with_bounds(
        daemon.clone(),
        daemon.clone(),
        2,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();
    daemon.parked.notified().await;

    daemon.state.lock().unwrap().2 = "1.10.1".to_owned();
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    assert_eq!(
        client.adopt_if_ready().await.err().map(|error| error.kind),
        Some(Some(CoreErrorKind::OperationConflict))
    );

    daemon.release.notify_one();
    let endpoint = adopt_once_reaped(&client, &daemon).await;
    assert_eq!(endpoint.host(), ExecutionHost::Service);
    assert_eq!(daemon.updates.load(Ordering::SeqCst), 1);
    assert_eq!(client.status().phase, ServicePhase::Ready);
}

/// `CommandSettled` is the completion evidence (T10 §1.11): false while a
/// helper runs, true for a command that answered and left none, and the
/// query that reaps a finished helper probes once so the published phase
/// follows what the helper did.
#[tokio::test]
async fn command_settled_waits_out_a_running_helper_and_reaps_it_with_one_probe() {
    let daemon = FakeDaemon::new(false, false, "1.10.1");
    daemon.fail_start.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn_with_bounds(
        daemon.clone(),
        daemon.clone(),
        2,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();

    // A command that failed on its own is finished the moment it answers.
    assert!(client.start_daemon().await.is_err());
    assert!(client.command_settled().await.unwrap());

    daemon.park_install.store(true, Ordering::SeqCst);
    assert!(client.install().await.is_err());
    daemon.parked.notified().await;
    let before = probes(&daemon);
    assert!(!client.command_settled().await.unwrap());
    assert_eq!(probes(&daemon), before, "a running helper is not probed");

    daemon.release.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while !client.command_settled().await.unwrap() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the released helper must end");
    assert_eq!(probes(&daemon), before + 1);
    assert_eq!(client.status().phase, ServicePhase::Ready);
    assert!(client.command_settled().await.unwrap());
    assert_eq!(probes(&daemon), before + 1, "nothing left to reap");
}

#[tokio::test]
async fn ensure_ready_converges_from_not_installed_through_the_gate() {
    let daemon = FakeDaemon::new(false, false, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let endpoint = client.ensure_ready().await.unwrap();
    assert_eq!(endpoint.host(), ExecutionHost::Service);
    assert_eq!(daemon.installs.load(Ordering::SeqCst), 1);
    assert_eq!(client.status().phase, ServicePhase::Ready);
    assert!(matches!(
        client.status().compat,
        ServiceCompat::Compatible { .. }
    ));
}

#[tokio::test]
async fn an_outdated_daemon_is_upgraded_once_at_startup() {
    for version in ["1.4.5", "1.9.9", "1.10.0"] {
        let daemon = FakeDaemon::new(true, true, version);
        let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
            .await
            .unwrap();
        // pre_start already reconciled: one update, then the gate passes.
        assert_eq!(daemon.updates.load(Ordering::SeqCst), 1);
        assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    }
}

#[tokio::test]
async fn rc7_is_upgraded_before_serving_core_requests() {
    let daemon = FakeDaemon::new(true, true, "1.10.0-rc.7");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    assert_eq!(daemon.updates.load(Ordering::SeqCst), 1);
    assert_eq!(
        client.ensure_ready().await.unwrap().host(),
        ExecutionHost::Service
    );
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    assert_eq!(daemon.updates.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_version_gate_fails_closed() {
    let daemon = FakeDaemon::new(true, true, "1.10.0-rc.7");
    // A failed startup update leaves the incompatible daemon in place.
    struct StubbornDaemon(Arc<FakeDaemon>);
    #[async_trait::async_trait]
    impl ServiceProcessControl for StubbornDaemon {
        async fn probe(&self) -> Result<String, chimera_contracts::service::ServiceCommandError> {
            self.0.probe().await
        }
        async fn install(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.install().await
        }
        async fn uninstall(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.uninstall().await
        }
        async fn start_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.start_daemon().await
        }
        async fn stop_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.stop_daemon().await
        }
        async fn update(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            Err(service_command_error("service upgrade failed"))
        }
        async fn restart_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.restart_daemon().await
        }
    }

    impl crate::actor_v2::service_router::ServiceEndpointFactory for StubbornDaemon {
        fn endpoint(&self) -> EndpointHandle {
            self.0.endpoint()
        }
    }
    let client = {
        let process_control = Arc::new(StubbornDaemon(daemon));
        ServiceClient::spawn(process_control.clone(), process_control, 2)
    }
    .await
    .unwrap();
    let error = client
        .ensure_ready()
        .await
        .err()
        .expect("the gate must fail closed");
    assert!(!error.retryable, "fail-closed is not a retry loop");
    assert_eq!(client.status().phase, ServicePhase::Incompatible);
}

#[tokio::test]
async fn uninstall_refuses_while_the_daemon_owns_a_running_core() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.set_detail(Some(CoreStateDetail::Running { epoch: 1, pid: 42 }));
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client.uninstall().await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);

    // After the core is handed off, uninstall proceeds (stop + uninstall).
    daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
    client.uninstall().await.unwrap();
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 1);
    assert_eq!(client.status().phase, ServicePhase::NotInstalled);
}

#[tokio::test]
async fn uninstall_stops_the_daemon_before_removing_it() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    daemon.calls.lock().unwrap().clear();

    client.uninstall().await.unwrap();

    let calls = daemon.calls.lock().unwrap().clone();
    assert_eq!(&calls[..4], &["probe", "stop_daemon", "probe", "uninstall"]);
}

#[tokio::test]
async fn uninstall_refuses_when_the_daemon_will_not_stop() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.set_detail(Some(CoreStateDetail::Stopped { reason: None }));
    daemon.stop_succeeds.store(false, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    daemon.calls.lock().unwrap().clear();

    let error = client.uninstall().await.unwrap_err();

    assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn explicit_start_rearms_an_exhausted_daemon() {
    let daemon = FakeDaemon::new(true, false, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon, 0)
        .await
        .unwrap();
    assert!(client.recover_endpoint().await.is_err());
    assert_eq!(client.status().phase, ServicePhase::Exhausted);
    client.start_daemon().await.unwrap();
    assert_eq!(client.status().phase, ServicePhase::Ready);
    assert!(client.recover_endpoint().await.is_ok());
}

#[tokio::test]
async fn recovery_distinguishes_failed_start_from_an_unconfirmed_timeout() {
    for hangs in [false, true] {
        let daemon = FakeDaemon::new(true, false, "1.10.1");
        daemon.fail_start.store(!hangs, Ordering::SeqCst);
        daemon.hang_start.store(hangs, Ordering::SeqCst);
        let client = ServiceClient::spawn_with_bounds(
            daemon.clone(),
            daemon.clone(),
            3,
            std::time::Duration::from_millis(50),
        )
        .await
        .unwrap();
        let error = client.recover_endpoint().await.err().unwrap();
        assert_eq!(error.retryable, !hangs);
        assert_eq!(
            error.kind,
            Some(if hangs {
                CoreErrorKind::Internal
            } else {
                CoreErrorKind::BackendUnavailable
            })
        );
        assert_eq!(daemon.starts.load(Ordering::SeqCst), 1);
        assert_eq!(
            client.probe().await.unwrap().phase,
            ServicePhase::DaemonStopped
        );
    }
}

#[tokio::test]
async fn recovery_retries_a_live_daemon_without_a_status_response() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 3)
        .await
        .unwrap();
    daemon.probe_blind.store(true, Ordering::SeqCst);
    let error = client.recover_endpoint().await.err().unwrap();
    assert!(error.retryable);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 0);
    daemon.probe_blind.store(false, Ordering::SeqCst);
    assert!(client.recover_endpoint().await.is_ok());
}

#[tokio::test]
async fn endpoint_down_restarts_within_budget_then_latches_exhausted() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 1)
        .await
        .unwrap();

    // Daemon dies; the first report restarts it within budget.
    daemon.state.lock().unwrap().1 = false;
    let _ = client.recover_endpoint().await;
    let status = client.probe().await.unwrap();
    assert_eq!(status.phase, ServicePhase::Ready);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 1);

    // Dies again with the budget spent: honest exhaustion latch.
    daemon.state.lock().unwrap().1 = false;
    let _ = client.recover_endpoint().await; // attempt 2 > budget 1 -> Exhausted
    let mut status_rx = client.subscribe();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if status_rx.borrow_and_update().phase == ServicePhase::Exhausted {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("the spent budget must latch Exhausted");

    // EnsureReady is the explicit escape: it resets the budget and
    // converges again.
    client.ensure_ready().await.unwrap();
    assert_eq!(client.status().phase, ServicePhase::Ready);
}

/// M-10: a transitional detail is not a terminal state. The coarse
/// `CoreState` a real daemon publishes alongside `Starting` is
/// `Stopped`-shaped, so a guard reading it would uninstall out from under
/// a core that is coming up.
#[tokio::test]
async fn uninstall_refuses_on_transitional_detail() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.set_detail(Some(CoreStateDetail::Starting { epoch: 3 }));
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client.uninstall().await.unwrap_err();
    assert_eq!(error.kind, Some(CoreErrorKind::AlreadyRunning));
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
}

/// A running daemon that does not answer its probe tells us nothing, and
/// nothing is not permission.
#[tokio::test]
async fn uninstall_refuses_when_the_running_daemon_cannot_be_probed() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.probe_blind.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client.uninstall().await.unwrap_err();
    assert_eq!(
        error.kind, None,
        "the daemon might own a core; that it does is not something we know"
    );
    assert!(
        error.message.contains("cannot be determined"),
        "an unknown is reported as one, got: {}",
        error.message
    );
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
}

/// A daemon too old to publish `detail` is equally unknown. It is
/// `Incompatible` under the version gate anyway; the way out is
/// `StopDaemon` first, which makes the answer knowable.
#[tokio::test]
async fn uninstall_refuses_when_detail_is_missing() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.set_detail(None);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client.uninstall().await.unwrap_err();
    assert_eq!(
        error.kind, None,
        "the daemon might own a core; that it does is not something we know"
    );
    assert!(
        error.message.contains("cannot be determined"),
        "an unknown is reported as one, got: {}",
        error.message
    );
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
}

/// The escape hatch the refusals point at: a stopped daemon holds no core,
/// whatever its last published detail said.
#[tokio::test]
async fn uninstall_proceeds_when_the_daemon_is_stopped() {
    let daemon = FakeDaemon::new(true, false, "1.10.1");
    daemon.set_detail(Some(CoreStateDetail::Running { epoch: 1, pid: 42 }));
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    daemon.calls.lock().unwrap().clear();

    client.uninstall().await.unwrap();
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 1);
    assert_eq!(client.status().phase, ServicePhase::NotInstalled);
    // #5444: a stopped daemon is not stopped again on the way out.
    let calls = daemon.calls.lock().unwrap().clone();
    assert_eq!(&calls[..2], &["probe", "uninstall"]);
}

/// M-11: the latch is a latch. A probe that happens to find the daemon up
/// must not publish `Ready` for a host nobody re-armed, and must not let
/// the next `RecoverEndpoint` restart it.
#[tokio::test]
async fn exhausted_survives_probes_until_an_explicit_ensure_ready() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 0)
        .await
        .unwrap();

    daemon.state.lock().unwrap().1 = false;
    let _ = client.recover_endpoint().await;
    let mut status_rx = client.subscribe();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if status_rx.borrow_and_update().phase == ServicePhase::Exhausted {
                break;
            }
            status_rx.changed().await.unwrap();
        }
    })
    .await
    .expect("a spent budget must latch Exhausted");

    // The daemon comes back on its own; a probe still reports the latch.
    daemon.state.lock().unwrap().1 = true;
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Exhausted);

    // And a further down report does not pull it up behind our back.
    let starts = daemon.starts.load(Ordering::SeqCst);
    let _ = client.recover_endpoint().await;
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Exhausted);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);

    // The one escape.
    client.ensure_ready().await.unwrap();
    let status = client.status();
    assert_eq!(status.phase, ServicePhase::Ready);
    assert_eq!(status.restart_attempts, 0);
}

fn consent_refusal(command: chimera_contracts::service::ServiceCommand) -> ServiceCommandError {
    ServiceCommandError::AuthorizeServiceCommand {
        command,
        source: chimera_contracts::elevation::ElevationConsentError::AuthorizeElevation {
            operation: chimera_contracts::elevation::ElevationOperation::Service { command },
        },
    }
}

#[tokio::test]
async fn refused_service_command_keeps_the_probed_phase() {
    use chimera_contracts::service::ServiceCommand;

    for (installed, command, phase) in [
        (false, ServiceCommand::Install, ServicePhase::NotInstalled),
        (true, ServiceCommand::Start, ServicePhase::DaemonStopped),
    ] {
        let daemon = FakeDaemon::new(installed, false, "1.10.1");
        *daemon.command_failure.lock().unwrap() = Some(consent_refusal(command));
        let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
            .await
            .unwrap();
        let error = client
            .ensure_ready()
            .await
            .err()
            .expect("consent must be refused");
        assert!(!error.retryable);
        assert_eq!(client.status().phase, phase);
        assert_eq!(client.probe().await.unwrap().phase, phase);
    }

    let daemon = FakeDaemon::new(true, true, "1.10.1");
    *daemon.command_failure.lock().unwrap() = Some(consent_refusal(ServiceCommand::Stop));
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    let error = client.stop_daemon().await.unwrap_err();
    assert!(!error.retryable);
    assert_eq!(client.status().phase, ServicePhase::Ready);
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Ready);
    assert!(
        daemon.state.lock().unwrap().1,
        "refusal does not stop the daemon"
    );
}

#[tokio::test]
async fn service_consent_refusal_is_not_retryable() {
    use chimera_contracts::service::ServiceCommand;

    for (failure, retryable) in [
        (consent_refusal(ServiceCommand::Start), false),
        (
            ServiceCommandError::RunElevated {
                command: ServiceCommand::Start,
                source: std::io::Error::other("OS dialog cancelled"),
            },
            true,
        ),
        (
            ServiceCommandError::ServiceCommandExit {
                command: ServiceCommand::Start,
                exit_code: Some(1),
                signal: None,
            },
            true,
        ),
    ] {
        let daemon = FakeDaemon::new(true, false, "1.10.1");
        *daemon.command_failure.lock().unwrap() = Some(failure);
        let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 3)
            .await
            .unwrap();
        let error = client
            .recover_endpoint()
            .await
            .err()
            .expect("scripted command fails");
        assert_eq!(error.retryable, retryable);
        assert_eq!(client.status().phase, ServicePhase::DaemonStopped);
        assert_eq!(daemon.starts.load(Ordering::SeqCst), 1);
    }
}

/// Minor-A3: a convergence that fails must not leave a transitional phase
/// published. `Installing` forever reads as "still working".
#[tokio::test]
async fn a_failed_install_leaves_the_probed_phase_not_the_transitional_one() {
    let daemon = FakeDaemon::new(false, false, "1.10.1");
    daemon.fail_install.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client
        .ensure_ready()
        .await
        .err()
        .expect("the convergence must fail");
    assert!(
        error.message.contains("install refused"),
        "the adapter's own error is what returns, got: {}",
        error.message
    );
    assert_eq!(client.status().phase, ServicePhase::NotInstalled);
}

#[tokio::test]
async fn a_failed_start_leaves_the_probed_phase_not_the_transitional_one() {
    let daemon = FakeDaemon::new(true, false, "1.10.1");
    daemon.fail_start.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client
        .ensure_ready()
        .await
        .err()
        .expect("the convergence must fail");
    assert!(
        error.message.contains("start refused"),
        "the adapter's own error is what returns, got: {}",
        error.message
    );
    assert_eq!(client.status().phase, ServicePhase::DaemonStopped);
}

/// F5: pre-fix, `probe` had no error channel at all, so a probe failure
/// could only be represented as `ServiceStatus::Stopped` -- which the
/// uninstall guard's `(NotInstalled | Stopped, _) => None` arm read as
/// "no core held" and let straight through. An unreachable probe must
/// refuse exactly like the in-band "running but blind" case, with a
/// `kind: None` (nothing is known, not even that a core is running) and
/// a "cannot be determined" message.
#[tokio::test]
async fn uninstall_refuses_when_the_probe_fails_outright() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.probe_fail.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let error = client.uninstall().await.unwrap_err();
    assert_eq!(
        error.kind, None,
        "an unreachable probe proves nothing about a held core"
    );
    assert!(
        error.message.contains("cannot be determined"),
        "an unknown probe is reported as one, got: {}",
        error.message
    );
    assert_eq!(daemon.uninstalls.load(Ordering::SeqCst), 0);
}

/// F5: `converge`'s final match already treats any non-Ready/Incompatible
/// phase as a retryable `BackendUnavailable`. Pre-fix this could not even
/// be exercised -- there was no way for `probe` to fail -- and the
/// nearest equivalent (a hung probe) was defined to answer `Stopped`,
/// which would have driven `ensure_ready` into a doomed install/start
/// loop instead of reporting the true, retryable unknown.
#[tokio::test]
async fn ensure_ready_reports_a_retryable_error_when_the_probe_fails() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.probe_fail.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    // `EndpointHandle` is a trait object and has no `Debug`, so the error
    // has to be taken by match rather than `unwrap_err`.
    let Err(error) = client.ensure_ready().await else {
        panic!("an undetermined probe must not hand out an endpoint");
    };
    assert!(
        error.retryable,
        "an undetermined probe is a retry candidate, not a fatal gate"
    );
    assert_eq!(client.status().phase, ServicePhase::Unknown);
}

/// F5 x F7: the literal contract this fixes -- the trait doc used to say
/// a hung probe "answers Stopped-shaped", which is exactly the phase
/// `RecoverEndpoint` restarts from. An unreachable probe must not start
/// anything: it does not know there is nothing already running.
#[tokio::test]
async fn endpoint_down_does_not_start_when_the_probe_fails() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    daemon.probe_fail.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();

    let starts = daemon.starts.load(Ordering::SeqCst);
    let _ = client.recover_endpoint().await;
    let status = client.probe().await.unwrap();
    assert_eq!(status.phase, ServicePhase::Unknown);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);
}

#[tokio::test]
async fn session_end_probe_publishes_unknown_without_starting_the_daemon() {
    let daemon = FakeDaemon::new(true, false, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    assert_eq!(client.status().phase, ServicePhase::DaemonStopped);
    daemon.session_ending.store(true, Ordering::SeqCst);

    let error = client.recover_endpoint().await.err().unwrap();
    assert_eq!(error.kind, Some(CoreErrorKind::BackendUnavailable));
    assert_eq!(client.status().phase, ServicePhase::Unknown);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 0);
}

/// F6: every adapter call the actor makes is bounded, `install` included.
/// Pre-fix there was no timeout anywhere in this file, so a wedged
/// `install` would hang `ensure_ready` -- and with it the actor's single
/// serialized mailbox -- forever. `spawn_with_bounds` injects a short
/// bound so the assertion is reached by the actor's own timer, not the
/// test's wall clock.
#[tokio::test]
async fn ensure_ready_times_out_a_hung_install_and_the_actor_stays_responsive() {
    let daemon = FakeDaemon::new(false, false, "1.10.1");
    daemon.hang_install.store(true, Ordering::SeqCst);
    let client = ServiceClient::spawn_with_bounds(
        daemon.clone(),
        daemon.clone(),
        2,
        std::time::Duration::from_millis(50),
    )
    .await
    .unwrap();

    let outcome = tokio::time::timeout(std::time::Duration::from_secs(5), client.ensure_ready())
        .await
        .expect("a bounded adapter call must not hang the actor");
    let Err(error) = outcome else {
        panic!("a wedged install must not hand out a ready endpoint");
    };
    assert!(
        error.message.contains("timed out"),
        "got: {}",
        error.message
    );

    // The mailbox is still alive and answering after the bounded call
    // gave up.
    let status = client.probe().await.unwrap();
    assert_eq!(status.phase, ServicePhase::NotInstalled);
}

/// F7: `RecoverEndpoint` must only call `start_daemon` when the probed
/// phase is `DaemonStopped`. Pre-fix the guard excluded only `Ready`, so
/// an incompatible-but-running daemon fell through to a pointless
/// restart of a daemon that was never stopped.
#[tokio::test]
async fn endpoint_down_does_not_restart_an_incompatible_running_daemon() {
    let daemon = FakeDaemon::new(true, true, "1.4.5");
    // Stub the startup auto-update so the daemon stays `Incompatible`
    // instead of being upgraded by `pre_start`.
    struct StubbornDaemon(Arc<FakeDaemon>);
    #[async_trait::async_trait]
    impl ServiceProcessControl for StubbornDaemon {
        async fn probe(&self) -> Result<String, chimera_contracts::service::ServiceCommandError> {
            self.0.probe().await
        }
        async fn install(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.install().await
        }
        async fn uninstall(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.uninstall().await
        }
        async fn start_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.start_daemon().await
        }
        async fn stop_daemon(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.stop_daemon().await
        }
        async fn update(&self) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            Ok(()) // succeeds without changing anything
        }
        async fn restart_daemon(
            &self,
        ) -> Result<(), chimera_contracts::service::ServiceCommandError> {
            self.0.restart_daemon().await
        }
    }

    impl crate::actor_v2::service_router::ServiceEndpointFactory for StubbornDaemon {
        fn endpoint(&self) -> EndpointHandle {
            self.0.endpoint()
        }
    }
    let client = {
        let process_control = Arc::new(StubbornDaemon(daemon.clone()));
        ServiceClient::spawn(process_control.clone(), process_control, 2)
    }
    .await
    .unwrap();
    assert_eq!(client.status().phase, ServicePhase::Incompatible);

    let starts = daemon.starts.load(Ordering::SeqCst);
    let _ = client.recover_endpoint().await;
    let status = client.probe().await.unwrap();
    assert_eq!(status.phase, ServicePhase::Incompatible);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), starts);
}

/// The `ServiceClient` twin of `CoreClient`'s F3 fix: a caller bound has
/// to outlast the sum of every internal leg its message can honestly
/// take, not one fixed number shared by all of them. Pre-fix,
/// `call_timeout` was a single `command_timeout + 30s` used for every
/// message; `EnsureReady`'s own worst case is six sequential bounded legs
/// (`converge`'s three probes plus its conditional install and start,
/// plus `ensure_ready`'s recovery probe on failure -- see
/// `ensure_ready_budget`'s doc), and `Uninstall`'s is five. A timing test
/// cannot discriminate this at safe-for-CI durations: shrinking
/// `command_timeout` shrinks every leg together, so the fixed 30s slack
/// swamps the gap at any tiny bound and the pre-fix code would pass a
/// 50ms timing test regardless. The arithmetic itself is what has to be
/// tested, at both a tiny bound and the production one. This test does
/// not compile against the pre-fix module, which has no per-message
/// `*_budget` functions at all -- only a single `call_timeout` field.
#[test]
fn every_message_budget_outlasts_its_own_worst_case_leg_sum() {
    for command_timeout in [
        std::time::Duration::from_millis(50),
        DEFAULT_SERVICE_COMMAND_TIMEOUT,
    ] {
        let one_leg = command_timeout;
        let two_legs = command_timeout * 2;
        let five_legs = command_timeout * 5;
        let six_legs = command_timeout * 6;

        assert!(
            probe_budget(command_timeout) > one_leg,
            "probe_budget must outlast {one_leg:?} at command_timeout={command_timeout:?}"
        );
        assert!(
            command_and_probe_budget(command_timeout) > two_legs,
            "command_and_probe_budget must outlast {two_legs:?} at command_timeout={command_timeout:?}"
        );
        assert!(
            uninstall_budget(command_timeout) > five_legs,
            "uninstall_budget must outlast {five_legs:?} at command_timeout={command_timeout:?}"
        );
        assert!(
            ensure_ready_budget(command_timeout) > six_legs,
            "ensure_ready_budget must outlast {six_legs:?} at command_timeout={command_timeout:?}"
        );
    }

    // The exact numbers this fixes: at the production 100s
    // `command_timeout`, the old fixed `command_timeout + 30s` (130s)
    // caller bound was far short of `EnsureReady`'s ~600s worst case (and
    // `Uninstall`'s ~400s), so a caller received a non-retryable
    // `Internal` while the actor kept going with elevated install/stop/
    // uninstall side effects still in flight.
    let old_fixed_budget = DEFAULT_SERVICE_COMMAND_TIMEOUT + std::time::Duration::from_secs(30);
    let ensure_ready_worst_case = DEFAULT_SERVICE_COMMAND_TIMEOUT * 6;
    let uninstall_worst_case = DEFAULT_SERVICE_COMMAND_TIMEOUT * 5;
    assert!(ensure_ready_worst_case > old_fixed_budget);
    assert!(uninstall_worst_case > old_fixed_budget);
    assert!(ensure_ready_budget(DEFAULT_SERVICE_COMMAND_TIMEOUT) > ensure_ready_worst_case);
    assert!(uninstall_budget(DEFAULT_SERVICE_COMMAND_TIMEOUT) > uninstall_worst_case);
}
/// The health check probes only while it is on, publishes only what
/// changed, and never converges the daemon it finds stopped.
#[tokio::test]
async fn the_health_check_probes_while_on_and_publishes_only_changes() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    let mut status = client.subscribe();
    status.mark_unchanged();
    // Rounds start at 1, so round 1 is the one turning the check on arms.
    let check = |client: &ServiceClient| {
        client
            .actor
            .cast(ServiceActorMessage::HealthCheck { round: 1 })
            .unwrap();
    };

    let before = probes(&daemon);
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert_eq!(probes(&daemon), before, "off: a stray tick probes nothing");

    client.set_health_check(true);
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert_eq!(probes(&daemon), before + 1);
    assert!(
        !status.has_changed().unwrap(),
        "a steady daemon is not republished"
    );

    daemon.state.lock().unwrap().1 = false;
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert!(status.has_changed().unwrap());
    assert_eq!(
        status.borrow_and_update().phase,
        ServicePhase::DaemonStopped
    );
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 0);
    assert_eq!(daemon.installs.load(Ordering::SeqCst), 0);

    // A probe that keeps failing is not republished either.
    daemon.probe_fail.store(true, Ordering::SeqCst);
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert_eq!(status.borrow_and_update().phase, ServicePhase::Unknown);
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert!(!status.has_changed().unwrap());

    client.set_health_check(false);
    let before = probes(&daemon);
    check(&client);
    assert!(client.command_settled().await.unwrap());
    assert_eq!(probes(&daemon), before);
}

/// A new ready generation needs a settled not-ready in between: a probe
/// that failed proves no restart, a daemon seen stopped does.
#[tokio::test]
async fn only_a_settled_restart_makes_a_new_ready_generation() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    let first = client.status();
    assert_eq!(first.phase, ServicePhase::Ready);

    daemon.probe_fail.store(true, Ordering::SeqCst);
    assert_eq!(client.probe().await.unwrap().phase, ServicePhase::Unknown);
    daemon.probe_fail.store(false, Ordering::SeqCst);
    let after_failure = client.probe().await.unwrap();
    assert_eq!(after_failure.phase, ServicePhase::Ready);
    assert_eq!(after_failure.ready_generation, first.ready_generation);

    daemon.state.lock().unwrap().1 = false;
    assert!(!client.probe().await.unwrap().settled_ready);
    daemon.state.lock().unwrap().1 = true;
    let restarted = client.probe().await.unwrap();
    assert!(restarted.settled_ready);
    assert_eq!(restarted.ready_generation, first.ready_generation + 1);
}

/// Chimera's native restart is one serialized OS command, not two independent
/// stop/start commands that could leave a usable daemon in between.
#[tokio::test]
async fn native_restart_preserves_single_service_actor_command_boundary() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    assert_eq!(client.status().phase, ServicePhase::Ready);

    client.restart_daemon().await.unwrap();

    assert_eq!(client.status().phase, ServicePhase::Ready);
    assert_eq!(daemon.restarts.load(Ordering::SeqCst), 1);
    assert_eq!(daemon.starts.load(Ordering::SeqCst), 0);
    let calls = daemon.calls.lock().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| **call == "restart_daemon")
            .count(),
        1
    );
    assert!(!calls.contains(&"stop_daemon"));
}

/// Only the OS reporting the daemon not running releases a Service owner
/// that stopped answering.
#[tokio::test]
async fn daemon_not_running_answers_from_the_os_state() {
    let daemon = FakeDaemon::new(true, true, "1.10.1");
    let client = ServiceClient::spawn(daemon.clone(), daemon.clone(), 2)
        .await
        .unwrap();
    assert!(!client.daemon_not_running().await.unwrap());

    daemon.state.lock().unwrap().1 = false;
    assert!(client.daemon_not_running().await.unwrap());

    *daemon.state.lock().unwrap() = (false, false, String::new());
    assert!(client.daemon_not_running().await.unwrap());
}
