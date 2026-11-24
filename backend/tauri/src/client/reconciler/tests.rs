use std::{
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering as AtomicOrdering},
    },
    time::Duration,
};

use async_trait::async_trait;
use camino::Utf8PathBuf;
use chimera_config::application::ChimeraAppConfigPatch;
use chimera_config::clash::config::ClashConfig;
use chimera_core::state::{
    PersistentStateManagerSetup, ReplaceIfVersionResult, StateAckSubscriber, StateChange,
    StateDecision, SubscriberName,
};
use chimera_ipc::api::status::CoreState;
use tokio::sync::{Notify, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use super::*;
use crate::{config::chimera::ClashCore, core::RunType};

#[test]
fn service_status_tracks_ready_generation_without_changing_ipc_fields() {
    let mut current = ServiceHostStatus::probing();
    assert!(!current.settled_ready);
    assert_eq!(current.ready_generation, 0);

    let mut ready = current.clone();
    ready.phase = ServicePhase::Ready;
    current = ready.settle_after(&current);
    assert!(current.settled_ready);
    assert_eq!(current.ready_generation, 1);

    let mut unknown = current.clone();
    unknown.phase = ServicePhase::Unknown;
    current = unknown.settle_after(&current);
    assert!(
        current.settled_ready,
        "failed probe cannot invent a stopped daemon"
    );
    assert_eq!(current.ready_generation, 1);

    let mut stopped = current.clone();
    stopped.phase = ServicePhase::DaemonStopped;
    current = stopped.settle_after(&current);
    assert!(!current.settled_ready);
    assert_eq!(current.ready_generation, 1);

    let mut restarted = current.clone();
    restarted.phase = ServicePhase::Ready;
    current = restarted.settle_after(&current);
    assert!(current.settled_ready);
    assert_eq!(current.ready_generation, 2);

    let json = serde_json::to_value(&current).unwrap();
    assert!(!json.as_object().unwrap().contains_key("settled_ready"));
    assert!(!json.as_object().unwrap().contains_key("ready_generation"));
    assert_eq!(json["phase"], "ready");
}

#[test]
fn startup_recovery_evidence_detects_same_generation_probe_recovery() {
    let mut previous = ServiceHostStatus::probing();
    previous = previous.observe_probe(
        chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Running,
            server: Some(chimera_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: std::borrow::Cow::Borrowed("1.11.1"),
                core_infos: chimera_ipc::api::status::CoreInfos {
                    instance_id: Some("service-instance".into()),
                    r#type: None,
                    state: CoreState::Running,
                    state_changed_at: 1,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail: Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
                        reason: None,
                    }),
                },
                runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(Default::default()),
                    service_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                },
            }),
        },
        true,
        None,
    );
    let unavailable = previous.on_probe_failure();
    let recovered = unavailable.observe_probe(
        chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Running,
            server: previous.server.clone(),
        },
        true,
        None,
    );

    assert_eq!(unavailable.ready_generation, recovered.ready_generation);
    assert!(startup_recovery_evidence_changed(
        &unavailable,
        &recovered,
        true
    ));
    assert!(startup_recovery_evidence_changed(
        &unavailable,
        &recovered,
        false,
    ));
}

#[test]
fn startup_recovery_evidence_detects_readiness_and_generation_changes() {
    let mut previous = ServiceHostStatus::probing();
    previous.phase = ServicePhase::Ready;
    previous.settled_ready = true;
    previous.ready_generation = 1;

    let mut current = previous.clone();
    current.ready_generation = 2;

    assert_eq!(
        crate::client::application_workflow::startup::ServiceEvidence::from_status(&previous),
        crate::client::application_workflow::startup::ServiceEvidence::from_status(&current),
        "a generation advance alone must be enough to request recovery"
    );
    assert!(startup_recovery_evidence_changed(
        &previous, &current, false
    ));

    let mut unavailable = previous.clone();
    unavailable.phase = ServicePhase::DaemonStopped;
    unavailable.settled_ready = false;
    assert!(startup_recovery_evidence_changed(
        &previous,
        &unavailable,
        false,
    ));
}

#[test]
fn unchanged_service_observations_do_not_flood_core_lifecycle_mailbox() {
    use chimera_ipc::types::ServiceStatus;

    let mut not_installed = ServiceHostStatus::probing();
    not_installed.phase = ServicePhase::NotInstalled;
    not_installed.status = ServiceStatus::NotInstalled;

    for _ in 0..1000 {
        assert!(!service_followup_evidence_changed(
            &not_installed,
            &not_installed.clone(),
        ));
    }

    // The ServiceActor's status watch is allowed to publish the same
    // probe repeatedly; no CoreLifecycle follow-up is owed without new
    // evidence. This leaves Checked RPC ahead of redundant health polls.
    let mut stopped = not_installed.clone();
    stopped.status = ServiceStatus::Stopped;
    stopped.phase = ServicePhase::DaemonStopped;
    assert!(service_followup_evidence_changed(&not_installed, &stopped));
    assert!(!service_followup_evidence_changed(&stopped, &stopped));
}

#[test]
fn service_followup_retains_generation_owner_core_and_policy_transitions() {
    let mut current = ServiceHostStatus::probing();
    current.phase = ServicePhase::Ready;
    current.settled_ready = true;
    current.ready_generation = 4;

    let mut next = current.clone();
    next.ready_generation += 1;
    assert!(service_followup_evidence_changed(&current, &next));

    next = current.clone();
    next.runtime_owned = !current.runtime_owned;
    assert!(service_followup_evidence_changed(&current, &next));

    next = current.clone();
    next.restart_attempts += 1;
    assert!(service_followup_evidence_changed(&current, &next));

    next = current.clone();
    next.phase = ServicePhase::Restarting;
    assert!(service_followup_evidence_changed(&current, &next));

    // A Core status change can matter even if the Service's coarse
    // readiness phase and generation remain unchanged.
    use chimera_ipc::api::status::{CoreInfos, CoreStateDetail, RuntimeInfos, StatusResBody};
    use chimera_ipc::types::ServiceStatus;
    current.status = ServiceStatus::Running;
    current.server = Some(StatusResBody {
        log_query_version: None,
        version: std::borrow::Cow::Borrowed("test"),
        core_infos: CoreInfos {
            instance_id: None,
            r#type: None,
            state: chimera_ipc::api::status::CoreState::Stopped(None),
            state_changed_at: 0,
            config_path: None,
            controller: None,
            health: None,
            revision: None,
            detail: Some(CoreStateDetail::Stopped { reason: None }),
        },
        runtime_infos: RuntimeInfos {
            service_data_dir: std::borrow::Cow::Owned(Default::default()),
            service_config_dir: std::borrow::Cow::Owned(Default::default()),
            nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
            nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
        },
    });
    next = current.clone();
    next.server.as_mut().unwrap().core_infos.detail =
        Some(CoreStateDetail::Running { epoch: 1, pid: 42 });
    assert!(service_followup_evidence_changed(&current, &next));
}

#[tokio::test]
async fn ref_service_actor_generation_drives_startup_recovery_not_legacy_watch() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut actor_status = ServiceHostStatus::probing();
    actor_status.phase = ServicePhase::Ready;
    actor_status.settled_ready = true;
    actor_status.ready_generation = 5;
    let service = Arc::new(ActorEvidenceService {
        actor_status: Arc::new(Mutex::new(actor_status)),
        probe_failed: AtomicBool::new(false),
        probes: AtomicUsize::new(0),
        events: events.clone(),
    });
    let core = Arc::new(ConfirmedStoppedCore {
        events,
        run_type: RunType::Normal,
    });
    let client = spawn_startup_client(core, service.clone()).await;
    let ClientInner::Actor {
        service_failed_generation,
        service_status,
        ..
    } = client.0.as_ref()
    else {
        panic!("startup fixture must use the lifecycle actor")
    };
    assert_eq!(
        service_status.borrow().ready_generation,
        0,
        "the legacy watch deliberately has a different generation"
    );
    service_failed_generation.store(5, Ordering::Release);
    assert!(
        !client.startup_recovery_needed(),
        "the same ref ServiceActor generation must not retry an already failed startup"
    );

    service.actor_status.lock().unwrap().ready_generation = 6;
    assert!(
        client.startup_recovery_needed(),
        "a new Ready generation in the ref Actor permits recovery"
    );

    service.actor_status.lock().unwrap().phase = ServicePhase::Unknown;
    assert!(
        !client.startup_recovery_needed(),
        "a new but unproven generation cannot authorize recovery"
    );
}

#[tokio::test]
async fn startup_service_evidence_keeps_ref_actor_generation_on_probe_error() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut actor_status = ServiceHostStatus::probing();
    actor_status.phase = ServicePhase::DaemonStopped;
    actor_status.ready_generation = 13;
    let service = Arc::new(ActorEvidenceService {
        actor_status: Arc::new(Mutex::new(actor_status.clone())),
        probe_failed: AtomicBool::new(false),
        probes: AtomicUsize::new(0),
        events: events.clone(),
    });
    let (legacy_sender, legacy_receiver) =
        tokio::sync::watch::channel(ServiceHostStatus::probing());
    legacy_sender.send_modify(|status| {
        status.ready_generation = 999;
        status.phase = ServicePhase::Ready;
    });

    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ConfirmedStoppedCore {
            events,
            run_type: RunType::Normal,
        }),
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service.clone(),
    )
    .with_service_status(legacy_receiver);

    let (evidence, generation) = workflow.service_evidence().await;
    assert_eq!(generation, 13);
    assert_eq!(
        evidence,
        crate::client::application_workflow::startup::ServiceEvidence::from_status(&actor_status),
        "startup ownership must not be derived from the contradictory legacy watch"
    );

    service.probe_failed.store(true, Ordering::Release);
    let (unreadable, failed_generation) = workflow.service_evidence().await;
    assert_eq!(failed_generation, 13);
    assert!(matches!(
        unreadable,
        crate::client::application_workflow::startup::ServiceEvidence::Unreadable {
            phase: ServicePhase::Unknown
        }
    ));
}

#[tokio::test]
async fn ref_service_actor_does_not_publish_duplicate_legacy_status_or_probe() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut status = ServiceHostStatus::probing();
    status.phase = ServicePhase::Ready;
    status.settled_ready = true;
    status.ready_generation = 23;
    let service = Arc::new(ActorEvidenceService {
        actor_status: Arc::new(Mutex::new(status.clone())),
        probe_failed: AtomicBool::new(false),
        probes: AtomicUsize::new(0),
        events: events.clone(),
    });
    let client = spawn_startup_client(
        Arc::new(ConfirmedStoppedCore {
            events,
            run_type: RunType::Normal,
        }),
        service.clone(),
    )
    .await;
    let ClientInner::Actor { service_status, .. } = client.0.as_ref() else {
        panic!("fixture must own the lifecycle actor");
    };
    assert_eq!(service_status.borrow().phase, ServicePhase::Probing);

    client.observe_service_status(chimera_ipc::types::StatusInfo {
        name: "chimera-service".into(),
        version: "1.10.1".into(),
        status: chimera_ipc::types::ServiceStatus::Stopped,
        server: None,
    });
    let observed = client.probe_service().await.expect("read the ref actor");
    assert_eq!(observed.phase, ServicePhase::Ready);
    assert_eq!(observed.ready_generation, 23);
    assert_eq!(
        service.probes.load(AtomicOrdering::Relaxed),
        0,
        "a CoreLifecycle read must not trigger a redundant Service probe"
    );
    assert_eq!(
        service_status.borrow().phase,
        ServicePhase::Probing,
        "a stale Service IPC observation must not publish a second phase"
    );

    client.observe_service_probe_failure();
    let observed = client
        .probe_service()
        .await
        .expect("read after IPC failure");
    assert_eq!(observed.phase, ServicePhase::Ready);
    assert_eq!(observed.ready_generation, 23);
    assert_eq!(service_status.borrow().phase, ServicePhase::Probing);
    assert_eq!(service.probes.load(AtomicOrdering::Relaxed), 0);
}

#[tokio::test]
async fn application_workflow_connection_keeps_owner_and_runs_startup() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(ConfirmedStoppedCore {
            events: events.clone(),
            run_type: RunType::Normal,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;
    assert!(client.cached_startup_report().await.unwrap().is_none());

    let _connected = crate::client::application_workflow::ApplicationWorkflowClient::spawn(
        client.clone(),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        Arc::new(tokio::sync::Mutex::new(None)),
        None,
    )
    .await
    .expect("the production ApplicationWorkflow connection should succeed");

    assert!(client.cached_startup_report().await.unwrap().is_none());

    let completed = client
        .startup_reconcile()
        .await
        .expect("startup should execute through the connected application workflow");
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    assert!(
        events.lock().unwrap().contains(&"rebuild"),
        "startup must reach the core adapter through the connected workflow"
    );
    let cached = client
        .cached_startup_report()
        .await
        .expect("the production actor should answer the report query")
        .expect("the connected ApplicationWorkflow should own the startup report");
    assert_eq!(cached.operation_id, completed.report.operation_id);
    assert_eq!(cached.outcome, completed.report.outcome);
}

#[tokio::test]
async fn application_workflow_connection_waits_for_active_operation_and_rejects_duplicates() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(RecordingCore {
        events: events.clone(),
        stop_started: Arc::new(Notify::new()),
        release_stop: Arc::new(Notify::new()),
        recovery_notify: Arc::new(Notify::new()),
        run_type: RunType::Normal,
    });
    let client = spawn_startup_client(
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await });
    core.stop_started.notified().await;

    let connect_client = client.clone();
    let (registered, pending) = oneshot::channel();
    let connect = tokio::spawn(async move {
        crate::client::application_workflow::ApplicationWorkflowClient::spawn_with_registration(
            connect_client,
            Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            Arc::new(tokio::sync::Mutex::new(None)),
            None,
            registered,
        )
        .await
    });
    pending
        .await
        .expect("the actor should acknowledge that it registered the first connection");

    let duplicate = crate::client::application_workflow::ApplicationWorkflowClient::spawn(
        client.clone(),
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        Arc::new(tokio::sync::Mutex::new(None)),
        None,
    )
    .await;
    assert!(
        duplicate.is_err(),
        "a second mutation workflow must be rejected while the first connection is pending"
    );

    core.release_stop.notify_one();
    stop.await
        .expect("StopCore task should complete")
        .expect("StopCore should succeed");
    connect
        .await
        .expect("connection task should complete")
        .expect("the pending mutation workflow should attach after completion");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end"]
    );
}

#[tokio::test]
async fn cancelled_pending_application_workflow_does_not_poison_connection() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(RecordingCore {
        events: events.clone(),
        stop_started: Arc::new(Notify::new()),
        release_stop: Arc::new(Notify::new()),
        recovery_notify: Arc::new(Notify::new()),
        run_type: RunType::Normal,
    });
    let client = spawn_startup_client(
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await });
    core.stop_started.notified().await;

    let connect_client = client.clone();
    let (registered, pending) = oneshot::channel();
    let connect = tokio::spawn(async move {
        crate::client::application_workflow::ApplicationWorkflowClient::spawn_with_registration(
            connect_client,
            Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            Arc::new(tokio::sync::Mutex::new(None)),
            None,
            registered,
        )
        .await
    });
    pending
        .await
        .expect("the actor should confirm that it stored the pending connection");
    connect.abort();
    assert!(
        connect.await.is_err(),
        "cancel the waiting connection caller"
    );

    core.release_stop.notify_one();
    stop.await
        .expect("StopCore task should complete")
        .expect("StopCore should succeed");

    crate::client::application_workflow::ApplicationWorkflowClient::spawn(
        client,
        Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        Arc::new(tokio::sync::Mutex::new(None)),
        None,
    )
    .await
    .expect("a cancelled pending connection must leave the actor connectable");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end"]
    );
}

#[tokio::test]
async fn explicit_reconcile_keeps_committed_profiles_active_after_deferred_retry_recovery() {
    let directory = tempfile::tempdir().expect("temporary Profiles directory");
    let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
        .expect("UTF-8 temporary Profiles path");
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .expect("Profiles state manager");
    let before = manager.snapshot_handle().load();
    let version = before.version;
    let mut candidate = before.state.clone();
    candidate.valid.push("production-actor-tcc-test".into());
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
        manager.snapshot_handle(),
    )
    .await;
    let mutations = crate::state::mutation::MutationCoordinator::pending();
    mutations
        .connect(client.clone())
        .await
        .expect("production ApplicationWorkflow should attach to Core Lifecycle");
    let operation_id = chimera_core_manager::OperationId::generate();
    let participant = mutations
        .participant(
            operation_id.clone(),
            crate::client::application_workflow::impact::MutationHints::default(),
            crate::client::application_workflow::policy::CommandClass::Save,
        )
        .expect("connected mutation coordinator should create a participant");
    let decision_capture = Arc::new(Mutex::new(None));

    let result = manager
        .replace_if_version_with_participant(
            version,
            candidate.clone(),
            {
                let decision_capture = decision_capture.clone();
                move |decision| {
                    *decision_capture.lock().unwrap() = Some(decision.clone());
                    participant(decision)
                }
            },
            || async { Ok(()) },
            || async { Ok(()) },
        )
        .await
        .expect("Profile source transaction");
    assert!(matches!(result, ReplaceIfVersionResult::Replaced));
    let committed = manager.snapshot_handle().load();
    assert_eq!(committed.state.valid, candidate.valid);
    assert_eq!(core.validations.load(AtomicOrdering::SeqCst), 1);

    let (receipt, degradations) = mutations
        .finish(operation_id, "profiles", *committed.version.as_ref())
        .await;
    assert_eq!(
        receipt.runtime,
        crate::client::runtime::RuntimeCommitStatus::SavedInactive
    );
    assert!(degradations.is_empty());

    let decision = decision_capture
        .lock()
        .unwrap()
        .clone()
        .expect("source transaction must expose its decision to the participant");
    let (unresolved_operation, digest) = client
        .seed_unresolved_attempt_with_deferred_retry_for_tests(decision.clone())
        .await
        .expect("seed the actor's unresolved source decision and due retry");
    let (attempt, deferred_retry, _) = client
        .application_recovery_state_for_tests()
        .await
        .expect("read the actor's recovery state");
    assert_eq!(attempt, Some((unresolved_operation, true)));
    let (deferred_digest, next_attempt) =
        deferred_retry.expect("rejected retry must be rescheduled");
    assert_eq!(deferred_digest, digest);
    assert!(
        next_attempt > tokio::time::Instant::now(),
        "a rejected retry must retain a future deadline without spending an attempt"
    );
    client
        .reconcile()
        .await
        .expect("the normal reconcile API must recover a Profile source attempt");
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .expect("read the recovered actor state")
            .0,
        None,
        "the explicit Profile recovery must release the live attempt"
    );
    assert_eq!(
        core.active_profiles
            .lock()
            .unwrap()
            .as_ref()
            .map(|profiles| profiles.valid.clone()),
        Some(candidate.valid.clone()),
        "recovery must leave the reconciled committed Profiles active"
    );
    assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);

    let committed_target = client
        .seed_unresolved_committed_target_for_tests(decision)
        .await
        .expect("seed a committed deferred Profile retry with an uncertain result");
    let (attempt, _, retry_budget) = client
        .application_recovery_state_for_tests()
        .await
        .expect("read the committed target recovery state");
    assert_eq!(attempt, Some((committed_target, true)));
    assert_eq!(retry_budget, Some((2, 1)));
    core.fail_next_reconcile
        .store(true, AtomicOrdering::Release);
    assert!(
        client.reconcile().await.is_err(),
        "a failed current-config reconcile must remain a failed recovery"
    );
    let (attempt, _, retry_budget) = client
        .application_recovery_state_for_tests()
        .await
        .expect("read the still-isolated committed target state");
    assert_eq!(attempt, Some((committed_target.clone(), true)));
    assert_eq!(retry_budget, Some((2, 1)));
    assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);
    client
        .reconcile()
        .await
        .expect("reconcile should recover the current committed Profile target");
    assert_eq!(
        core.active_profiles
            .lock()
            .unwrap()
            .as_ref()
            .map(|profiles| profiles.valid.clone()),
        Some(candidate.valid),
        "committed-target recovery must not restore its old runtime baseline"
    );
    assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 0);
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .expect("read the settled committed target state"),
        (None, None, None),
        "successful reconciliation must settle the committed retry target"
    );
    client.shutdown().await.unwrap();
}

#[test]
fn exhausted_service_probe_stays_latched_not_ready() {
    let mut previous = ServiceHostStatus::probing();
    previous.phase = ServicePhase::Ready;
    previous = previous.settle_after(&ServiceHostStatus::probing());

    let mut exhausted = previous.clone();
    exhausted.phase = ServicePhase::Exhausted;
    exhausted = exhausted.settle_after(&previous);
    assert!(!exhausted.settled_ready);
    assert_eq!(exhausted.ready_generation, 1);

    let mut failed_probe = ServiceHostStatus::probe_failed(&exhausted);
    failed_probe.phase = ServicePhase::Exhausted;
    failed_probe = failed_probe.settle_after(&exhausted);
    assert!(!failed_probe.settled_ready);
    assert_eq!(failed_probe.ready_generation, 1);
}

struct RecordingCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    stop_started: Arc<Notify>,
    release_stop: Arc<Notify>,
    recovery_notify: Arc<Notify>,
    run_type: RunType,
}

/// Keep one source apply in flight while the Actor mailbox fills. The
/// second reconcile must eventually apply the newest committed source.
struct PausedFirstReconcileCore {
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    calls: AtomicUsize,
}

#[async_trait]
impl CoreLifecyclePort for PausedFirstReconcileCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        if self.calls.fetch_add(1, AtomicOrdering::AcqRel) == 0 {
            self.entered.notify_one();
            self.resume.notified().await;
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

struct ConfirmedStoppedCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    run_type: RunType,
}

struct ProfileTccCore {
    validations: AtomicUsize,
    attempts: AtomicUsize,
    stops: AtomicUsize,
    fail_next_reconcile: AtomicBool,
    active_profiles: Mutex<Option<chimera_config::profile::Profiles>>,
}

/// A test endpoint with a stable accepted runtime receipt. Unlike
/// lifecycle-only fakes it can prove the exact committed target.
struct ReceiptProofCore {
    active: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
    publish_wrong_target: bool,
    change_instance_between_reads: bool,
    status_reads: AtomicUsize,
    submissions: AtomicUsize,
    stops: AtomicUsize,
    service_to_retire: Option<Arc<AtomicBool>>,
    retire_calls: AtomicUsize,
}

/// The Core accepted a committed revision, but publishing the derived
/// Applied snapshot failed after the product had been promoted.
struct PendingProductCore {
    lifecycle: Arc<crate::client::runtime::RuntimeLifecycle>,
    confirmed: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
    submissions: AtomicUsize,
    publications: AtomicUsize,
}

#[async_trait]
impl CoreLifecyclePort for PendingProductCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("this fixture needs a typed committed target")
    }

    async fn reconcile_profiles(
        &self,
        _: ClashConfig,
        _: ClashCore,
        _: RunType,
        _: Arc<chimera_config::profile::Profiles>,
        _: chimera_config::application::ChimeraAppConfig,
        _: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.submissions.fetch_add(1, AtomicOrdering::AcqRel);
        let product = Arc::new(crate::client::runtime::RuntimeSnapshot::new(
            self.lifecycle.allocate_revision().unwrap(),
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec(),
            serde_yaml::Mapping::new(),
        ));
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.revision = product.revision;
        receipt.config_digest = chimera_core_manager::payload_digest(product.product_bytes());
        receipt.target = target;
        let receipt = Arc::new(receipt);
        self.lifecycle.publish_confirmed(receipt.clone());
        self.lifecycle.publish_promoted(product);
        *self.confirmed.lock().unwrap() = Some(receipt);
        anyhow::bail!("injected derived RuntimeProduct publication failure")
    }

    fn pending_runtime_product_target(&self) -> Option<String> {
        self.lifecycle.pending_confirmed_product()?.0.target.clone()
    }

    async fn republish_pending_runtime_product(&self, target: &str) -> anyhow::Result<bool> {
        let Some(receipt) = self.confirmed.lock().unwrap().clone() else {
            return Ok(false);
        };
        if receipt.target.as_deref() != Some(target) {
            return Ok(false);
        }
        let published = self
            .lifecycle
            .republish_confirmed_product(&receipt)?
            .is_some();
        if published {
            self.publications.fetch_add(1, AtomicOrdering::AcqRel);
        }
        Ok(published)
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        Ok(self.confirmed.lock().unwrap().clone())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let active = self.confirmed.lock().unwrap().is_some();
        Ok(CoreStatusSnapshot {
            instance_id: active.then(|| "pending-product-core".to_owned()),
            state: if active {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: 1,
            run_type: RunType::Normal,
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.lifecycle.invalidate_confirmed();
        *self.confirmed.lock().unwrap() = None;
        Ok(())
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ReceiptProofCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("this fixture requires typed committed Profiles")
    }

    async fn reconcile_prepared_outcome(
        &self,
        prepared: ports::PreparedRuntime,
        spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<ports::CheckedApplyOutcome> {
        anyhow::ensure!(
            prepared.intent.config_text.as_bytes() == prepared.snapshot.product_bytes(),
            "prepared source bytes were not submitted verbatim"
        );
        anyhow::ensure!(
            spec.kind == chimera_core_manager::CoreKind::Mihomo,
            "unexpected CoreSpec"
        );
        self.submissions.fetch_add(1, AtomicOrdering::AcqRel);
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.target = prepared.target;
        receipt.revision = prepared.snapshot.revision;
        receipt.config_digest = prepared.intent.digest.clone();
        *self.active.lock().unwrap() = Some(Arc::new(receipt));
        Ok(ports::CheckedApplyOutcome {
            replaced: Some(true),
        })
    }

    async fn reconcile_profiles(
        &self,
        _: ClashConfig,
        _: ClashCore,
        _: RunType,
        _: Arc<chimera_config::profile::Profiles>,
        _: chimera_config::application::ChimeraAppConfig,
        _: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.submissions.fetch_add(1, AtomicOrdering::AcqRel);
        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.target = if self.publish_wrong_target {
            Some("an-older-committed-target".into())
        } else {
            target
        };
        *self.active.lock().unwrap() = Some(Arc::new(receipt));
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.stops.fetch_add(1, AtomicOrdering::AcqRel);
        *self.active.lock().unwrap() = None;
        Ok(())
    }

    async fn retire_service_core(
        &self,
        _expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        let service = self
            .service_to_retire
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no owned Service Core in this test endpoint"))?;
        self.retire_calls.fetch_add(1, AtomicOrdering::AcqRel);
        service.store(false, AtomicOrdering::Release);
        Ok(())
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let running = self.active.lock().unwrap().is_some();
        Ok(CoreStatusSnapshot {
            instance_id: running.then(|| {
                if self.change_instance_between_reads {
                    format!(
                        "receipt-proof-core-{}",
                        self.status_reads.fetch_add(1, AtomicOrdering::AcqRel)
                    )
                } else {
                    "receipt-proof-core".to_owned()
                }
            }),
            state: if running {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: 1,
            run_type: RunType::Normal,
        })
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        Ok(self.active.lock().unwrap().clone())
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        // Startup requires an authoritative stopped-state fixture, not
        // merely a Stopped status projection with unknown lower outcome.
        Ok(if self.active.lock().unwrap().is_some() {
            crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied
        } else {
            crate::client::application_workflow::mutation::KnownRuntimeState::Stopped
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

struct AckOnlyProfileChange;

#[async_trait]
impl StateAckSubscriber<chimera_config::profile::Profiles> for AckOnlyProfileChange {
    fn name(&self) -> SubscriberName<'_> {
        "pending-profile-source-decision-test".into()
    }

    async fn on_prepare(
        &self,
        _change: StateChange<chimera_config::profile::Profiles>,
    ) -> chimera_core::state::Ack {
        chimera_core::state::Ack::Ok
    }
}

struct CoreUpdateRecordingCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    state: CoreState,
    state_changed_at: i64,
    active_kind: Option<chimera_core_manager::CoreKind>,
    receipt_error: bool,
}

struct PanicOnStopCore {
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct LowerUncertainCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    uncertain: AtomicBool,
}

struct BindingCore {
    running: AtomicBool,
}

#[derive(Default)]
struct RecordingRuntimeBindings {
    ports: Mutex<Vec<Option<u16>>>,
}

impl crate::client::effects::ports::CommitNotifications for RecordingRuntimeBindings {
    fn application_committed(
        &self,
        _: crate::client::effects::plan::ApplicationEffectFields,
        _: Vec<crate::client::effects::plan::EffectKind>,
    ) {
    }

    fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {}

    fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {}

    fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
        _: bool,
    ) {
        self.ports
            .lock()
            .unwrap()
            .push(ports.map(|binding| binding.mixed_port));
    }

    fn publish_full(&self, _: Option<chimera_config::runtime::executor::ResolvedPortBindings>) {}
}

struct RecordingInstaller {
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct RecordingService {
    events: Arc<Mutex<Vec<&'static str>>>,
}

/// Hold the Service lease outside the Actor while two source commits
/// arrive through the real RuntimeReconcilerClient::source_committed entry.
struct PausedStartupRecoveryLeaseService {
    inner: RecordingService,
    gate: Arc<tokio::sync::Mutex<()>>,
}

#[async_trait]
impl ServiceLifecyclePort for PausedStartupRecoveryLeaseService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        let _guard = self.gate.lock().await;
        self.inner.begin_transition().await
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        let Ok(_guard) = self.gate.try_lock() else {
            anyhow::bail!("injected startup recovery lease still held");
        };
        self.inner.try_begin_transition().await
    }
}

/// Explicitly contradictory Actor/legacy generation fixture. This
/// catches accidental mixing of two independent readiness counters.
struct ActorEvidenceService {
    actor_status: Arc<Mutex<ServiceHostStatus>>,
    probe_failed: AtomicBool,
    probes: AtomicUsize,
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct ToggleStartupServiceProbe {
    events: Arc<Mutex<Vec<&'static str>>>,
    ready: Arc<AtomicBool>,
    core_running: Arc<AtomicBool>,
}

/// A Service daemon with an owned but *stopped* Core. Its transition
/// lease can reject one stop so tests exercise ref's pending_release debt.
struct ReleaseOwedService {
    inner: ToggleStartupServiceProbe,
    runtime_owned: bool,
    attempts: Arc<AtomicUsize>,
    fail_next: Arc<AtomicBool>,
}

struct ReleaseOwedLease {
    delegate: RecordingServiceTransition,
    ready: Arc<AtomicBool>,
    attempts: Arc<AtomicUsize>,
    fail_next: Arc<AtomicBool>,
}

#[async_trait]
impl ServiceTransitionLease for ReleaseOwedLease {
    async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.delegate.install_daemon().await
    }
    async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        self.delegate.uninstall_daemon().await
    }
    async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.delegate.update_daemon().await
    }
    async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.delegate.start_daemon().await
    }
    async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.delegate.restart_daemon().await
    }
    async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.attempts.fetch_add(1, AtomicOrdering::AcqRel);
        if self.fail_next.swap(false, AtomicOrdering::AcqRel) {
            anyhow::bail!("injected Service daemon stop failure");
        }
        self.ready.store(false, AtomicOrdering::Release);
        self.delegate.stop_daemon().await
    }
    async fn confirm_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        self.delegate.confirm_ready(timeout).await
    }
    async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.ready.load(AtomicOrdering::Acquire),
            "daemon was not confirmed stopped"
        );
        self.delegate.confirm_stopped().await
    }
}

#[async_trait]
impl ServiceLifecyclePort for ReleaseOwedService {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }
    fn runtime_owned(&self, _: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.runtime_owned
    }
    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.try_begin_transition().await
    }
    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(ReleaseOwedLease {
            delegate: RecordingServiceTransition {
                events: self.inner.events.clone(),
            },
            ready: self.inner.ready.clone(),
            attempts: self.attempts.clone(),
            fail_next: self.fail_next.clone(),
        }))
    }
}

/// Inject one transient lease refusal on the post-startup S2 handoff.
struct LeaseBlockedToggleService {
    inner: ToggleStartupServiceProbe,
    block_next: AtomicBool,
}

struct TransitionUnavailableService {
    inner: RecordingService,
}

struct UnreadableService {
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct StartupServiceProbe {
    events: Arc<Mutex<Vec<&'static str>>>,
    probes: AtomicU8,
    confirms_stopped: bool,
    runtime_owned: bool,
}

struct StartupFallbackServiceProbe {
    inner: StartupServiceProbe,
}

struct StartupTransitionRaceService {
    inner: StartupServiceProbe,
    transition_lock: Arc<tokio::sync::Mutex<()>>,
    transition_attempts: Arc<AtomicUsize>,
    probe_entered: Arc<Notify>,
    release_probe: Arc<Notify>,
}

struct RetiringCore {
    inner: RecordingCore,
    selected: Mutex<RunType>,
}

struct StartupOwnerCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    reconciles: AtomicUsize,
    selected: Mutex<RunType>,
    local_status: Mutex<(CoreState, i64)>,
    service_running: Arc<AtomicBool>,
    local_receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
    service_receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
    receipt_queries: AtomicU8,
    invalidate_receipt_after_first_query: bool,
    stop_confirms: bool,
    stop_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
    service_retire_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
    service_recover_error: Option<(chimera_core_manager::CoreErrorKind, bool)>,
    outcome_uncertain: AtomicBool,
}

struct MutableStartupServiceProbe {
    events: Arc<Mutex<Vec<&'static str>>>,
    running: Arc<AtomicBool>,
    stopped_reason: Option<String>,
    runtime_owned: bool,
}

struct ForeignAfterRecoveryServiceProbe {
    inner: MutableStartupServiceProbe,
}

struct RestartBudgetService {
    events: Arc<Mutex<Vec<&'static str>>>,
    attempts: AtomicU8,
    exhausted: AtomicBool,
}

struct StartupRetryLeaseService {
    inner: RestartBudgetService,
    transition_attempts: AtomicU8,
    retry_lease_gate: Option<(Arc<Notify>, Arc<Notify>)>,
}

struct RecordingServiceTransition {
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct StartupTransitionRaceLease {
    _guard: tokio::sync::OwnedMutexGuard<()>,
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct ServiceHandoffCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    local: Arc<std::sync::atomic::AtomicBool>,
}

struct ServiceModeRecordingCore {
    application: ApplicationClient,
    observations: Arc<Mutex<Vec<(RunType, ClashCore, bool)>>>,
    mutate_application_on_first_reconcile: AtomicBool,
    fail_first_reconcile: AtomicBool,
}

struct StartupDegradedServiceCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    reconciles: AtomicU8,
    stopped: AtomicBool,
    selected: Mutex<RunType>,
}

/// Simulates a Service application whose operation returned a refusal and
/// whose Core status has settled to Stopped; the independent Service probe
/// may still observe an owned daemon Core until its conditional retirement.
struct PostcommitServiceFallbackCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    service_running: Arc<AtomicBool>,
    selected: Mutex<RunType>,
    local_running: AtomicBool,
    refused_service: AtomicBool,
    forbid_local: AtomicBool,
}

#[async_trait]
impl CoreLifecyclePort for PostcommitServiceFallbackCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _: ClashConfig,
        _: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        *self.selected.lock().unwrap() = run_type;
        match run_type {
            RunType::Service => {
                self.events.lock().unwrap().push("service-apply-refused");
                self.refused_service.store(true, AtomicOrdering::Release);
                anyhow::bail!("injected Service Core apply refusal")
            }
            RunType::Normal => {
                anyhow::ensure!(
                    !self.forbid_local.load(AtomicOrdering::Acquire)
                        && !self.service_running.load(AtomicOrdering::Acquire),
                    "unsafe Local start while Service still owns a Core"
                );
                self.local_running.store(true, AtomicOrdering::Release);
                self.events.lock().unwrap().push("rebuild-local");
                Ok(())
            }
            RunType::Elevated => anyhow::bail!("unexpected elevated fallback"),
        }
    }

    async fn reconcile_after_service_release(
        &self,
        _: ClashConfig,
        _: ClashCore,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.forbid_local.load(AtomicOrdering::Acquire)
                && !self.service_running.load(AtomicOrdering::Acquire),
            "Local fallback must not start while an owned Service Core remains"
        );
        *self.selected.lock().unwrap() = RunType::Normal;
        self.local_running.store(true, AtomicOrdering::Release);
        self.events
            .lock()
            .unwrap()
            .push("rebuild-after-service-release");
        Ok(())
    }

    async fn retire_service_core(&self, _: chimera_core_manager::CoreOwner) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-core-retire");
        self.service_running.store(false, AtomicOrdering::Release);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.local_running.store(false, AtomicOrdering::Release);
        Ok(())
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let run_type = *self.selected.lock().unwrap();
        let running = match run_type {
            RunType::Service => {
                self.service_running.load(AtomicOrdering::Acquire)
                    && !self.refused_service.load(AtomicOrdering::Acquire)
            }
            RunType::Normal => self.local_running.load(AtomicOrdering::Acquire),
            RunType::Elevated => false,
        };
        Ok(CoreStatusSnapshot {
            instance_id: Some("service-fallback-test-instance".into()),
            state: if running {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: if running { 1 } else { 2 },
            run_type,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

struct StartupTransientCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    reconciles: AtomicU8,
    stops: AtomicU8,
    recovered: Arc<Notify>,
    outcome_uncertain: bool,
    stop_gate: Option<(Arc<Notify>, Arc<Notify>)>,
}

struct StartupReceiptCore {
    events: Arc<Mutex<Vec<&'static str>>>,
    status: Mutex<(CoreState, i64)>,
    receipt: Mutex<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>>,
    stop_confirms: bool,
}

fn startup_owner_core(
    events: Arc<Mutex<Vec<&'static str>>>,
    local_state: (CoreState, i64),
    service_running: Arc<AtomicBool>,
) -> StartupOwnerCore {
    StartupOwnerCore {
        events,
        reconciles: AtomicUsize::new(0),
        selected: Mutex::new(RunType::Normal),
        local_status: Mutex::new(local_state),
        service_running,
        local_receipt: Mutex::new(None),
        service_receipt: Mutex::new(None),
        receipt_queries: AtomicU8::new(0),
        invalidate_receipt_after_first_query: false,
        stop_confirms: true,
        stop_error: None,
        service_retire_error: None,
        service_recover_error: None,
        outcome_uncertain: AtomicBool::new(false),
    }
}

async fn spawn_startup_client(
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
) -> RuntimeReconcilerClient {
    spawn_startup_client_with_application(core, service, ApplicationClient::legacy().unwrap()).await
}

async fn spawn_startup_client_with_application(
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
    application: ApplicationClient,
) -> RuntimeReconcilerClient {
    spawn_startup_client_with_application_and_clash(
        core,
        service,
        application,
        ClashConfigClient::legacy().unwrap(),
    )
    .await
}

async fn spawn_startup_client_with_application_and_clash(
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
    application: ApplicationClient,
    clash: ClashConfigClient,
) -> RuntimeReconcilerClient {
    let installer: Arc<dyn ports::BinaryInstaller> =
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        )));
    RuntimeReconcilerClient::spawn_with_service(
        core,
        application,
        clash,
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        installer,
        service,
    )
    .await
    .unwrap()
}

fn signal_service_readiness_change(client: &RuntimeReconcilerClient) {
    let ClientInner::Actor { actor_ref, .. } = client.0.as_ref() else {
        panic!("Service readiness follow-up requires the lifecycle actor");
    };
    actor_ref
        .cast(Message::ServiceReadiness)
        .expect("Service readiness change should reach the lifecycle actor");
}

async fn spawn_startup_client_with_profiles(
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
    profiles: chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>,
) -> RuntimeReconcilerClient {
    let installer: Arc<dyn ports::BinaryInstaller> =
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        )));
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    RuntimeReconcilerClient::spawn_with_profiles(
        core,
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        Some(profiles),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        installer,
        service,
        shutdown,
        &tasks,
    )
    .await
    .unwrap()
}

/// Canonical committed-source fixture. The fake builder serializes the frozen
/// Profiles document into a test product, so the fake Core sees only the
/// submitted bytes (never a second read of the live source).
async fn spawn_prepared_client_with_profiles(
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
    profiles: chimera_core::state::StateSnapshot<chimera_config::profile::Profiles>,
) -> RuntimeReconcilerClient {
    use crate::client::{
        ports::{RuntimePreparationSession, SessionPortResolver},
        reconciler::check::{Args, CandidateCheck},
    };
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| Utf8PathBuf::from_path_buf(dir.path().join(name)).unwrap();
    let application = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let app_manager = chimera_state_engine::PersistentStateManagerSetup::<
        chimera_config::application::ChimeraAppConfig,
    >::builder()
    .config_path(path("application.yaml"))
    .assemble()
    .from_state(application.get_typed());
    let clash_manager = chimera_state_engine::PersistentStateManagerSetup::<ClashConfig>::builder()
        .config_path(path("clash.yaml"))
        .assemble()
        .from_state(clash.get().unwrap());
    let reader = crate::state::mutation::ProfileContentReader::new(
        profiles.clone(),
        crate::state::mutation::ProfileContentLock::default(),
    );
    let lifecycle = Arc::new(crate::client::runtime::RuntimeLifecycle::default());
    let gate = Arc::new(CandidateCheck::new(Args {
        application: app_manager.snapshot_handle(),
        clash: clash_manager.snapshot_handle(),
        profile_content: reader.clone(),
        builder: Arc::new(PreparedProfilesBuild),
        session: RuntimePreparationSession {
            revisions: lifecycle.revision_allocator(),
            ports: Arc::new(SessionPortResolver::new(lifecycle)),
        },
        validator: Arc::new(UnusedPreparedCheck),
        workflow: Arc::new(UnusedPreparedInbox),
    }));
    let installer = Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
        chimera_os::elevation::RefuseElevation,
    )));
    let client = RuntimeReconcilerClient::spawn_with_profiles_and_content(
        core,
        application,
        clash,
        Some(profiles),
        Some(reader),
        Some(Arc::new(EmptyManagedFiles)),
        RuntimePaths::from_config_root(dir.path().to_path_buf()),
        installer,
        service,
        CancellationToken::new(),
        &TaskTracker::new(),
    )
    .await
    .unwrap();
    client.install_candidate_gate(gate).await.unwrap();
    client
}

async fn startup_target_for(
    application: ApplicationClient,
    clash: ClashConfigClient,
    core: Arc<dyn CoreLifecyclePort>,
    service: Arc<dyn ServiceLifecyclePort>,
) -> String {
    let workflow = CoreLifecycleWorkflow::new(
        application,
        clash,
        None,
        core,
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    workflow
        .startup_target_identity()
        .await
        .expect("startup target should be available")
}

fn startup_test_receipt(
    target: Option<String>,
) -> Arc<crate::client::runtime::RuntimeApplyReceipt> {
    startup_test_receipt_for(
        target,
        crate::core::actor_v2::control_endpoint::ExecutionHost::Local,
    )
}

fn startup_test_receipt_for(
    target: Option<String>,
    host: crate::core::actor_v2::control_endpoint::ExecutionHost,
) -> Arc<crate::client::runtime::RuntimeApplyReceipt> {
    let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
    receipt.target = target;
    receipt.host = host;
    Arc::new(receipt)
}

struct ApplicationPatchRecordingCore {
    application: ApplicationClient,
    observations: Arc<Mutex<Vec<(bool, bool)>>>,
    reject_check: bool,
}

struct TestApplicationBridge;

impl crate::state::mirror::VergeLegacyBridge for TestApplicationBridge {
    fn prepare(
        &self,
        _snap: &chimera_config::application::ChimeraAppConfig,
    ) -> anyhow::Result<Box<dyn crate::state::mirror::PreparedLegacyMirror>> {
        Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
    }

    fn snapshot_legacy(&self) -> anyhow::Result<chimera_config::application::ChimeraAppConfig> {
        Ok(chimera_config::application::ChimeraAppConfig::default())
    }
}

struct LifecycleTestProxy {
    state: Mutex<chimera_contracts::system_proxy::OsProxyConfig>,
    writes: Mutex<Vec<chimera_contracts::system_proxy::OsProxyConfig>>,
    pause_disable: bool,
    started: Mutex<Option<oneshot::Sender<()>>>,
    released: Mutex<bool>,
    release: Condvar,
}

impl LifecycleTestProxy {
    fn new(state: chimera_contracts::system_proxy::OsProxyConfig) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(state),
            writes: Mutex::new(Vec::new()),
            pause_disable: false,
            started: Mutex::new(None),
            released: Mutex::new(true),
            release: Condvar::new(),
        })
    }

    fn pause_first_disable(
        state: chimera_contracts::system_proxy::OsProxyConfig,
    ) -> (Arc<Self>, oneshot::Receiver<()>) {
        let (started_tx, started_rx) = oneshot::channel();
        (
            Arc::new(Self {
                state: Mutex::new(state),
                writes: Mutex::new(Vec::new()),
                pause_disable: true,
                started: Mutex::new(Some(started_tx)),
                released: Mutex::new(false),
                release: Condvar::new(),
            }),
            started_rx,
        )
    }

    fn release_disable(&self) {
        *self.released.lock().unwrap() = true;
        self.release.notify_all();
    }

    fn current(&self) -> chimera_contracts::system_proxy::OsProxyConfig {
        self.state.lock().unwrap().clone()
    }

    fn writes(&self) -> Vec<chimera_contracts::system_proxy::OsProxyConfig> {
        self.writes.lock().unwrap().clone()
    }
}

impl chimera_contracts::system_proxy::OsProxyPort for LifecycleTestProxy {
    fn get(
        &self,
    ) -> Result<
        chimera_contracts::system_proxy::OsProxyConfig,
        chimera_contracts::system_proxy::OsProxyError,
    > {
        Ok(self.current())
    }

    fn set(
        &self,
        config: &chimera_contracts::system_proxy::OsProxyConfig,
    ) -> Result<(), chimera_contracts::system_proxy::OsProxyError> {
        if self.pause_disable
            && !config.enable
            && let Some(started) = self.started.lock().unwrap().take()
        {
            let _ = started.send(());
            let mut released = self.released.lock().unwrap();
            while !*released {
                released = self.release.wait(released).unwrap();
            }
        }
        self.writes.lock().unwrap().push(config.clone());
        *self.state.lock().unwrap() = config.clone();
        Ok(())
    }

    fn default_bypass(&self) -> &'static str {
        "localhost"
    }
}

struct LifecycleTestAutoLaunch;

impl chimera_contracts::system_proxy::AutoLaunchPort for LifecycleTestAutoLaunch {
    fn is_enabled(&self) -> Result<bool, chimera_contracts::system_proxy::AutoLaunchError> {
        Ok(false)
    }

    fn set_enabled(
        &self,
        _enabled: bool,
    ) -> Result<(), chimera_contracts::system_proxy::AutoLaunchError> {
        Ok(())
    }
}

struct LifecycleTestPac;

#[async_trait]
impl chimera_contracts::system_proxy::PacPort for LifecycleTestPac {
    fn is_supported(&self) -> bool {
        false
    }

    async fn apply(
        &self,
        _url: &url::Url,
        _cancel: CancellationToken,
    ) -> Result<(), chimera_contracts::system_proxy::PacError> {
        Err(chimera_contracts::system_proxy::PacError::DownloadCancelled)
    }

    fn disable(&self) -> Result<(), chimera_contracts::system_proxy::PacError> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingProxyActionNotifications {
    applications: Mutex<Vec<(bool, Vec<crate::client::effects::plan::EffectKind>)>>,
    ports: Mutex<Vec<Option<u16>>>,
}

impl crate::client::effects::ports::CommitNotifications for RecordingProxyActionNotifications {
    fn application_committed(
        &self,
        fields: crate::client::effects::plan::ApplicationEffectFields,
        kinds: Vec<crate::client::effects::plan::EffectKind>,
    ) {
        self.applications
            .lock()
            .unwrap()
            .push((fields.enable_system_proxy, kinds));
    }

    fn clash_committed(&self, _: crate::client::effects::plan::ClashEffectFields) {}

    fn profiles_committed(&self, _: Vec<chimera_config::profile::ProfileId>) {}

    fn runtime_bound(
        &self,
        ports: Option<chimera_config::runtime::executor::ResolvedPortBindings>,
        _: bool,
    ) {
        self.ports
            .lock()
            .unwrap()
            .push(ports.map(|binding| binding.mixed_port));
    }

    fn publish_full(&self, _: Option<chimera_config::runtime::executor::ResolvedPortBindings>) {}
}

async fn spawn_lifecycle_system_proxy(
    os: Arc<dyn chimera_contracts::system_proxy::OsProxyPort>,
) -> (
    crate::client::system_proxy::SystemProxyClient,
    CancellationToken,
    TaskTracker,
) {
    let shutdown = CancellationToken::new();
    let tasks = TaskTracker::new();
    let client = crate::client::system_proxy::SystemProxyClient::spawn(
        crate::client::system_proxy::SystemProxyArgs {
            os,
            auto_launch: Arc::new(LifecycleTestAutoLaunch),
            pac: Arc::new(LifecycleTestPac),
            schedule_guard_ticks: false,
            shutdown: shutdown.child_token(),
        },
        &tasks,
    )
    .await
    .unwrap();
    (client, shutdown, tasks)
}

fn stale_proxy_config() -> chimera_contracts::system_proxy::OsProxyConfig {
    chimera_contracts::system_proxy::OsProxyConfig {
        enable: true,
        host: "127.0.0.1".into(),
        port: 7890,
        bypass: "localhost".into(),
    }
}

async fn application_with_system_proxy(enabled: bool) -> (tempfile::TempDir, ApplicationClient) {
    let directory = tempfile::tempdir().unwrap();
    let config_path =
        camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
    let application = ApplicationClient::new(
        config_path,
        chimera_config::application::ChimeraAppConfig {
            enable_system_proxy: enabled,
            ..Default::default()
        },
        Arc::new(TestApplicationBridge),
        chimera_config::application::ReleaseChannel::Stable,
    )
    .await
    .unwrap();
    (directory, application)
}

fn direct_lifecycle_client(
    application: ApplicationClient,
    core: Arc<dyn CoreLifecyclePort>,
) -> RuntimeReconcilerClient {
    direct_lifecycle_client_with_mixed_port(application, core, 7890)
}

fn direct_lifecycle_client_with_mixed_port(
    application: ApplicationClient,
    core: Arc<dyn CoreLifecyclePort>,
    mixed_port: u16,
) -> RuntimeReconcilerClient {
    RuntimeReconcilerClient::direct(
        core,
        application,
        ClashConfigClient::legacy_with(ClashConfig {
            mixed_port: chimera_config::clash::config::clash_strategy::PortStrategy {
                kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
                start_port: mixed_port,
            },
            ..Default::default()
        })
        .unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
}

fn counted_mirror_persistence(
    calls: Arc<AtomicU8>,
) -> Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync> {
    Arc::new(move || {
        calls.fetch_add(1, AtomicOrdering::AcqRel);
        Ok(())
    })
}

#[tokio::test]
async fn stale_proxy_lifecycle_rejects_changed_core_without_host_or_mirror_writes() {
    use crate::client::system_proxy::SystemProxyActionError;

    let (_directory, application) = application_with_system_proxy(true).await;
    let os = LifecycleTestProxy::new(stale_proxy_config());
    let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
    let core = Arc::new(BindingCore {
        running: AtomicBool::new(false),
    });
    let lifecycle = direct_lifecycle_client(application, core.clone());
    let persist_calls = Arc::new(AtomicU8::new(0));
    let notifications = Arc::new(RecordingProxyActionNotifications::default());

    let error = lifecycle
        .disable_stale_system_proxy(
            system_proxy.clone(),
            7890,
            2,
            true,
            stale_proxy_config(),
            notifications.clone(),
            counted_mirror_persistence(persist_calls.clone()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, SystemProxyActionError::StateChanged));

    core.running.store(true, AtomicOrdering::Release);
    let error = lifecycle
        .disable_stale_system_proxy(
            system_proxy,
            7890,
            1,
            true,
            stale_proxy_config(),
            notifications,
            counted_mirror_persistence(persist_calls.clone()),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, SystemProxyActionError::StateChanged));
    assert!(os.writes().is_empty());
    assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn stale_proxy_lifecycle_rejects_changed_mixed_port_without_host_or_mirror_writes() {
    use crate::client::system_proxy::SystemProxyActionError;

    let (_directory, application) = application_with_system_proxy(true).await;
    let os = LifecycleTestProxy::new(stale_proxy_config());
    let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
    let lifecycle = direct_lifecycle_client_with_mixed_port(
        application,
        Arc::new(BindingCore {
            running: AtomicBool::new(false),
        }),
        7891,
    );
    let persist_calls = Arc::new(AtomicU8::new(0));

    let error = lifecycle
        .disable_stale_system_proxy(
            system_proxy,
            7890,
            1,
            true,
            stale_proxy_config(),
            Arc::new(RecordingProxyActionNotifications::default()),
            counted_mirror_persistence(persist_calls.clone()),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, SystemProxyActionError::StateChanged));
    assert!(os.writes().is_empty());
    assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn stale_proxy_lifecycle_commits_and_persists_preference_after_host_disable() {
    let (_directory, application) = application_with_system_proxy(true).await;
    let os = LifecycleTestProxy::new(stale_proxy_config());
    let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
    let lifecycle = direct_lifecycle_client(
        application.clone(),
        Arc::new(BindingCore {
            running: AtomicBool::new(false),
        }),
    );
    let persist_calls = Arc::new(AtomicU8::new(0));
    let notifications = Arc::new(RecordingProxyActionNotifications::default());

    lifecycle
        .disable_stale_system_proxy(
            system_proxy,
            7890,
            1,
            true,
            stale_proxy_config(),
            notifications.clone(),
            counted_mirror_persistence(persist_calls.clone()),
        )
        .await
        .unwrap();

    assert!(!application.get_typed().enable_system_proxy);
    assert!(!os.current().enable);
    assert_eq!(os.writes().len(), 1);
    assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 1);
    assert_eq!(
        notifications.applications.lock().unwrap().as_slice(),
        [(
            false,
            vec![crate::client::effects::plan::EffectKind::SystemProxy]
        )]
    );
    assert_eq!(notifications.ports.lock().unwrap().as_slice(), [None]);

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn stale_proxy_lifecycle_reports_mirror_failure_without_reenabling_proxy() {
    use crate::client::system_proxy::SystemProxyActionError;

    let (_directory, application) = application_with_system_proxy(true).await;
    let os = LifecycleTestProxy::new(stale_proxy_config());
    let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
    let lifecycle = direct_lifecycle_client(
        application.clone(),
        Arc::new(BindingCore {
            running: AtomicBool::new(false),
        }),
    );
    let notifications = Arc::new(RecordingProxyActionNotifications::default());
    let persist_calls = Arc::new(AtomicU8::new(0));
    let persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync> = {
        let persist_calls = persist_calls.clone();
        Arc::new(move || {
            persist_calls.fetch_add(1, AtomicOrdering::AcqRel);
            anyhow::bail!("injected mirror failure")
        })
    };

    let error = lifecycle
        .disable_stale_system_proxy(
            system_proxy,
            7890,
            1,
            true,
            stale_proxy_config(),
            notifications.clone(),
            persist_legacy_mirror,
        )
        .await
        .unwrap_err();

    assert!(matches!(error, SystemProxyActionError::PartialApply(_)));
    assert!(!application.get_typed().enable_system_proxy);
    assert!(!os.current().enable);
    assert_eq!(os.writes().len(), 1);
    assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 1);
    assert_eq!(notifications.applications.lock().unwrap().len(), 1);

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[tokio::test]
async fn stale_proxy_lifecycle_conflict_keeps_latest_preference_without_stale_rollback() {
    use crate::client::system_proxy::SystemProxyActionError;

    let (_directory, application) = application_with_system_proxy(true).await;
    let (os, disable_started) = LifecycleTestProxy::pause_first_disable(stale_proxy_config());
    let (system_proxy, shutdown, tasks) = spawn_lifecycle_system_proxy(os.clone()).await;
    let lifecycle = direct_lifecycle_client(
        application.clone(),
        Arc::new(BindingCore {
            running: AtomicBool::new(false),
        }),
    );
    let notifications = Arc::new(RecordingProxyActionNotifications::default());
    let persist_calls = Arc::new(AtomicU8::new(0));
    let action = {
        let lifecycle = lifecycle.clone();
        let system_proxy = system_proxy.clone();
        let notifications = notifications.clone();
        let persist_calls = persist_calls.clone();
        tokio::spawn(async move {
            lifecycle
                .disable_stale_system_proxy(
                    system_proxy,
                    7890,
                    1,
                    true,
                    stale_proxy_config(),
                    notifications,
                    counted_mirror_persistence(persist_calls),
                )
                .await
        })
    };
    disable_started.await.unwrap();

    application
        .patch_typed(ChimeraAppConfigPatch {
            enable_system_proxy: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
    os.release_disable();

    let error = action.await.unwrap().unwrap_err();
    assert!(matches!(error, SystemProxyActionError::StateChanged));
    assert!(!application.get_typed().enable_system_proxy);
    assert!(!os.current().enable);
    assert_eq!(os.writes().len(), 1, "stale recovery must not re-enable");
    assert_eq!(persist_calls.load(AtomicOrdering::Acquire), 0);
    assert_eq!(
        notifications.applications.lock().unwrap().last().unwrap().0,
        false
    );
    assert_eq!(notifications.ports.lock().unwrap().as_slice(), [None]);

    shutdown.cancel();
    tasks.close();
    tasks.wait().await;
}

#[async_trait]
impl ServiceTransitionLease for RecordingServiceTransition {
    async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-install");
        Ok(())
    }

    async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-uninstall");
        Ok(())
    }

    async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-update");
        Ok(())
    }

    async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-start");
        Ok(())
    }

    async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-restart");
        Ok(())
    }

    async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-stop");
        Ok(())
    }

    async fn confirm_ready(&mut self, _timeout: Duration) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-ready");
        Ok(())
    }

    async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-confirm");
        Ok(())
    }
}

#[async_trait]
impl ServiceLifecyclePort for RecordingService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe");
        Ok(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Stopped,
            server: None,
        })
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.events.lock().unwrap().push("service-begin");
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for ActorEvidenceService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.probes.fetch_add(1, AtomicOrdering::Relaxed);
        if self.probe_failed.load(Ordering::Acquire) {
            anyhow::bail!("ref ServiceActor probe did not answer");
        }
        Ok(chimera_ipc::types::StatusInfo {
            name: "chimera-service".into(),
            version: "1.10.1".into(),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
        })
    }

    fn actor_status(&self) -> Option<ServiceHostStatus> {
        Some(self.actor_status.lock().unwrap().clone())
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for TransitionUnavailableService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner
            .events
            .lock()
            .unwrap()
            .push("service-begin-failed");
        anyhow::bail!("test Service transition lease is unavailable")
    }
}

#[async_trait]
impl ServiceLifecyclePort for UnreadableService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe-failed");
        anyhow::bail!("test Service probe is unavailable")
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.events.lock().unwrap().push("service-begin");
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for StartupServiceProbe {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe");
        let probe = self.probes.fetch_add(1, AtomicOrdering::AcqRel);
        let core_detail = if probe == 0 || !self.confirms_stopped {
            chimera_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }
        } else {
            chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }
        };
        Ok(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.11.1"),
            status: chimera_ipc::types::ServiceStatus::Running,
            server: Some(chimera_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: std::borrow::Cow::Borrowed("1.11.1"),
                core_infos: chimera_ipc::api::status::CoreInfos {
                    instance_id: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
                    r#type: None,
                    state: CoreState::Running,
                    state_changed_at: 1,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: Some(chimera_ipc::api::status::ConfigRevisionInfo {
                        epoch: 1,
                        generation: 3,
                        source_hash: "89abcdef01234567".into(),
                        effective_hash: "0123456789abcdef".into(),
                    }),
                    detail: Some(core_detail),
                },
                runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(Default::default()),
                    service_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                },
            }),
        })
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.runtime_owned
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.events.lock().unwrap().push("service-begin");
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for StartupFallbackServiceProbe {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        let mut info = self.inner.probe().await?;
        if self.inner.probes.load(AtomicOrdering::Acquire) >= 2
            && let Some(server) = info.server.as_mut()
        {
            server.core_infos.detail =
                Some(chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None });
        }
        Ok(info)
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.inner.runtime_owned
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.begin_transition().await
    }
}

#[async_trait]
impl ServiceLifecyclePort for StartupTransitionRaceService {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        if self.inner.probes.load(AtomicOrdering::Acquire) == 0 {
            self.probe_entered.notify_one();
            self.release_probe.notified().await;
        }
        self.inner.probe().await
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        true
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.transition_attempts
            .fetch_add(1, AtomicOrdering::AcqRel);
        let guard = self.transition_lock.clone().lock_owned().await;
        self.inner.events.lock().unwrap().push("service-begin");
        Ok(Box::new(StartupTransitionRaceLease {
            _guard: guard,
            events: self.inner.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceTransitionLease for StartupTransitionRaceLease {
    async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-install");
        Ok(())
    }

    async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-uninstall");
        Ok(())
    }

    async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-update");
        Ok(())
    }

    async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-start");
        Ok(())
    }

    async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-restart");
        Ok(())
    }

    async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-stop");
        Ok(())
    }

    async fn confirm_ready(&mut self, _timeout: Duration) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-ready");
        Ok(())
    }

    async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-confirm");
        Ok(())
    }
}

#[async_trait]
impl CoreLifecyclePort for RetiringCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.inner.events.lock().unwrap().push("rebuild");
        Ok(())
    }

    async fn retire_service_core(
        &self,
        _expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        self.inner
            .events
            .lock()
            .unwrap()
            .push("service-core-retire");
        Ok(())
    }

    async fn reconcile_after_service_release(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
    ) -> anyhow::Result<()> {
        self.inner
            .events
            .lock()
            .unwrap()
            .push("rebuild-after-service-release");
        Ok(())
    }

    async fn reconcile_profiles_after_service_release(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _profiles: Arc<chimera_config::profile::Profiles>,
        _app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
        _target: Option<String>,
    ) -> anyhow::Result<()> {
        self.inner
            .events
            .lock()
            .unwrap()
            .push("rebuild-after-service-release");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.inner.stop().await
    }

    async fn change_core(&self, core: ClashCore) -> anyhow::Result<()> {
        self.inner.change_core(core).await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let mut status = self.inner.status().await?;
        let selected = *self.selected.lock().unwrap();
        status.run_type = selected;
        if selected == RunType::Normal && self.inner.run_type == RunType::Service {
            status.instance_id = None;
            status.state_changed_at = 0;
        }
        Ok(status)
    }

    async fn select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        *self.selected.lock().unwrap() = match host {
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
        };
        self.inner.events.lock().unwrap().push(match host {
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local => "select-local",
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service => "select-service",
        });
        Ok(())
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        Some(self.inner.recovery_notify.clone())
    }

    async fn on_profile_change(&self, break_when: bool) {
        self.inner.on_profile_change(break_when).await;
    }
}

#[async_trait]
impl CoreLifecyclePort for StartupOwnerCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        self.reconciles.fetch_add(1, AtomicOrdering::AcqRel);
        let (host, label) = match run_type {
            RunType::Normal => {
                *self.local_status.lock().unwrap() = (CoreState::Running, 3);
                ("local", "rebuild-local")
            }
            RunType::Service => {
                self.service_running.store(true, AtomicOrdering::Release);
                ("service", "rebuild-service")
            }
            RunType::Elevated => anyhow::bail!("elevated Core is unsupported in this fixture"),
        };
        *self.selected.lock().unwrap() = run_type;
        self.events.lock().unwrap().push(label);
        tracing::debug!(host, "test startup Core reconciled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let selected = *self.selected.lock().unwrap();
        let (state, event) = match selected {
            RunType::Normal => {
                if self.stop_confirms {
                    *self.local_status.lock().unwrap() = (CoreState::Stopped(None), 2);
                }
                (self.local_status.lock().unwrap().0.clone(), "stop-local")
            }
            RunType::Service => {
                if self.stop_confirms {
                    self.service_running.store(false, AtomicOrdering::Release);
                }
                (
                    if self.service_running.load(AtomicOrdering::Acquire) {
                        CoreState::Running
                    } else {
                        CoreState::Stopped(None)
                    },
                    "stop-service",
                )
            }
            RunType::Elevated => anyhow::bail!("elevated Core is unsupported in this fixture"),
        };
        self.events.lock().unwrap().push(event);
        if let Some((kind, retryable)) = self.stop_error {
            if kind == chimera_core_manager::CoreErrorKind::StopUnconfirmed
                || kind == chimera_core_manager::CoreErrorKind::Quarantined
            {
                self.outcome_uncertain.store(true, AtomicOrdering::Release);
            }
            return Err(chimera_core_manager::CoreError::new(
                kind,
                format!("injected {kind:?} stop error while {state:?}"),
                retryable,
            )
            .into());
        }
        Ok(())
    }

    async fn recover_service_quarantine(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-core-recover");
        if let Some((kind, retryable)) = self.service_recover_error {
            if matches!(
                kind,
                chimera_core_manager::CoreErrorKind::StopUnconfirmed
                    | chimera_core_manager::CoreErrorKind::Quarantined
            ) {
                self.outcome_uncertain.store(true, AtomicOrdering::Release);
            }
            return Err(chimera_core_manager::CoreError::new(
                kind,
                format!("injected {kind:?} Service recovery error"),
                retryable,
            )
            .into());
        }
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let run_type = *self.selected.lock().unwrap();
        let (instance_id, state, state_changed_at) = match run_type {
            RunType::Normal => {
                let (state, changed_at) = self.local_status.lock().unwrap().clone();
                ("local-core-instance".into(), state, changed_at)
            }
            RunType::Service if self.service_running.load(AtomicOrdering::Acquire) => (
                "9af54a67-fc31-4dcb-8422-26644c6277c8".into(),
                CoreState::Running,
                1,
            ),
            RunType::Service => (
                "9af54a67-fc31-4dcb-8422-26644c6277c8".into(),
                CoreState::Stopped(None),
                2,
            ),
            RunType::Elevated => (String::new(), CoreState::Stopped(None), 0),
        };
        Ok(CoreStatusSnapshot {
            instance_id: (!instance_id.is_empty()).then_some(instance_id),
            state,
            state_changed_at,
            run_type,
        })
    }

    async fn select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        let run_type = match host {
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
        };
        *self.selected.lock().unwrap() = run_type;
        self.events.lock().unwrap().push(match run_type {
            RunType::Normal => "select-local",
            RunType::Service => "select-service",
            RunType::Elevated => unreachable!(),
        });
        Ok(())
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        let query = self.receipt_queries.fetch_add(1, AtomicOrdering::AcqRel);
        if self.invalidate_receipt_after_first_query && query > 0 {
            return Ok(None);
        }
        let status = self.status().await?;
        if !matches!(status.state, CoreState::Running) {
            return Ok(None);
        }
        Ok(match status.run_type {
            RunType::Normal => self.local_receipt.lock().unwrap().clone(),
            RunType::Service => self.service_receipt.lock().unwrap().clone(),
            RunType::Elevated => None,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        let status = self.status().await?;
        if matches!(status.state, CoreState::Stopped(_)) {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
        } else {
            Ok(crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied)
        }
    }

    async fn retire_service_core(
        &self,
        _expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-core-retire");
        self.service_running.store(false, AtomicOrdering::Release);
        if let Some((kind, retryable)) = self.service_retire_error {
            if matches!(
                kind,
                chimera_core_manager::CoreErrorKind::StopUnconfirmed
                    | chimera_core_manager::CoreErrorKind::Quarantined
            ) {
                self.outcome_uncertain.store(true, AtomicOrdering::Release);
            }
            return Err(chimera_core_manager::CoreError::new(
                kind,
                format!("injected {kind:?} Service stop error"),
                retryable,
            )
            .into());
        }
        Ok(())
    }

    fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(AtomicOrdering::Acquire)
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl ServiceLifecyclePort for MutableStartupServiceProbe {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe");
        let running = self.running.load(AtomicOrdering::Acquire);
        let state = if running {
            CoreState::Running
        } else {
            CoreState::Stopped(self.stopped_reason.clone())
        };
        let detail = if running {
            chimera_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }
        } else {
            chimera_ipc::api::status::CoreStateDetail::Stopped {
                reason: self.stopped_reason.clone(),
            }
        };
        Ok(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.11.1"),
            status: chimera_ipc::types::ServiceStatus::Running,
            server: Some(chimera_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: std::borrow::Cow::Borrowed("1.11.1"),
                core_infos: chimera_ipc::api::status::CoreInfos {
                    instance_id: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
                    r#type: None,
                    state,
                    state_changed_at: if running { 1 } else { 2 },
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: Some(chimera_ipc::api::status::ConfigRevisionInfo {
                        epoch: 1,
                        generation: 3,
                        source_hash: "89abcdef01234567".into(),
                        effective_hash: "0123456789abcdef".into(),
                    }),
                    detail: Some(detail),
                },
                runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(Default::default()),
                    service_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                },
            }),
        })
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.runtime_owned
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for ToggleStartupServiceProbe {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe");
        let ready = self.ready.load(AtomicOrdering::Acquire);
        let core_running = self.core_running.load(AtomicOrdering::Acquire);
        let state = if core_running {
            CoreState::Running
        } else {
            CoreState::Stopped(None)
        };
        let detail = if core_running {
            chimera_ipc::api::status::CoreStateDetail::Running { epoch: 1, pid: 7 }
        } else {
            chimera_ipc::api::status::CoreStateDetail::Stopped { reason: None }
        };
        Ok(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.11.1"),
            status: if ready {
                chimera_ipc::types::ServiceStatus::Running
            } else {
                chimera_ipc::types::ServiceStatus::Stopped
            },
            server: ready.then(|| chimera_ipc::api::status::StatusResBody {
                log_query_version: None,
                version: std::borrow::Cow::Borrowed("1.11.1"),
                core_infos: chimera_ipc::api::status::CoreInfos {
                    instance_id: Some("9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
                    r#type: None,
                    state,
                    state_changed_at: if core_running { 1 } else { 2 },
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: Some(chimera_ipc::api::status::ConfigRevisionInfo {
                        epoch: 1,
                        generation: 3,
                        source_hash: "89abcdef01234567".into(),
                        effective_hash: "0123456789abcdef".into(),
                    }),
                    detail: Some(detail),
                },
                runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                    service_data_dir: std::borrow::Cow::Owned(Default::default()),
                    service_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_config_dir: std::borrow::Cow::Owned(Default::default()),
                    nyanpasu_data_dir: std::borrow::Cow::Owned(Default::default()),
                },
            }),
        })
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        true
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.events.lock().unwrap().push("service-begin");
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.events.lock().unwrap().push("service-try-begin");
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for LeaseBlockedToggleService {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }

    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.inner.runtime_owned(info)
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.begin_transition().await
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        if self.block_next.swap(false, AtomicOrdering::AcqRel) {
            anyhow::bail!("injected Service transition lease busy");
        }
        self.inner.try_begin_transition().await
    }
}

#[async_trait]
impl ServiceLifecyclePort for ForeignAfterRecoveryServiceProbe {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }

    fn runtime_owned(&self, _info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        self.inner.runtime_owned
            && !self
                .inner
                .events
                .lock()
                .unwrap()
                .contains(&"service-core-recover")
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        self.inner.begin_transition().await
    }
}

#[async_trait]
impl CoreLifecyclePort for StartupReceiptCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild");
        *self.status.lock().unwrap() = (CoreState::Running, 3);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("stop");
        if self.stop_confirms {
            *self.status.lock().unwrap() = (CoreState::Stopped(None), 2);
        }
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        anyhow::bail!("startup receipt test does not select a core")
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let (state, state_changed_at) = self.status.lock().unwrap().clone();
        Ok(CoreStatusSnapshot {
            instance_id: Some("local-core-instance".into()),
            state,
            state_changed_at,
            run_type: RunType::Normal,
        })
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        let (state, _) = self.status.lock().unwrap().clone();
        Ok(matches!(state, CoreState::Running)
            .then(|| self.receipt.lock().unwrap().clone())
            .flatten())
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        let (state, _) = self.status.lock().unwrap().clone();
        match state {
            CoreState::Stopped(_) => {
                Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
            }
            CoreState::Running => {
                Ok(crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied)
            }
        }
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for StartupDegradedServiceCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        let attempt = self.reconciles.fetch_add(1, AtomicOrdering::AcqRel);
        if attempt == 0 {
            self.events.lock().unwrap().push("rebuild-service-failed");
            anyhow::bail!("injected Service reconciliation failure")
        }
        self.events.lock().unwrap().push("rebuild-local");
        Ok(())
    }

    async fn retire_service_core(
        &self,
        _expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("service-core-retire");
        self.stopped.store(true, AtomicOrdering::Release);
        Ok(())
    }

    async fn reconcile_after_service_release(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
    ) -> anyhow::Result<()> {
        self.events
            .lock()
            .unwrap()
            .push("rebuild-after-service-release");
        *self.selected.lock().unwrap() = RunType::Normal;
        self.stopped.store(false, AtomicOrdering::Release);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        *self.selected.lock().unwrap() = match host {
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local => RunType::Normal,
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service => RunType::Service,
        };
        self.events.lock().unwrap().push(match host {
            crate::core::actor_v2::control_endpoint::ExecutionHost::Local => "select-local",
            crate::core::actor_v2::control_endpoint::ExecutionHost::Service => "select-service",
        });
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let stopped = self.stopped.load(AtomicOrdering::Acquire);
        let run_type = *self.selected.lock().unwrap();
        Ok(CoreStatusSnapshot {
            instance_id: (run_type == RunType::Service).then(|| "service-core-instance".into()),
            state: if stopped {
                CoreState::Stopped(None)
            } else {
                CoreState::Running
            },
            state_changed_at: if stopped { 2 } else { 1 },
            run_type,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for StartupTransientCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        let attempt = self.reconciles.fetch_add(1, AtomicOrdering::AcqRel);
        if attempt == 0 {
            self.events
                .lock()
                .unwrap()
                .push("rebuild-transient-failure");
            return Err(anyhow::Error::new(chimera_core_manager::CoreError::new(
                // BackendUnavailable is a dependency wait in ref. This fixture
                // exercises the bounded application retry after a failed apply.
                chimera_core_manager::CoreErrorKind::ApplyFailed,
                "temporary runtime apply failure",
                true,
            )));
        }
        self.events.lock().unwrap().push("rebuild-recovered");
        self.recovered.notify_one();
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let attempt = self.stops.fetch_add(1, AtomicOrdering::AcqRel);
        if attempt == 1
            && let Some((started, release)) = &self.stop_gate
        {
            started.notify_one();
            release.notified().await;
        }
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let reconciles = self.reconciles.load(AtomicOrdering::Acquire);
        let stopped = self.stops.load(AtomicOrdering::Acquire) > 0 && reconciles < 2;
        Ok(CoreStatusSnapshot {
            instance_id: Some("local-core-instance".into()),
            state: if stopped {
                CoreState::Stopped(None)
            } else {
                CoreState::Running
            },
            state_changed_at: if stopped { 2 } else { 1 },
            run_type: RunType::Normal,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        let reconciles = self.reconciles.load(AtomicOrdering::Acquire);
        let stopped = self.stops.load(AtomicOrdering::Acquire) > 0 && reconciles < 2;
        Ok(if stopped {
            crate::client::application_workflow::mutation::KnownRuntimeState::Stopped
        } else {
            crate::client::application_workflow::mutation::KnownRuntimeState::NeverApplied
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl ServiceLifecyclePort for RestartBudgetService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.events.lock().unwrap().push("service-probe");
        Ok(chimera_ipc::types::StatusInfo {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed("1.0.0"),
            status: chimera_ipc::types::ServiceStatus::Stopped,
            server: None,
        })
    }

    async fn report_endpoint_down(
        &self,
        _transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("endpoint-down");
        let attempts = self.attempts.load(AtomicOrdering::Acquire);
        if attempts >= 3 {
            self.exhausted.store(true, AtomicOrdering::Release);
        } else {
            self.attempts
                .store(attempts.saturating_add(1), AtomicOrdering::Release);
        }
        Ok(())
    }

    fn restart_policy(&self) -> ServiceRestartPolicySnapshot {
        ServiceRestartPolicySnapshot {
            attempts: self.attempts.load(AtomicOrdering::Acquire),
            exhausted: self.exhausted.load(AtomicOrdering::Acquire),
        }
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(RecordingServiceTransition {
            events: self.events.clone(),
        }))
    }
}

#[async_trait]
impl ServiceLifecyclePort for StartupRetryLeaseService {
    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        self.inner.probe().await
    }

    fn restart_policy(&self) -> ServiceRestartPolicySnapshot {
        self.inner.restart_policy()
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        let attempt = self
            .transition_attempts
            .fetch_add(1, AtomicOrdering::AcqRel);
        if attempt == 1 {
            self.inner
                .events
                .lock()
                .unwrap()
                .push("startup-retry-lease-failed");
            if let Some((started, release)) = &self.retry_lease_gate {
                started.notify_one();
                release.notified().await;
            }
            anyhow::bail!("test Service lease failure");
        }
        self.inner.begin_transition().await
    }
}

struct FailingInstaller {
    events: Arc<Mutex<Vec<&'static str>>>,
}

struct RecordingProgress {
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[async_trait]
impl ports::BinaryInstaller for RecordingInstaller {
    async fn install(
        &self,
        artifact: &chimera_contracts::core_install::CoreBinaryInstallRequest,
    ) -> Result<(), chimera_contracts::core_install::InstallCoreBinaryError> {
        // Verify the lifecycle forwards the staged input to the upstream
        // contract without introducing unsupported companion assets.
        assert!(artifact.source.starts_with(artifact.staging.path()));
        assert!(artifact.companion.is_none());
        assert!(!artifact.destination.as_os_str().is_empty());
        self.events.lock().unwrap().push("install");
        Ok(())
    }
}

#[async_trait]
impl ports::BinaryInstaller for FailingInstaller {
    async fn install(
        &self,
        artifact: &chimera_contracts::core_install::CoreBinaryInstallRequest,
    ) -> Result<(), chimera_contracts::core_install::InstallCoreBinaryError> {
        self.events.lock().unwrap().push("install");
        Err(
            chimera_contracts::core_install::InstallCoreBinaryError::ElevatedCopyFailed {
                core: artifact.target,
                destination: (&artifact.destination).into(),
                exit_code: Some(1),
            },
        )
    }
}

impl BinaryInstallProgress for RecordingProgress {
    fn restarting(&self) {
        self.events.lock().unwrap().push("restarting");
    }

    fn finished(&self, error: Option<&str>) {
        self.events.lock().unwrap().push(if error.is_some() {
            "finished-error"
        } else {
            "finished"
        });
    }
}

#[async_trait]
impl CoreLifecyclePort for CoreUpdateRecordingCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("stop");
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("select");
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: self.state.clone(),
            state_changed_at: self.state_changed_at,
            run_type: RunType::Normal,
        })
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        if self.receipt_error {
            anyhow::bail!("active core status is unavailable");
        }
        let Some(kind) = self.active_kind else {
            return Ok(None);
        };
        if !matches!(self.state, CoreState::Running) {
            return Ok(None);
        }
        let target_core = match kind {
            chimera_core_manager::CoreKind::Mihomo => ClashCore::Mihomo,
            chimera_core_manager::CoreKind::ClashRust => ClashCore::ClashRs,
            chimera_core_manager::CoreKind::ClashPremium => ClashCore::ClashPremium,
            chimera_core_manager::CoreKind::ChimeraClient => ClashCore::ChimeraClient,
            chimera_core_manager::CoreKind::Meow => return Ok(None),
        };
        let mut receipt = crate::client::ports::test_runtime_receipt_with_mixed_port(7892);
        receipt.target_core = target_core;
        receipt.core_spec.kind = kind;
        Ok(Some(Arc::new(receipt)))
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for PanicOnStopCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild-after-panic");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("panic-stop");
        panic!("injected lifecycle panic");
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("select-after-panic");
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for BindingCore {
    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        self.running.store(true, AtomicOrdering::Release);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, AtomicOrdering::Release);
        Ok(())
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        self.running.store(true, AtomicOrdering::Release);
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: if self.running.load(AtomicOrdering::Acquire) {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: 1,
            run_type: RunType::Normal,
        })
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        Ok(self.running.load(AtomicOrdering::Acquire).then(|| {
            Arc::new(crate::client::ports::test_runtime_receipt_with_mixed_port(
                7892,
            ))
        }))
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        crate::config::clash::ClashInfo {
            port: 7892,
            server: "127.0.0.1".into(),
            secret: None,
        }
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for LowerUncertainCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("lower-rebuild");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("lower-stop");
        self.uncertain.store(true, AtomicOrdering::Release);
        anyhow::bail!("lower mutation reply lost")
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("lower-select");
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    fn outcome_uncertain(&self) -> bool {
        self.uncertain.load(AtomicOrdering::Acquire)
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for RecordingCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("stop-start");
        self.stop_started.notify_one();
        self.release_stop.notified().await;
        self.events.lock().unwrap().push("stop-end");
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("select");
        Ok(())
    }
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: if self.run_type == RunType::Service {
                1
            } else {
                0
            },
            run_type: self.run_type,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        Some(self.recovery_notify.clone())
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ConfirmedStoppedCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("stop");
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("select");
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            run_type: self.run_type,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ProfileTccCore {
    async fn reconcile_prepared(
        &self,
        prepared: ports::PreparedRuntime,
        _: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        let profiles = submitted_profiles(&prepared)?;
        self.attempts.fetch_add(1, AtomicOrdering::AcqRel);
        if self.fail_next_reconcile.swap(false, AtomicOrdering::AcqRel) {
            anyhow::bail!("injected current Profiles reconcile failure");
        }
        *self.active_profiles.lock().unwrap() = Some(profiles);
        Ok(())
    }

    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn reconcile_profiles(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
        profiles: Arc<chimera_config::profile::Profiles>,
        _app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
        _target: Option<String>,
    ) -> anyhow::Result<()> {
        self.attempts.fetch_add(1, AtomicOrdering::AcqRel);
        if self.fail_next_reconcile.swap(false, AtomicOrdering::AcqRel) {
            anyhow::bail!("injected current Profiles reconcile failure");
        }
        *self.active_profiles.lock().unwrap() = Some((*profiles).clone());
        Ok(())
    }

    async fn validate_profile_runtime(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
        _profiles: Arc<chimera_config::profile::Profiles>,
        _app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        self.validations.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(crate::client::application_workflow::mutation::CheckRecord::Passed)
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.stops.fetch_add(1, AtomicOrdering::SeqCst);
        *self.active_profiles.lock().unwrap() = None;
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            run_type: RunType::Normal,
        })
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        Ok(crate::client::application_workflow::mutation::KnownRuntimeState::Stopped)
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ServiceHandoffCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("rebuild-local");
        self.local.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Running,
            state_changed_at: 0,
            run_type: if self.local.load(std::sync::atomic::Ordering::Relaxed) {
                RunType::Normal
            } else {
                RunType::Service
            },
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ServiceModeRecordingCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        let app = self.application.get_typed();
        self.observations
            .lock()
            .unwrap()
            .push((run_type, target_core, app.enable_service_mode));
        if self
            .mutate_application_on_first_reconcile
            .swap(false, AtomicOrdering::AcqRel)
        {
            self.application
                .patch_typed(ChimeraAppConfigPatch {
                    core: Some(ClashCore::ClashRsAlpha),
                    enable_builtin_enhanced: Some(false),
                    ..Default::default()
                })
                .await?;
        }
        if self
            .fail_first_reconcile
            .swap(false, AtomicOrdering::AcqRel)
        {
            anyhow::bail!("injected Service Mode reconcile failure");
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[async_trait]
impl CoreLifecyclePort for ApplicationPatchRecordingCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn validate_profile_runtime(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
        _profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        // A real advisory preflight must be read-only: the candidate is
        // new, while the committed source stays at its previous value.
        self.observations.lock().unwrap().push((
            app.enable_builtin_enhanced,
            self.application.get_typed().enable_builtin_enhanced,
        ));
        if self.reject_check {
            anyhow::bail!("injected independent runtime candidate rejection");
        }
        Ok(crate::client::application_workflow::mutation::CheckRecord::Passed)
    }

    async fn reconcile(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
    ) -> anyhow::Result<()> {
        anyhow::bail!("application patch must pass its typed runtime candidate")
    }

    async fn reconcile_profiles(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
        _profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
        _target: Option<String>,
    ) -> anyhow::Result<()> {
        self.observations.lock().unwrap().push((
            app.enable_builtin_enhanced,
            self.application.get_typed().enable_builtin_enhanced,
        ));
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _core: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _break_when: bool) {}
}

#[tokio::test]
async fn service_mode_config_patch_commits_before_runtime_reconcile() {
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ServiceModeRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            mutate_application_on_first_reconcile: AtomicBool::new(false),
            fail_first_reconcile: AtomicBool::new(false),
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );

    workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

    assert_eq!(
        *observations.lock().unwrap(),
        [(
            RunType::Service,
            chimera_config::application::ChimeraAppConfig::default().core,
            true
        )]
    );
    assert!(application.get_typed().enable_service_mode);
}

#[tokio::test]
async fn builtin_enhancement_patch_checks_before_commit_and_applies_after() {
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let profiles_dir = tempfile::tempdir().unwrap();
    let profiles_path =
        camino::Utf8PathBuf::from_path_buf(profiles_dir.path().join("profiles.yaml")).unwrap();
    let profiles_manager = chimera_core::state::PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(profiles_path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        Some(profiles_manager.snapshot_handle()),
        Arc::new(ApplicationPatchRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            reject_check: false,
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );

    workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_builtin_enhanced: Some(false),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();

    assert_eq!(
        *observations.lock().unwrap(),
        [(false, true), (false, false)],
        "advisory check sees the old source; Core apply sees the committed source"
    );
    assert!(!application.get_typed().enable_builtin_enhanced);
}

#[tokio::test]
async fn rejected_advisory_application_check_does_not_commit_or_touch_core() {
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let dir = tempfile::tempdir().unwrap();
    let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let profiles = chimera_core::state::PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        Some(profiles.snapshot_handle()),
        Arc::new(ApplicationPatchRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            reject_check: true,
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    let result = workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_builtin_enhanced: Some(false),
                ..Default::default()
            },
            false,
        )
        .await;
    let error = result.unwrap_err();
    assert!(format!("{error:#}").contains("injected independent runtime candidate rejection"));
    assert!(
        matches!(
            error
                .downcast_ref::<Arc<chimera_contracts::runtime_error::RuntimeError>>()
                .map(Arc::as_ref),
            Some(chimera_contracts::runtime_error::RuntimeError::CheckUnavailable { .. })
        ),
        "an untyped validator error has no authoritative Core rejection verdict"
    );
    assert!(application.get_typed().enable_builtin_enhanced);
    assert_eq!(
        *observations.lock().unwrap(),
        [(false, true)],
        "preflight runs once against the old committed source, without Core apply"
    );
}

#[tokio::test]
async fn application_persistence_failure_after_preflight_never_applies_core() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("application-config");
    std::fs::create_dir(&config_dir).unwrap();
    let application = ApplicationClient::new(
        camino::Utf8PathBuf::from_path_buf(config_dir.join("application.yaml")).unwrap(),
        chimera_config::application::ChimeraAppConfig::default(),
        Arc::new(TestApplicationBridge),
        chimera_config::application::ReleaseChannel::Stable,
    )
    .await
    .unwrap();
    let old = application.get().await.unwrap();
    let profiles = chimera_core::state::PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(camino::Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap())
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        Some(profiles.snapshot_handle()),
        Arc::new(ApplicationPatchRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            reject_check: false,
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );
    let moved = dir.path().join("hidden-application-config");
    std::fs::rename(&config_dir, &moved).unwrap();
    let result = workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_builtin_enhanced: Some(false),
                ..Default::default()
            },
            false,
        )
        .await;
    assert!(format!("{:#}", result.unwrap_err()).contains("commit"));
    let now = application.get().await.unwrap();
    assert_eq!(now.version, old.version);
    assert_eq!(
        now.state.enable_builtin_enhanced,
        old.state.enable_builtin_enhanced
    );
    assert_eq!(
        *observations.lock().unwrap(),
        [(false, true)],
        "successful advisory preflight must never apply Core if persistence fails"
    );
    std::fs::rename(moved, config_dir).unwrap();
}

#[tokio::test]
async fn app_commit_superseded_during_apply_defers_latest_reconcile() {
    let directory = tempfile::tempdir().unwrap();
    let config_path =
        camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
    let application = ApplicationClient::new(
        config_path,
        chimera_config::application::ChimeraAppConfig::default(),
        Arc::new(TestApplicationBridge),
        chimera_config::application::ReleaseChannel::Stable,
    )
    .await
    .unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ServiceModeRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            mutate_application_on_first_reconcile: AtomicBool::new(true),
            fail_first_reconcile: AtomicBool::new(false),
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );

    let degradations = workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

    assert_eq!(degradations.len(), 1);
    assert_eq!(degradations[0].code, "application_runtime_deferred");
    assert!(degradations[0].message.contains("source advanced"));
    assert!(degradations[0].retryable);
    assert_eq!(
        *observations.lock().unwrap(),
        [(
            RunType::Service,
            chimera_config::application::ChimeraAppConfig::default().core,
            true
        )]
    );
    let latest = application.get_typed();
    assert!(latest.enable_service_mode);
    assert!(!latest.enable_builtin_enhanced);
    assert_eq!(latest.core, ClashCore::ClashRsAlpha);
}

#[tokio::test]
async fn postcommit_runtime_failure_returns_degradation_without_rolling_back_source() {
    let directory = tempfile::tempdir().unwrap();
    let config_path =
        camino::Utf8PathBuf::from_path_buf(directory.path().join("application.yaml")).unwrap();
    let application = ApplicationClient::new(
        config_path,
        chimera_config::application::ChimeraAppConfig::default(),
        Arc::new(TestApplicationBridge),
        chimera_config::application::ReleaseChannel::Stable,
    )
    .await
    .unwrap();
    let observations = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        application.clone(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ServiceModeRecordingCore {
            application: application.clone(),
            observations: observations.clone(),
            mutate_application_on_first_reconcile: AtomicBool::new(true),
            fail_first_reconcile: AtomicBool::new(true),
        }),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
    );

    let degradations = workflow
        .patch_application_config(
            ChimeraAppConfigPatch {
                enable_service_mode: Some(true),
                ..Default::default()
            },
            true,
        )
        .await
        .unwrap();

    assert_eq!(degradations.len(), 1);
    assert_eq!(degradations[0].code, "application_runtime_deferred");
    assert!(
        degradations[0]
            .message
            .contains("injected Service Mode reconcile failure")
    );
    assert_eq!(
        *observations.lock().unwrap(),
        [(
            RunType::Service,
            chimera_config::application::ChimeraAppConfig::default().core,
            true
        )]
    );
    assert!(application.get_typed().enable_service_mode);
    assert!(!application.get_typed().enable_builtin_enhanced);
    assert_eq!(application.get_typed().core, ClashCore::ClashRsAlpha);
}

#[tokio::test]
async fn mailbox_serializes_stop_and_core_selection() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: stop_started.clone(),
            release_stop: release_stop.clone(),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await.unwrap() });
    if tokio::time::timeout(Duration::from_secs(5), stop_started.notified())
        .await
        .is_err()
    {
        release_stop.notify_one();
        stop.abort();
        panic!("stop command did not reach the lifecycle actor");
    }
    let ClientInner::Actor { actor_ref, .. } = client.0.as_ref() else {
        panic!("production lifecycle client should own an actor");
    };
    let (reply, result) = tokio::sync::oneshot::channel();
    if actor_ref
        .send_message(Message::Request(Request {
            command: Command::SelectCore(ClashCore::Mihomo),
            response: Response {
                id: 2,
                reply: Some(reply.into()),
            },
            deferred_retry: None,
        }))
        .is_err()
    {
        release_stop.notify_one();
        panic!("failed to queue core selection");
    }
    release_stop.notify_one();
    tokio::time::timeout(Duration::from_secs(5), async {
        stop.await.unwrap();
        result.await.expect("selection reply was dropped").unwrap();
    })
    .await
    .expect("lifecycle mailbox did not finish queued commands");

    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end", "select"]
    );
}

#[tokio::test]
async fn lifecycle_reconcile_and_shutdown_publish_active_runtime_binding() {
    let runtime_bindings = Arc::new(RecordingRuntimeBindings::default());
    let core = Arc::new(BindingCore {
        running: AtomicBool::new(false),
    });
    let client = RuntimeReconcilerClient::spawn(
        core,
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    let mutations = crate::state::mutation::MutationCoordinator::pending_with_notifications(
        runtime_bindings.clone(),
    );
    mutations.connect(client.clone()).await.unwrap();

    client.select_core(ClashCore::Mihomo).await.unwrap();
    assert_eq!(*runtime_bindings.ports.lock().unwrap(), [Some(7892)]);

    client.shutdown().await.unwrap();
    assert_eq!(
        *runtime_bindings.ports.lock().unwrap(),
        [Some(7892), None],
        "stopping the active core must clear its published port"
    );
}

#[tokio::test]
async fn stop_service_hands_core_back_to_local_inside_lifecycle_mailbox() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let local = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut app_state = chimera_config::application::ChimeraAppConfig::default();
    app_state.enable_service_mode = true;
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(ServiceHandoffCore {
            events: events.clone(),
            local: local.clone(),
        }),
        ApplicationClient::static_state(app_state),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();

    client.stop_service().await.unwrap();

    assert!(local.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "service-begin",
            "service-stop",
            "service-confirm",
            "rebuild-local",
            "service-probe"
        ]
    );
}

#[tokio::test]
async fn service_control_commands_share_the_lifecycle_actor() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();

    client.install_service().await.unwrap();
    client.update_service().await.unwrap();
    client.start_service(Duration::from_secs(1)).await.unwrap();
    client
        .restart_service(Duration::from_secs(1))
        .await
        .unwrap();
    client.uninstall_service().await.unwrap();
    client.stop_service().await.unwrap();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "service-begin",
            "service-install",
            "service-probe",
            "service-begin",
            "service-update",
            "service-probe",
            "service-begin",
            "service-start",
            "service-probe",
            "service-begin",
            "service-restart",
            "service-probe",
            "service-begin",
            "service-uninstall",
            "service-confirm",
            "service-probe",
            "service-begin",
            "service-stop",
            "service-confirm",
            "service-probe"
        ]
    );
}

#[tokio::test]
async fn probe_service_reads_through_lifecycle_mailbox() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();

    let status = client.probe_service().await.unwrap();

    assert_eq!(status.status, chimera_ipc::types::ServiceStatus::Stopped);
    assert_eq!(status.phase, ServicePhase::DaemonStopped);
    assert_eq!(status.name.as_ref(), "chimera-service");
    assert_eq!(client.service_status().phase, ServicePhase::DaemonStopped);
    assert_eq!(events.lock().unwrap().as_slice(), ["service-probe"]);
}

#[tokio::test]
async fn health_observation_updates_cached_service_status_without_reprobe() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();

    assert_eq!(client.service_status().phase, ServicePhase::Probing);
    client.observe_service_status(chimera_ipc::types::StatusInfo {
        name: std::borrow::Cow::Borrowed("chimera-service"),
        version: std::borrow::Cow::Borrowed("1.0.0"),
        status: chimera_ipc::types::ServiceStatus::Stopped,
        server: None,
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.service_status().phase == ServicePhase::DaemonStopped {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("health observation should update cached service status");
    assert!(events.lock().unwrap().is_empty());

    client.observe_service_probe_failure();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.service_status().phase == ServicePhase::Unknown {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("probe failure should publish unknown service phase");
    let status = client.service_status();
    assert_eq!(status.status, chimera_ipc::types::ServiceStatus::Stopped);
    assert!(!status.runtime_owned);
    assert!(events.lock().unwrap().is_empty());
}

#[tokio::test]
async fn start_and_restart_service_confirm_ready_before_returning() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app_state = chimera_config::application::ChimeraAppConfig::default();
    app_state.enable_service_mode = true;
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(ServiceHandoffCore {
            events: events.clone(),
            local: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }),
        ApplicationClient::static_state(app_state),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();

    client.start_service(Duration::from_secs(1)).await.unwrap();
    client
        .restart_service(Duration::from_secs(1))
        .await
        .unwrap();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "service-begin",
            "service-start",
            "service-ready",
            "service-ready",
            "service-probe",
            "service-begin",
            "service-restart",
            "service-ready",
            "service-ready",
            "service-probe"
        ]
    );
}

#[tokio::test]
async fn shutdown_is_idempotent_and_blocks_follow_up_mutations() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let release_stop = Arc::new(Notify::new());
    release_stop.notify_one();
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop,
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    client.shutdown().await.unwrap();
    client.shutdown().await.unwrap();

    assert!(client.status().shutting_down);
    let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("core lifecycle is shutting down")
    );
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end"]
    );
}

#[tokio::test]
async fn shutdown_drains_pending_queue_before_final_stop() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: stop_started.clone(),
            release_stop: release_stop.clone(),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await });
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("first stop should become the active operation");

    let select_client = client.clone();
    let select = tokio::spawn(async move { select_client.select_core(ClashCore::Mihomo).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !client.status().queued.contains(&2) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("core selection should be queued behind the active stop");

    let shutdown_client = client.clone();
    let shutdown = tokio::spawn(async move { shutdown_client.shutdown().await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while !client.status().shutting_down {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("shutdown should close and drain the pending queue");

    let error = select
        .await
        .expect("drained selection should settle")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("core lifecycle is shutting down")
    );
    assert!(
        !shutdown.is_finished(),
        "shutdown must wait for the active stop"
    );

    release_stop.notify_one();
    stop.await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("shutdown should run one final stop after the active operation");
    assert!(!shutdown.is_finished(), "shutdown reply follows final stop");

    release_stop.notify_one();
    shutdown.await.unwrap().unwrap();
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end", "stop-start", "stop-end"]
    );
}

#[tokio::test]
async fn pending_queue_rejects_overflow_and_finishes_rejected_binary_update() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: stop_started.clone(),
            release_stop: release_stop.clone(),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await });
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("stop should enter the lifecycle actor");

    let mut queued = Vec::new();
    for _ in 0..MAX_PENDING {
        let queued_client = client.clone();
        queued.push(tokio::spawn(async move {
            queued_client.select_core(ClashCore::Mihomo).await
        }));
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.status().queued.len() == MAX_PENDING {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending lifecycle queue should fill");

    let progress_events = Arc::new(Mutex::new(Vec::new()));
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let error = client
        .replace_core_binary(PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source: staging.path().join("staged-core"),
            destination: staging.path().join("installed-core"),
            staging,
            progress: Arc::new(RecordingProgress {
                events: progress_events.clone(),
            }),
        })
        .await
        .unwrap_err();
    assert!(error.to_string().contains("core lifecycle queue is full"));
    assert_eq!(
        progress_events.lock().unwrap().as_slice(),
        ["finished-error"]
    );
    assert!(
        client
            .status()
            .completed
            .iter()
            .any(|entry| entry.error.as_deref() == Some("core lifecycle queue is full"))
    );

    release_stop.notify_one();
    stop.await.unwrap().unwrap();
    for request in queued {
        request.await.unwrap().unwrap();
    }

    assert!(client.status().queued.is_empty());
    let events = events.lock().unwrap();
    assert_eq!(
        events.iter().filter(|event| **event == "select").count(),
        MAX_PENDING
    );
}

/// A full lifecycle queue may not erase a committed source's runtime
/// intent. When capacity returns, the Actor must apply the latest source
/// without needing a second SourceCommitted notification.
#[tokio::test]
async fn committed_source_dirty_survives_full_lifecycle_queue() {
    let core = Arc::new(PausedFirstReconcileCore {
        entered: Arc::new(Notify::new()),
        resume: Arc::new(Notify::new()),
        calls: AtomicUsize::new(0),
    });
    let client = RuntimeReconcilerClient::spawn(
        core.clone(),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    client.request_runtime_rebuild();
    tokio::time::timeout(Duration::from_secs(3), core.entered.notified())
        .await
        .expect("first reconcile should hold the sole Core executor");
    assert_eq!(core.calls.load(AtomicOrdering::Acquire), 1);

    let mut queued = Vec::new();
    for _ in 0..MAX_PENDING {
        let queued_client = client.clone();
        queued.push(tokio::spawn(async move {
            queued_client.select_core(ClashCore::Mihomo).await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if client.status().queued.len() == MAX_PENDING {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Actor mailbox must be at capacity before committing source");

    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    assert_eq!(core.calls.load(AtomicOrdering::Acquire), 1);

    core.resume.notify_one();
    for task in queued {
        task.await.unwrap().unwrap();
    }
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = client.reconciler_status();
            if core.calls.load(AtomicOrdering::Acquire) == 2
                && status
                    .recent
                    .iter()
                    .any(|round| round.trigger == "SourceCommitted")
                && status.active.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        let status = client.reconciler_status();
        panic!(
            "latest source lost after queue pressure: calls={}, recent={:?}, active={:?}, queued={:?}",
            core.calls.load(AtomicOrdering::Acquire),
            status.recent.iter().map(|round| &round.trigger).collect::<Vec<_>>(),
            client.status().active,
            client.status().queued,
        )
    });
    assert_eq!(core.calls.load(AtomicOrdering::Acquire), 2);
    assert_eq!(
        client
            .reconciler_status()
            .recent
            .iter()
            .filter(|round| round.trigger == "SourceCommitted")
            .count(),
        1,
        "queue recovery must not duplicate the source reconciliation"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_reconcile_waits_for_lifecycle_completion() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client
        .startup_reconcile()
        .await
        .expect("startup reconcile should complete through the lifecycle actor");
    completed
        .result
        .expect("startup reconcile should return its terminal operation result");
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    assert!(events.lock().unwrap().contains(&"rebuild"));
}

#[tokio::test]
async fn startup_recovery_keeps_a_receipt_only_for_the_latest_committed_target() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(StartupReceiptCore {
        events: events.clone(),
        status: Mutex::new((CoreState::Running, 1)),
        receipt: Mutex::new(None),
        stop_confirms: true,
    });
    let application = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let service: Arc<dyn ServiceLifecyclePort> = Arc::new(RecordingService {
        events: events.clone(),
    });
    let target = startup_target_for(
        application.clone(),
        clash.clone(),
        core.clone(),
        service.clone(),
    )
    .await;
    *core.receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
    let client =
        spawn_startup_client_with_application_and_clash(core, service, application, clash).await;

    client
        .recover_startup()
        .await
        .expect("matching recovery receipt should be accepted");

    let events = events.lock().unwrap();
    assert!(!events.contains(&"stop"), "events: {events:?}");
    assert!(!events.contains(&"rebuild"), "events: {events:?}");
    assert!(!client.status().uncertain);
}

#[tokio::test]
async fn initial_startup_retires_a_matching_running_receipt_before_reconcile() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(StartupReceiptCore {
        events: events.clone(),
        status: Mutex::new((CoreState::Running, 1)),
        receipt: Mutex::new(None),
        stop_confirms: true,
    });
    let application = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let service: Arc<dyn ServiceLifecyclePort> = Arc::new(RecordingService {
        events: events.clone(),
    });
    let target = startup_target_for(
        application.clone(),
        clash.clone(),
        core.clone(),
        service.clone(),
    )
    .await;
    *core.receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
    let client =
        spawn_startup_client_with_application_and_clash(core, service, application, clash).await;

    let completed = client
        .startup_reconcile()
        .await
        .expect("initial startup should stop and rebuild the live target");
    let events = events.lock().unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    let stop = events.iter().position(|event| *event == "stop").unwrap();
    let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
    assert!(stop < rebuild);
}

/// ST-LIFE-01; isolated lifecycle integration using mock Local/Service ports.
/// Given Service and Local both report a running Core, when startup resolves
/// to Local, Service release and a confirmed Local stop must precede apply.
/// This catches owner-plan branches that skip the shared S3/S4 path.
#[tokio::test]
async fn startup_retires_service_then_stops_running_local_owner_before_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: None,
        runtime_owned: true,
    });
    let client = spawn_startup_client(core, service).await;

    let completed = client.startup_reconcile().await.unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);

    let events = events.lock().unwrap();
    let retire_service = events
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    let stop_local = events
        .iter()
        .position(|event| *event == "stop-local")
        .unwrap();
    let apply_local = events
        .iter()
        .position(|event| *event == "rebuild-local")
        .unwrap();
    assert!(retire_service < stop_local && stop_local < apply_local);
}

/// ST-LIFE-02; isolated lifecycle integration with a matching Recovery receipt.
/// Given Service release leaves the same Local runtime and exact target,
/// recovery may keep that receipt; it must not stop or rebuild Local.
#[tokio::test]
async fn recovery_keeps_local_owner_after_service_retirement_only_with_matching_receipt() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: None,
        runtime_owned: true,
    });
    let application = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = startup_target_for(
        application.clone(),
        clash.clone(),
        core.clone(),
        service.clone(),
    )
    .await;
    *core.local_receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
    let client =
        spawn_startup_client_with_application_and_clash(core, service, application, clash).await;

    client.recover_startup().await.unwrap();
    assert!(!client.status().uncertain);

    let events = events.lock().unwrap();
    assert!(events.contains(&"service-core-retire"));
    assert!(!events.contains(&"stop-local"));
    assert!(!events.contains(&"rebuild-local"));
}

/// Contract: a running Local Core follows Service readiness changes in
/// both directions, with owner handoff completed before the new host runs.
/// This actor/port integration uses only mock Core and Service hosts; an
/// implementation that ignores readiness changes fails the owner/count
/// assertions, and an unsafe handoff fails the recorded event ordering.
#[tokio::test]
async fn startup_follows_service_readiness_in_both_directions() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_ready = Arc::new(AtomicBool::new(false));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: service_ready.clone(),
        core_running: service_running.clone(),
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let client = spawn_startup_client_with_application(
        core.clone(),
        service,
        ApplicationClient::static_state(app),
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert_eq!(initial.report.outcome, StartupOutcome::Ready);
    assert_eq!(core.reconciles.load(AtomicOrdering::Acquire), 1);

    service_ready.store(true, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = client.core_status().await.unwrap();
            if core.reconciles.load(AtomicOrdering::Acquire) >= 2
                && status.run_type == RunType::Service
                && matches!(status.state, CoreState::Running)
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Service readiness should move the running Core to Service");

    let first_events = events.lock().unwrap().clone();
    let stop_local = first_events
        .iter()
        .position(|event| *event == "stop-local")
        .expect("Local must be stopped before handing off");
    let rebuild_service = first_events
        .iter()
        .position(|event| *event == "rebuild-service")
        .expect("the committed runtime should be applied on Service");
    assert!(stop_local < rebuild_service);
    assert!(!first_events.contains(&"service-start"));
    assert!(!first_events.contains(&"service-install"));

    service_running.store(false, AtomicOrdering::Release);
    service_ready.store(false, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = client.core_status().await.unwrap();
            if core.reconciles.load(AtomicOrdering::Acquire) >= 3
                && status.run_type == RunType::Normal
                && matches!(status.state, CoreState::Running)
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Service becoming unavailable should hand the Core back to Local");

    {
        let events = events.lock().unwrap();
        let rebuild_service = events
            .iter()
            .position(|event| *event == "rebuild-service")
            .unwrap();
        let select_local = events
            .iter()
            .rposition(|event| *event == "select-local")
            .expect("Local should be selected after Service becomes unavailable");
        let rebuild_local = events
            .iter()
            .rposition(|event| *event == "rebuild-local")
            .expect("the committed runtime should be re-established locally");
        assert!(rebuild_service < select_local && select_local < rebuild_local);
    }
    client.shutdown().await.unwrap();
}

/// Ref S2: an already selected Local host still owes retirement of a
/// second owned Service Core. A mere desired!=current-host check misses it.
#[tokio::test]
async fn service_readiness_retires_second_owned_core_while_local_is_preferred() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_ready = Arc::new(AtomicBool::new(false));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: service_ready.clone(),
        core_running: service_running.clone(),
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: false,
        ..Default::default()
    };
    let client = spawn_startup_client_with_application(
        core.clone(),
        service,
        ApplicationClient::static_state(app),
    )
    .await;
    let startup = client.startup_reconcile().await.unwrap();
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    let baseline = events.lock().unwrap().len();
    // Simulate a previously owned daemon Core becoming visible while
    // Local remains the user's selected host.
    service_ready.store(true, AtomicOrdering::Release);
    service_running.store(true, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if events
                .lock()
                .unwrap()
                .iter()
                .skip(baseline)
                .any(|event| *event == "service-core-retire")
                && !service_running.load(AtomicOrdering::Acquire)
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("ref S2 must retire the unexpected owned Service Core");
    assert_eq!(
        client.core_status().await.unwrap().run_type,
        RunType::Normal
    );
    let trace = events.lock().unwrap().clone();
    let recent = &trace[baseline..];
    let retire = recent
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    if let Some(local_rebuild) = recent.iter().position(|event| *event == "rebuild-local") {
        assert!(
            retire < local_rebuild,
            "the Service Core must retire before Local is rebuilt"
        );
    }
    assert!(!recent.contains(&"select-service"));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn waiting_service_submission_never_retires_owned_core_for_local_fallback() {
    use chimera_core_manager::{CoreError, CoreErrorKind};

    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(PostcommitServiceFallbackCore {
        events: events.clone(),
        service_running: service_running.clone(),
        selected: Mutex::new(RunType::Service),
        local_running: AtomicBool::new(false),
        refused_service: AtomicBool::new(false),
        forbid_local: AtomicBool::new(false),
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: Arc::new(AtomicBool::new(true)),
        core_running: service_running.clone(),
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let target = workflow.startup_target_identity().await.unwrap();
    let failure = CoreError::new(
        CoreErrorKind::BackendUnavailable,
        "Service still busy",
        false,
    );
    let error = workflow
        .fallback_after_service_refusal(&target, failure.clone().into())
        .await
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<CoreError>().and_then(|err| err.kind),
        Some(CoreErrorKind::BackendUnavailable)
    );
    assert!(service_running.load(AtomicOrdering::Acquire));
    assert!(!core.local_running.load(AtomicOrdering::Acquire));
    assert!(
        events.lock().unwrap().is_empty(),
        "Waiting may not acquire the Service release lease or mutate the owner"
    );
}

#[tokio::test]
async fn postcommit_service_refusal_retires_owned_core_before_local_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(PostcommitServiceFallbackCore {
        events: events.clone(),
        service_running: service_running.clone(),
        selected: Mutex::new(RunType::Service),
        local_running: AtomicBool::new(false),
        refused_service: AtomicBool::new(false),
        forbid_local: AtomicBool::new(false),
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: Arc::new(AtomicBool::new(true)),
        core_running: service_running.clone(),
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app.clone()),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let target = workflow.startup_target_identity().await.unwrap();
    let refusal = core
        .reconcile(ClashConfig::default(), app.core, RunType::Service)
        .await
        .unwrap_err();
    assert!(matches!(
        core.status().await.unwrap().state,
        CoreState::Stopped(_)
    ));
    workflow
        .fallback_after_service_refusal(&target, refusal)
        .await
        .expect("a safely retired Service owner allows one Local application");

    let trace = events.lock().unwrap();
    let failed = trace
        .iter()
        .position(|event| *event == "service-apply-refused")
        .unwrap();
    let retired = trace
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    let local = trace
        .iter()
        .position(|event| *event == "rebuild-after-service-release")
        .unwrap();
    assert!(failed < retired && retired < local);
    assert!(!service_running.load(AtomicOrdering::Acquire));
    assert!(core.local_running.load(AtomicOrdering::Acquire));
    assert_eq!(core.status().await.unwrap().run_type, RunType::Normal);
}

#[tokio::test]
async fn postcommit_service_fallback_waits_on_lease_without_starting_local() {
    use crate::client::reconciler::attempt::{ServiceFallbackDeferred, waiting_dependency};

    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(PostcommitServiceFallbackCore {
        events: events.clone(),
        service_running: service_running.clone(),
        selected: Mutex::new(RunType::Service),
        local_running: AtomicBool::new(false),
        refused_service: AtomicBool::new(false),
        forbid_local: AtomicBool::new(false),
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let service = Arc::new(LeaseBlockedToggleService {
        inner: ToggleStartupServiceProbe {
            events: events.clone(),
            ready: Arc::new(AtomicBool::new(true)),
            core_running: service_running.clone(),
        },
        block_next: AtomicBool::new(true),
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app.clone()),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let target = workflow.startup_target_identity().await.unwrap();
    let refusal = core
        .reconcile(ClashConfig::default(), app.core, RunType::Service)
        .await
        .unwrap_err();
    let error = workflow
        .fallback_after_service_refusal(&target, refusal)
        .await
        .expect_err("fallback must wait while the Service owner lease is unavailable");
    assert!(error.downcast_ref::<ServiceFallbackDeferred>().is_some());
    assert!(waiting_dependency(&error));
    assert!(service_running.load(AtomicOrdering::Acquire));
    assert!(!core.local_running.load(AtomicOrdering::Acquire));
    assert!(!events.lock().unwrap().contains(&"service-core-retire"));

    workflow
        .fallback_after_service_refusal(&target, anyhow::anyhow!("same refused Service target"))
        .await
        .expect("another event can safely retry the owner release");
    assert!(!service_running.load(AtomicOrdering::Acquire));
    assert!(core.local_running.load(AtomicOrdering::Acquire));
}

#[tokio::test]
async fn service_failure_with_still_running_core_never_falls_back() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(PostcommitServiceFallbackCore {
        events: events.clone(),
        service_running: service_running.clone(),
        selected: Mutex::new(RunType::Service),
        local_running: AtomicBool::new(false),
        refused_service: AtomicBool::new(false),
        forbid_local: AtomicBool::new(false),
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(Default::default()),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(ToggleStartupServiceProbe {
            events: events.clone(),
            ready: Arc::new(AtomicBool::new(true)),
            core_running: service_running.clone(),
        }),
    );
    let target = workflow.startup_target_identity().await.unwrap();
    let error = workflow
        .fallback_after_service_refusal(&target, anyhow::anyhow!("application rejected"))
        .await
        .expect_err("still-running Service Core must not be retired on an unproven failure");
    assert!(error.to_string().contains("application rejected"));
    assert!(service_running.load(AtomicOrdering::Acquire));
    assert!(!core.local_running.load(AtomicOrdering::Acquire));
    assert!(!events.lock().unwrap().contains(&"service-core-retire"));
}

#[tokio::test]
async fn service_handoff_lease_failure_is_visible_and_clears_after_retry() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let ready = Arc::new(AtomicBool::new(false));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(LeaseBlockedToggleService {
        inner: ToggleStartupServiceProbe {
            events: events.clone(),
            ready: ready.clone(),
            core_running: service_running.clone(),
        },
        block_next: AtomicBool::new(true),
    });
    let client = spawn_startup_client_with_application(
        core,
        service,
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig {
            enable_service_mode: false,
            ..Default::default()
        }),
    )
    .await;
    assert_eq!(
        client.startup_reconcile().await.unwrap().report.outcome,
        StartupOutcome::Ready
    );

    ready.store(true, AtomicOrdering::Release);
    service_running.store(true, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client
                .reconciler_status()
                .maintenance
                .as_deref()
                .is_some_and(|reason| reason.contains("injected Service transition lease busy"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a failed owner lease must be published as pending maintenance");
    assert!(
        service_running.load(AtomicOrdering::Acquire),
        "the ownership lease failure must not stop the daemon's Core"
    );

    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if !service_running.load(AtomicOrdering::Acquire)
                && client.reconciler_status().maintenance.is_none()
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a successful subsequent owner transition should settle maintenance");
    assert_eq!(
        client.core_status().await.unwrap().run_type,
        RunType::Normal
    );
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "service-core-retire")
            .count(),
        1,
        "only the successful owner transition may retire Service"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn stop_returning_ok_without_stopped_proof_remains_waiting() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let events = Arc::new(Mutex::new(Vec::new()));
    let mut fake = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    fake.stop_confirms = false;
    let client = spawn_startup_client(Arc::new(fake), Arc::new(RecordingService { events })).await;
    client.stop_core().await.unwrap();
    let status = client.reconciler_status();
    assert!(
        matches!(status.conclusion, Conclusion::Waiting { .. }),
        "a successful stop call is not evidence that Core actually stopped"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn service_readiness_does_not_restart_core_after_explicit_stop() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let events = Arc::new(Mutex::new(Vec::new()));
    let service_ready = Arc::new(AtomicBool::new(false));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: service_ready.clone(),
        core_running: service_running,
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let client = spawn_startup_client_with_application(
        core.clone(),
        service,
        ApplicationClient::static_state(app),
    )
    .await;

    let startup = client.startup_reconcile().await.unwrap();
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    client.stop_core().await.unwrap();
    let before = core.reconciles.load(AtomicOrdering::SeqCst);
    assert!(matches!(
        client.reconciler_status().conclusion,
        Conclusion::Inactive
    ));
    service_ready.store(true, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    assert_eq!(
        core.reconciles.load(AtomicOrdering::SeqCst),
        before,
        "service readiness is not an explicit Start request"
    );
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|event| *event == "rebuild-service"),
        "stopped intent must prohibit a Service Core restart"
    );
    client.shutdown().await.unwrap();
}

/// Contract: a stopped Local snapshot without explicit stop intent still
/// follows Service readiness, as ref does when `stop_requested` is false.
/// This fixture mutates only its in-memory status to represent an external
/// runtime stop, unlike an explicit StopCore command.
#[tokio::test]
async fn service_readiness_follows_after_local_core_stops_without_explicit_intent() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_ready = Arc::new(AtomicBool::new(false));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: service_ready.clone(),
        core_running: service_running,
    });
    let app = chimera_config::application::ChimeraAppConfig {
        enable_service_mode: true,
        ..Default::default()
    };
    let client = spawn_startup_client_with_application(
        core.clone(),
        service,
        ApplicationClient::static_state(app),
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert_eq!(initial.report.outcome, StartupOutcome::Ready);
    let followup_start = events.lock().unwrap().len();
    *core.local_status.lock().unwrap() = (CoreState::Stopped(None), 2);
    service_ready.store(true, AtomicOrdering::Release);
    signal_service_readiness_change(&client);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = client.core_status().await.unwrap();
            if core.reconciles.load(AtomicOrdering::Acquire) >= 2
                && status.run_type == RunType::Service
                && matches!(status.state, CoreState::Running)
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("Service readiness should follow a stopped Local snapshot without stop intent");

    {
        let events = events.lock().unwrap();
        let followup_events = &events[followup_start..];
        let select_service = followup_events
            .iter()
            .position(|event| *event == "select-service")
            .expect("Service should become the selected owner");
        let rebuild_service = followup_events
            .iter()
            .position(|event| *event == "rebuild-service")
            .expect("the committed runtime should be applied on Service");
        assert!(select_service < rebuild_service);
        assert!(!followup_events.contains(&"stop-local"));
    }
    client.shutdown().await.unwrap();
}

/// ST-LIFE-03; isolated lifecycle integration using mock hosts.
/// Given startup adopts a ready Service while Local is running, it must
/// confirm Local stopped, select Service, retire any unreceipted Service
/// runtime, and only then apply. Event order is the regression oracle.
#[tokio::test]
async fn startup_adopts_service_then_stops_running_service_owner_before_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: None,
        runtime_owned: true,
    });
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.enable_service_mode = true;
    let client =
        spawn_startup_client_with_application(core, service, ApplicationClient::static_state(app))
            .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);

    let events = events.lock().unwrap();
    let stop_local = events
        .iter()
        .position(|event| *event == "stop-local")
        .unwrap();
    let select_service = events
        .iter()
        .position(|event| *event == "select-service")
        .unwrap();
    let retire_service = events
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    let apply_service = events
        .iter()
        .position(|event| *event == "rebuild-service")
        .unwrap();
    assert!(stop_local < select_service);
    assert!(select_service < retire_service && retire_service < apply_service);
}

/// ST-LIFE-04; isolated lifecycle integration with a matching Service receipt.
/// Given recovery adopts the ready Service and its live receipt matches the
/// committed target, it keeps the owner without stop/rebuild side effects.
#[tokio::test]
async fn recovery_keeps_adopted_service_owner_only_with_matching_receipt() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Stopped(None), 0),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: None,
        runtime_owned: true,
    });
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.enable_service_mode = true;
    let application = ApplicationClient::static_state(app);
    let clash = ClashConfigClient::legacy().unwrap();
    let target = startup_target_for(
        application.clone(),
        clash.clone(),
        core.clone(),
        service.clone(),
    )
    .await;
    *core.service_receipt.lock().unwrap() = Some(startup_test_receipt_for(
        Some(target),
        crate::core::actor_v2::control_endpoint::ExecutionHost::Service,
    ));
    let client =
        spawn_startup_client_with_application_and_clash(core, service, application, clash).await;

    client.recover_startup().await.unwrap();
    assert!(!client.status().uncertain);

    let events = events.lock().unwrap();
    assert!(events.contains(&"select-service"));
    assert!(!events.contains(&"service-core-retire"));
    assert!(!events.contains(&"rebuild-service"));
}

/// ST-LIFE-05; isolated receipt-adapter integration.
/// The coarse status remains Running with the same timestamp while the
/// adapter invalidates the rich receipt on re-read (the in-place Service
/// revision case); recovery must stop and rebuild instead of accepting it.
#[tokio::test]
async fn recovery_rechecks_rich_receipt_after_coarse_running_status() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(false));
    let mut core = startup_owner_core(events.clone(), (CoreState::Running, 1), service_running);
    core.invalidate_receipt_after_first_query = true;
    let core = Arc::new(core);
    let application = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = startup_target_for(
        application.clone(),
        clash.clone(),
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;
    *core.local_receipt.lock().unwrap() = Some(startup_test_receipt(Some(target)));
    let client = spawn_startup_client_with_application_and_clash(
        core,
        Arc::new(RecordingService {
            events: events.clone(),
        }),
        application,
        clash,
    )
    .await;

    client.recover_startup().await.unwrap();
    assert!(!client.status().uncertain);

    let events = events.lock().unwrap();
    let stop = events
        .iter()
        .position(|event| *event == "stop-local")
        .unwrap();
    let apply = events
        .iter()
        .position(|event| *event == "rebuild-local")
        .unwrap();
    assert!(stop < apply);
}

/// ST-LIFE-10; isolated lifecycle integration for an uncertain Service.
/// Given conditional stop returns StopUnconfirmed and the backend's later
/// projection is error-stopped, startup must isolate the owner without
/// trusting that projection; neither Local stop nor Local apply may run.
#[tokio::test]
async fn service_stop_unconfirmed_does_not_trust_error_stopped_projection() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    );
    core.service_retire_error = Some((chimera_core_manager::CoreErrorKind::StopUnconfirmed, false));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: Some("quarantined: stop unconfirmed".into()),
        runtime_owned: true,
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(core),
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service.clone(),
    );
    let mut startup_state = StartupWorkflowState::default();

    let report = workflow
        .startup_reconcile(
            1,
            Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
            startup_state.retry_target_mut(),
        )
        .await;

    assert!(matches!(
        &report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    {
        let events = events.lock().unwrap();
        let retirement = events
            .iter()
            .position(|event| *event == "service-core-retire")
            .unwrap();
        assert!(
            events[retirement + 1..]
                .iter()
                .all(|event| *event != "service-probe"),
            "startup must not reinterpret the post-stop projection as proof"
        );
        assert!(!events.contains(&"stop-local"));
        assert!(!events.contains(&"rebuild-local"));
    }
    let info = service.probe().await.unwrap();
    assert!(matches!(
        info.server
            .as_ref()
            .and_then(|server| server.core_infos.detail.as_ref()),
        Some(chimera_ipc::api::status::CoreStateDetail::Stopped {
            reason: Some(reason)
        }) if reason.contains("quarantined")
    ));
}

/// ST-LIFE-11; isolated lifecycle integration for deterministic Service stop failure.
/// A fresh Service Stopped probe cannot turn InvalidConfig into success;
/// startup blocks before touching the running Local owner.
#[tokio::test]
async fn deterministic_service_stop_error_with_stopped_probe_remains_blocked() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(true));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        service_running.clone(),
    );
    core.service_retire_error = Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: None,
        runtime_owned: true,
    });
    let client = spawn_startup_client(Arc::new(core), service).await;

    let completed = client.startup_reconcile().await.unwrap();

    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::Blocked,
            ..
        }
    ));
    let events = events.lock().unwrap();
    assert!(events.contains(&"service-core-retire"));
    assert!(!events.contains(&"stop-local"));
    assert!(!events.contains(&"rebuild-local"));
}

/// ST-LIFE-12; isolated lifecycle integration for Local stop error handling.
/// InvalidConfig remains a deterministic failure even when a fresh status
/// says Stopped; only NotStarted has the idempotent exception.
#[tokio::test]
async fn deterministic_local_stop_error_with_fresh_stopped_state_remains_blocked() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    core.stop_error = Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
    let client = spawn_startup_client(
        Arc::new(core),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();

    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::Blocked,
            ..
        }
    ));
    let events = events.lock().unwrap();
    assert!(events.contains(&"stop-local"));
    assert!(!events.contains(&"rebuild-local"));
}

/// ST-LIFE-13; startup recovery for a Service manager across app restarts.
/// Given a stopped Service status carries a prior error reason, the manager
/// quarantine recovery must finish before a new Local Core is applied.
#[tokio::test]
async fn startup_recovers_error_stopped_service_before_local_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Stopped(None), 0),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: Some("service manager quarantine".into()),
        runtime_owned: true,
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core,
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let mut startup_state = StartupWorkflowState::default();

    let report = workflow
        .startup_reconcile(
            1,
            Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
            startup_state.retry_target_mut(),
        )
        .await;

    assert_eq!(report.outcome, StartupOutcome::Ready);
    let events = events.lock().unwrap();
    let recover = events
        .iter()
        .position(|event| *event == "service-core-recover")
        .unwrap();
    let apply = events
        .iter()
        .position(|event| *event == "rebuild-local")
        .unwrap();
    assert!(recover < apply);
}

/// ST-LIFE-15; startup must not recover another installation's Service manager.
/// A foreign stopped status with an error reason remains unproven because
/// manager recovery may terminate a quarantined process owned by that app.
#[tokio::test]
async fn foreign_error_stopped_service_is_not_recovered_or_replaced() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Stopped(None), 0),
        service_running.clone(),
    ));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: Some("foreign service manager quarantine".into()),
        runtime_owned: false,
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core,
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let mut startup_state = StartupWorkflowState::default();

    let report = workflow
        .startup_reconcile(
            1,
            Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
            startup_state.retry_target_mut(),
        )
        .await;

    assert!(
        matches!(
            &report.outcome,
            StartupOutcome::ReadyDegraded { reason, .. }
                if reason.contains("may still hold a Core")
        ),
        "foreign Service should keep the owner unproven: {:?}",
        report.outcome
    );
    let events = events.lock().unwrap();
    assert!(!events.contains(&"service-core-recover"));
    assert!(!events.contains(&"service-core-retire"));
    assert!(!events.contains(&"stop-local"));
    assert!(!events.contains(&"rebuild-local"));
}

/// ST-LIFE-16; a Service replacement observed after recovery stays isolated.
/// The fresh probe must still identify the same Service generation and
/// this installation before startup treats the stopped reason as cleared.
#[tokio::test]
async fn service_owner_change_during_recovery_blocks_local_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(false));
    let core = Arc::new(startup_owner_core(
        events.clone(),
        (CoreState::Stopped(None), 0),
        service_running.clone(),
    ));
    let service = Arc::new(ForeignAfterRecoveryServiceProbe {
        inner: MutableStartupServiceProbe {
            events: events.clone(),
            running: service_running,
            stopped_reason: Some("service manager quarantine".into()),
            runtime_owned: true,
        },
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core,
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let mut startup_state = StartupWorkflowState::default();

    let report = workflow
        .startup_reconcile(
            1,
            Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
            startup_state.retry_target_mut(),
        )
        .await;

    assert!(matches!(
        report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "service-core-recover")
            .count(),
        1
    );
    assert!(!events.contains(&"service-core-retire"));
    assert!(!events.contains(&"stop-local"));
    assert!(!events.contains(&"rebuild-local"));
}

/// ST-LIFE-14; unresolved Service quarantine must fail closed.
/// If manager recovery cannot prove the quarantined process dead, startup
/// reports RecoveryRequired and does not apply a Local runtime.
#[tokio::test]
async fn unresolved_service_quarantine_prevents_local_apply() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service_running = Arc::new(AtomicBool::new(false));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Stopped(None), 0),
        service_running.clone(),
    );
    core.service_recover_error = Some((chimera_core_manager::CoreErrorKind::Quarantined, false));
    let service = Arc::new(MutableStartupServiceProbe {
        events: events.clone(),
        running: service_running,
        stopped_reason: Some("service manager quarantine".into()),
        runtime_owned: true,
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(core),
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        service,
    );
    let mut startup_state = StartupWorkflowState::default();

    let report = workflow
        .startup_reconcile(
            1,
            Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
            startup_state.retry_target_mut(),
        )
        .await;

    assert!(matches!(
        report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    let events = events.lock().unwrap();
    assert!(events.contains(&"service-core-recover"));
    assert!(!events.contains(&"rebuild-local"));
}

/// ST-LIFE-06; isolated lifecycle integration with a typed deterministic stop error.
/// Given the Local owner remains Running and reports InvalidConfig on stop,
/// startup blocks and never applies over the still-running owner.
#[tokio::test]
async fn deterministic_local_stop_failure_blocks_startup_retry() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    core.stop_confirms = false;
    core.stop_error = Some((chimera_core_manager::CoreErrorKind::InvalidConfig, false));
    let client = spawn_startup_client(
        Arc::new(core),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::Blocked,
            ..
        }
    ));
    assert!(!events.lock().unwrap().contains(&"rebuild-local"));
}

/// ST-LIFE-07; isolated lifecycle integration with an uncertain stop result.
/// Given the stop outcome cannot be established, startup requires explicit
/// recovery and must not issue a second runtime mutation.
#[tokio::test]
async fn uncertain_local_stop_requires_explicit_recovery() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    core.stop_confirms = false;
    core.stop_error = Some((chimera_core_manager::CoreErrorKind::StopUnconfirmed, false));
    let client = spawn_startup_client(
        Arc::new(core),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    assert!(!events.lock().unwrap().contains(&"rebuild-local"));
}

/// ST-LIFE-08; isolated lifecycle integration where stop is accepted but
/// status still says Running. Startup waits for fresh owner evidence and
/// must not apply until a later retry can prove the stop.
#[tokio::test]
async fn accepted_local_stop_waits_while_owner_is_still_running() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    core.stop_confirms = false;
    let client = spawn_startup_client(
        Arc::new(core),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
            ..
        }
    ));
    assert!(!events.lock().unwrap().contains(&"rebuild-local"));
}

/// ST-LIFE-09; isolated lifecycle integration for idempotent NotStarted.
/// Given stop returns NotStarted but a fresh status and runtime baseline
/// prove Stopped, startup accepts that evidence and applies afterwards.
#[tokio::test]
async fn not_started_stop_is_accepted_after_fresh_stopped_confirmation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut core = startup_owner_core(
        events.clone(),
        (CoreState::Running, 1),
        Arc::new(AtomicBool::new(false)),
    );
    core.stop_error = Some((chimera_core_manager::CoreErrorKind::NotStarted, false));
    let client = spawn_startup_client(
        Arc::new(core),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    let events = events.lock().unwrap();
    let stop = events
        .iter()
        .position(|event| *event == "stop-local")
        .unwrap();
    let apply = events
        .iter()
        .position(|event| *event == "rebuild-local")
        .unwrap();
    assert!(stop < apply);
}

#[tokio::test]
async fn recovery_retires_a_running_receipt_when_its_target_is_stale() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(StartupReceiptCore {
            events: events.clone(),
            status: Mutex::new((CoreState::Running, 1)),
            receipt: Mutex::new(Some(startup_test_receipt(Some("older-target".into())))),
            stop_confirms: true,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    client
        .recover_startup()
        .await
        .expect("stale recovery receipt should be retired and reapplied");

    let events = events.lock().unwrap();
    let stop = events.iter().position(|event| *event == "stop").unwrap();
    let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
    assert!(stop < rebuild);
}

#[tokio::test]
async fn startup_does_not_reconcile_when_a_local_stop_is_unconfirmed() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(StartupReceiptCore {
            events: events.clone(),
            status: Mutex::new((CoreState::Running, 1)),
            receipt: Mutex::new(None),
            stop_confirms: false,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client
        .startup_reconcile()
        .await
        .expect("unconfirmed stop should return a startup report");

    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
            ..
        }
    ));
    let events = events.lock().unwrap();
    assert!(events.contains(&"stop"));
    assert!(!events.contains(&"rebuild"));
}

#[tokio::test]
async fn startup_retries_a_retryable_core_failure_through_the_lifecycle_actor() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Notify::new());
    let service = Arc::new(RestartBudgetService {
        events: events.clone(),
        attempts: AtomicU8::new(0),
        exhausted: AtomicBool::new(false),
    });
    let client = spawn_startup_client(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered: recovered.clone(),
            outcome_uncertain: false,
            stop_gate: None,
        }),
        service,
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(
        matches!(
            initial.report.outcome,
            StartupOutcome::ReadyDegraded {
                health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
                ..
            }
        ),
        "unexpected initial startup report: {:?}",
        initial.report
    );
    assert!(initial.result.is_err());
    assert!(
        events
            .lock()
            .unwrap()
            .contains(&"rebuild-transient-failure")
    );

    tokio::time::timeout(Duration::from_secs(5), recovered.notified())
        .await
        .expect("actor should retry a transient startup failure");

    let events = events.lock().unwrap();
    assert!(events.contains(&"rebuild-recovered"));
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "rebuild-transient-failure")
            .count(),
        1
    );
    let cached = client.startup_reconcile().await.unwrap();
    assert_eq!(cached.report.operation_id, initial.report.operation_id);
    assert!(matches!(
        cached.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));
    assert!(!client.status().uncertain);
}

#[tokio::test]
async fn disconnected_service_mode_patch_refreshes_the_startup_retry_target() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Notify::new());
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let client = spawn_startup_client_with_application(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered: recovered.clone(),
            outcome_uncertain: false,
            stop_gate: None,
        }),
        Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        }),
        application,
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));

    client
        .patch_application_config(ChimeraAppConfigPatch {
            enable_service_mode: Some(true),
            ..Default::default()
        })
        .await
        .expect("disconnected Service Mode preference should be saved without runtime reconcile");

    tokio::time::timeout(Duration::from_millis(800), recovered.notified())
        .await
        .expect("the changed committed target should retry immediately");
    let events = events.lock().unwrap();
    assert!(events.contains(&"rebuild-recovered"));
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "rebuild-transient-failure")
            .count(),
        1
    );
}

#[tokio::test]
async fn startup_retry_lease_failure_is_drained_after_actor_operation_returns() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Notify::new());
    let retry_lease_started = Arc::new(Notify::new());
    let release_retry_lease = Arc::new(Notify::new());
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let service = Arc::new(StartupRetryLeaseService {
        inner: RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        },
        transition_attempts: AtomicU8::new(0),
        retry_lease_gate: Some((retry_lease_started.clone(), release_retry_lease.clone())),
    });
    let client = spawn_startup_client(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered,
            outcome_uncertain: false,
            stop_gate: Some((stop_started.clone(), release_stop.clone())),
        }),
        service.clone(),
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));
    let (identity, next_attempt) = client
        .startup_retry_schedule_for_tests()
        .await
        .unwrap()
        .expect("startup failure should schedule a retry");
    assert!(next_attempt > tokio::time::Instant::now());
    let lease_failure_observed = client
        .observe_next_startup_retry_lease_failure()
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(8), retry_lease_started.notified())
        .await
        .expect("the retry should begin acquiring its Service lease");

    let stopping_client = client.clone();
    let stop = tokio::spawn(async move { stopping_client.stop_core().await });
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("the unrelated stop should take and hold ApplicationWorkflow");

    release_retry_lease.notify_one();
    tokio::time::timeout(Duration::from_secs(1), lease_failure_observed)
        .await
        .expect("the lease failure should be handled while the workflow is busy")
        .expect("the actor should acknowledge the handled lease failure");
    assert!(!stop.is_finished());

    release_stop.notify_one();
    stop.await
        .expect("the stop request task should complete")
        .expect("the stop operation should succeed");

    let (rescheduled_identity, rescheduled_at) = client
        .startup_retry_schedule_for_tests()
        .await
        .unwrap()
        .expect("the completed actor operation should restore and reschedule the retry");
    assert_eq!(rescheduled_identity, identity);
    assert!(rescheduled_at > tokio::time::Instant::now());

    // The stopped-intent command filter prevents a late RetryStartup
    // from reapplying Core, but the earlier retry TIMER must not even
    // acquire a fresh Service lease after explicit StopCore.
    let prior_lease_attempts = service.transition_attempts.load(AtomicOrdering::Acquire);
    tokio::time::sleep_until(rescheduled_at + Duration::from_millis(120)).await;
    assert_eq!(
        service.transition_attempts.load(AtomicOrdering::Acquire),
        prior_lease_attempts,
        "late Startup retry timer must not acquire a Service lease while stopped"
    );
    client.shutdown().await.unwrap();
}

#[test]
fn startup_retry_lease_failure_is_retained_until_application_workflow_returns() {
    let now = tokio::time::Instant::now();
    let mut target = crate::client::application_workflow::startup::StartupRetryTarget::new(
        "retry-target".into(),
        false,
    );
    target.schedule_dependency_at(now - Duration::from_secs(10), "initial retry".into());
    assert!(target.begin(now));

    let mut workflow = ApplicationWorkflow::for_startup_retry_tests();
    workflow.set_startup_retry_target_for_tests(target);
    let mut pending = VecDeque::new();

    record_startup_retry_lease_failure(
        None,
        &mut pending,
        "retry-target".into(),
        "Service transition lease is unavailable".into(),
    );
    assert_eq!(pending.len(), 1);
    assert!(workflow.startup_retry_schedule().is_none());

    apply_pending_startup_retry_lease_failures(Some(&mut workflow), &mut pending);
    let (identity, next_attempt) = workflow
        .startup_retry_schedule()
        .expect("restoring the workflow must reschedule its deferred retry");
    assert_eq!(identity, "retry-target");
    assert!(next_attempt > tokio::time::Instant::now());
    assert!(pending.is_empty());
    assert!(!workflow.startup_retry_due("retry-target"));
}

#[tokio::test]
async fn startup_retry_lease_failure_waits_for_dependency_without_spending_attempts() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Notify::new());
    let service = Arc::new(StartupRetryLeaseService {
        inner: RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        },
        transition_attempts: AtomicU8::new(0),
        retry_lease_gate: None,
    });
    let client = spawn_startup_client(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered: recovered.clone(),
            outcome_uncertain: false,
            stop_gate: None,
        }),
        service.clone(),
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));
    tokio::time::timeout(Duration::from_secs(8), recovered.notified())
        .await
        .expect("startup retry should resume after the Service lease becomes available");

    assert_eq!(service.transition_attempts.load(AtomicOrdering::Acquire), 3);
    let events = events.lock().unwrap();
    assert!(events.contains(&"startup-retry-lease-failed"));
    assert!(events.contains(&"rebuild-recovered"));
}

#[tokio::test]
async fn preexisting_uncertain_runtime_blocks_startup_mutations() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let client = spawn_startup_client_with_application(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered: Arc::new(Notify::new()),
            outcome_uncertain: true,
            stop_gate: None,
        }),
        Arc::new(RestartBudgetService {
            events: events.clone(),
            attempts: AtomicU8::new(0),
            exhausted: AtomicBool::new(false),
        }),
        application.clone(),
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    assert!(client.status().uncertain);
    client
        .patch_application_config(ChimeraAppConfigPatch {
            enable_service_mode: Some(true),
            ..Default::default()
        })
        .await
        .expect("source-only Service Mode save must remain available after uncertain Core outcome");
    assert!(application.get_typed().enable_service_mode);
    assert!(client.status().uncertain);
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    assert!(
        !events
            .lock()
            .unwrap()
            .contains(&"rebuild-transient-failure")
    );
    assert!(!events.lock().unwrap().contains(&"rebuild-recovered"));
}

#[tokio::test]
async fn unresolved_live_attempt_isolated_until_explicit_reconcile_recovery() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let client = spawn_startup_client_with_application(
        Arc::new(startup_owner_core(
            events.clone(),
            (CoreState::Running, 1),
            Arc::new(AtomicBool::new(false)),
        )),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
        application.clone(),
    )
    .await;
    client
        .startup_reconcile()
        .await
        .expect("the test actor must establish the startup owner first");
    let original_operation = client
        .seed_unresolved_application_attempt_for_tests()
        .await
        .expect("the production actor should retain an unresolved attempt");

    let error = client
        .select_core(ClashCore::Mihomo)
        .await
        .expect_err("runtime mutations must be isolated while the attempt is unresolved");
    assert!(error.to_string().contains("runtime attempt is unresolved"));
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .unwrap()
            .0,
        Some((original_operation.clone(), true))
    );

    client
        .patch_application_config(ChimeraAppConfigPatch {
            enable_auto_launch: Some(true),
            ..Default::default()
        })
        .await
        .expect("source-only preferences remain writable during runtime recovery");
    assert!(application.get_typed().enable_auto_launch);
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .unwrap()
            .0,
        Some((original_operation, true)),
        "a source-only patch must preserve the unresolved attempt that owns recovery"
    );

    client
        .reconcile()
        .await
        .expect("the regular reconcile entry point should perform explicit recovery");
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .unwrap()
            .0,
        None
    );
    assert!(events.lock().unwrap().contains(&"rebuild-local"));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconcile_preserves_an_undecided_profile_source_attempt_until_commit() {
    let directory = tempfile::tempdir().expect("temporary Profiles directory");
    let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
        .expect("UTF-8 temporary Profiles path");
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .expect("Profiles state manager");
    let before = manager.snapshot_handle().load();
    let version = before.version;
    let mut candidate = before.state.clone();
    candidate.valid.push("pending-source-decision-test".into());
    let profile_snapshot = manager.snapshot_handle();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
        profile_snapshot.clone(),
    )
    .await;

    let (decision_tx, decision_rx) = oneshot::channel();
    let (local_write_started_tx, local_write_started_rx) = oneshot::channel();
    let (continue_write_tx, continue_write_rx) = oneshot::channel();
    let source_writer = tokio::spawn(async move {
        manager
            .replace_if_version_with_participant(
                version,
                candidate.clone(),
                move |decision| {
                    decision_tx
                        .send(decision.clone())
                        .expect("the test must receive the source decision");
                    Arc::new(AckOnlyProfileChange)
                },
                move || async move {
                    let _ = local_write_started_tx.send(());
                    continue_write_rx.await.map_err(|_| {
                        anyhow::anyhow!("the test must release the source persistence write")
                    })?;
                    Ok(())
                },
                || async { Ok(()) },
            )
            .await
    });
    let decision = decision_rx
        .await
        .expect("source transaction publishes its read-only decision handle");
    local_write_started_rx
        .await
        .expect("source persistence must be paused before its commit");
    assert_eq!(decision.decision(), StateDecision::Undecided);

    let unresolved_operation = client
        .seed_unresolved_source_decision_for_tests(decision.clone())
        .await
        .expect("seed the actor with the live source decision");
    let error = client
        .reconcile()
        .await
        .expect_err("reconcile cannot settle an undecided source transaction");
    assert!(error.to_string().contains("undecided"));
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .expect("read the retained source attempt")
            .0,
        Some((unresolved_operation.clone(), true)),
        "an undecided source transaction must retain its recovery owner"
    );
    assert_eq!(
        core.active_profiles
            .lock()
            .unwrap()
            .as_ref()
            .map(|profiles| profiles.valid.clone()),
        Some(before.state.valid.clone()),
        "the first reconcile only sees the currently persisted Profiles"
    );

    continue_write_tx
        .send(())
        .expect("release source persistence after checking unresolved recovery");
    assert!(matches!(
        source_writer.await.expect("source persistence task"),
        Ok(ReplaceIfVersionResult::Replaced)
    ));
    assert!(matches!(
        decision.decision(),
        StateDecision::Committed { .. }
    ));
    client
        .reconcile()
        .await
        .expect("reconcile after commit applies and settles the committed Profiles");
    assert_eq!(
        core.active_profiles
            .lock()
            .unwrap()
            .as_ref()
            .map(|profiles| profiles.valid.clone()),
        Some(profile_snapshot.load().state.valid.clone()),
        "the next reconcile applies the now-committed Profiles"
    );
    assert_eq!(
        client
            .application_recovery_state_for_tests()
            .await
            .expect("read the settled source attempt")
            .0,
        None
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn reconcile_preserves_a_profile_source_attempt_after_resource_recovery_failure() {
    let directory = tempfile::tempdir().expect("temporary Profiles directory");
    let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml"))
        .expect("UTF-8 temporary Profiles path");
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .expect("Profiles state manager");
    let before = manager.snapshot_handle().load();
    let version = before.version;
    let mut candidate = before.state.clone();
    candidate
        .valid
        .push("resource-recovery-failure-test".into());
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
        manager.snapshot_handle(),
    )
    .await;

    let (decision_tx, decision_rx) = oneshot::channel();
    let source_result = manager
        .replace_if_version_with_participant(
            version,
            candidate,
            move |decision| {
                decision_tx
                    .send(decision.clone())
                    .expect("the test must receive the source decision");
                Arc::new(AckOnlyProfileChange)
            },
            || async { Err(anyhow::anyhow!("injected Profile persistence failure")) },
            || async {
                Err(anyhow::anyhow!(
                    "injected Profile resource recovery failure"
                ))
            },
        )
        .await;
    assert!(source_result.is_err(), "the source transaction must fail");
    let decision = decision_rx
        .await
        .expect("source transaction must publish its decision handle");
    assert!(matches!(
        decision.decision(),
        StateDecision::Aborted {
            resources: chimera_core::state::AbortResourceState::NeedsRecovery(_),
        }
    ));

    let unresolved_operation = client
        .seed_unresolved_source_decision_for_tests(decision)
        .await
        .expect("seed the actor with the source recovery obligation");
    let error = client
        .reconcile()
        .await
        .expect_err("runtime reconcile cannot settle a source resource-recovery failure");
    assert!(error.to_string().contains("resource recovery"));
    let (attempt, _, _) = client
        .application_recovery_state_for_tests()
        .await
        .expect("read the retained source attempt");
    assert_eq!(
        attempt,
        Some((unresolved_operation, true)),
        "a terminal source recovery failure must retain its owner"
    );
    assert_eq!(
        core.active_profiles
            .lock()
            .unwrap()
            .as_ref()
            .map(|profiles| profiles.valid.clone()),
        Some(before.state.valid.clone()),
        "reconcile applies the currently persisted Profiles without clearing the source obligation"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn startup_recovery_preserves_the_first_startup_report() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovered = Arc::new(Notify::new());
    let service = Arc::new(RestartBudgetService {
        events: events.clone(),
        attempts: AtomicU8::new(0),
        exhausted: AtomicBool::new(false),
    });
    let client = spawn_startup_client(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered,
            outcome_uncertain: false,
            stop_gate: None,
        }),
        service,
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));

    client
        .recover_startup()
        .await
        .expect("startup recovery should reconcile the current target");
    assert!(events.lock().unwrap().contains(&"rebuild-recovered"));

    let cached = client.startup_reconcile().await.unwrap();
    assert_eq!(cached.report.operation_id, initial.report.operation_id);
    assert_eq!(cached.report.outcome, initial.report.outcome);
}

#[tokio::test]
async fn changed_core_target_invalidates_the_old_startup_retry_timer() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = Arc::new(RestartBudgetService {
        events: events.clone(),
        attempts: AtomicU8::new(0),
        exhausted: AtomicBool::new(false),
    });
    let client = spawn_startup_client(
        Arc::new(StartupTransientCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stops: AtomicU8::new(0),
            recovered: Arc::new(Notify::new()),
            outcome_uncertain: false,
            stop_gate: None,
        }),
        service,
    )
    .await;

    let initial = client.startup_reconcile().await.unwrap();
    assert!(matches!(
        initial.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::RetryScheduled,
            ..
        }
    ));
    client
        .select_core(ClashCore::ClashRs)
        .await
        .expect("selecting a new target should reconcile and commit it");
    tokio::time::sleep(Duration::from_millis(1_200)).await;

    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "rebuild-transient-failure")
            .count(),
        1
    );
    assert!(!events.lock().unwrap().contains(&"rebuild-recovered"));
}

#[tokio::test]
async fn startup_reconcile_is_idempotent_and_returns_the_first_report() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let first = client.startup_reconcile().await.unwrap();
    let second = client.startup_reconcile().await.unwrap();

    assert_eq!(first.report.operation_id, second.report.operation_id);
    assert_eq!(first.report.outcome, StartupOutcome::Ready);
    assert_eq!(second.report.outcome, StartupOutcome::Ready);
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "rebuild")
            .count(),
        1
    );
}

#[tokio::test]
async fn startup_lease_failure_caches_an_unsettled_report_and_is_idempotent() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        Arc::new(TransitionUnavailableService {
            inner: RecordingService {
                events: events.clone(),
            },
        }),
    )
    .await;

    let first = client.startup_reconcile().await.unwrap();
    let second = client.startup_reconcile().await.unwrap();

    assert_eq!(first.report.operation_id, second.report.operation_id);
    assert_eq!(
        first.report.outcome,
        StartupOutcome::Unsettled {
            reason: "could not acquire the Service transition lease: test Service transition lease is unavailable".into(),
        }
    );
    assert_eq!(second.report.outcome, first.report.outcome);
    assert!(first.result.is_err());
    assert!(client.status().uncertain);
    let events = events.lock().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "service-begin-failed")
            .count(),
        1,
        "the cached report must prevent another lease acquisition"
    );
    assert!(!events.contains(&"service-probe"));
    assert!(!events.contains(&"rebuild"));
}

#[tokio::test]
async fn startup_reconcile_refuses_local_start_when_service_owner_is_unreadable() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let application =
        ApplicationClient::static_state(chimera_config::application::ChimeraAppConfig::default());
    let service: Arc<dyn ports::ServiceLifecyclePort> = Arc::new(UnreadableService {
        events: events.clone(),
    });
    let client = spawn_startup_client_with_application(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        service,
        application.clone(),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();

    assert!(completed.result.is_err());
    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
            ..
        }
    ));
    assert!(!events.lock().unwrap().contains(&"rebuild"));
    assert!(events.lock().unwrap().contains(&"service-probe-failed"));
    assert!(client.status().uncertain);
    let mutation_error = client
        .select_core(ClashCore::Mihomo)
        .await
        .expect_err("unproven startup owner must block core mutation");
    assert!(
        mutation_error
            .to_string()
            .contains("startup has not established")
    );
    let retry_error = client
        .reconcile()
        .await
        .expect_err("normal reconcile must use a fresh startup owner check");
    assert!(retry_error.to_string().contains("may still hold a Core"));
    assert!(client.status().uncertain);
    assert!(!events.lock().unwrap().contains(&"rebuild"));

    client
        .patch_application_config(ChimeraAppConfigPatch {
            enable_service_mode: Some(true),
            ..Default::default()
        })
        .await
        .expect("source-only Service Mode save must remain available with owner evidence pending");
    assert!(application.get_typed().enable_service_mode);
    assert!(client.status().uncertain);
    assert!(!events.lock().unwrap().contains(&"rebuild"));
}

#[tokio::test]
async fn ready_service_reconcile_failure_retires_owned_core_before_local_fallback() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.enable_service_mode = true;
    let service = Arc::new(StartupFallbackServiceProbe {
        inner: StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: false,
            runtime_owned: true,
        },
    });
    let client = spawn_startup_client_with_application(
        Arc::new(StartupDegradedServiceCore {
            events: events.clone(),
            reconciles: AtomicU8::new(0),
            stopped: AtomicBool::new(false),
            selected: Mutex::new(RunType::Service),
        }),
        service,
        ApplicationClient::static_state(app),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    let events = events.lock().unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    assert_eq!(
        completed.report.observation.as_ref().unwrap().desired,
        crate::core::actor_v2::control_endpoint::ExecutionHost::Local
    );
    let failed_reconcile = events
        .iter()
        .position(|event| *event == "rebuild-service-failed")
        .unwrap();
    let retire = events
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    let fallback = events
        .iter()
        .position(|event| *event == "rebuild-after-service-release")
        .unwrap();
    let probes = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (*event == "service-probe").then_some(index))
        .collect::<Vec<_>>();
    assert!(probes.len() >= 3);
    assert!(probes[0] < retire);
    assert!(retire < probes[1]);
    assert!(probes[1] < failed_reconcile);
    assert!(failed_reconcile < probes[2]);
    assert!(probes[2] < fallback);
    assert!(!client.status().uncertain);
}

#[tokio::test]
async fn startup_adoption_falls_back_when_service_reconcile_fails() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.enable_service_mode = true;
    let service = Arc::new(StartupFallbackServiceProbe {
        inner: StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: false,
            runtime_owned: true,
        },
    });
    let core = Arc::new(StartupDegradedServiceCore {
        events: events.clone(),
        reconciles: AtomicU8::new(0),
        stopped: AtomicBool::new(true),
        selected: Mutex::new(RunType::Normal),
    });
    let client = spawn_startup_client_with_application(
        core.clone(),
        service,
        ApplicationClient::static_state(app),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    assert_eq!(
        completed.report.observation.as_ref().unwrap().desired,
        crate::core::actor_v2::control_endpoint::ExecutionHost::Local
    );
    assert_eq!(core.status().await.unwrap().run_type, RunType::Normal);

    let events = events.lock().unwrap();
    let select_service = events
        .iter()
        .position(|event| *event == "select-service")
        .unwrap();
    let failed_reconcile = events
        .iter()
        .position(|event| *event == "rebuild-service-failed")
        .unwrap();
    let fallback = events
        .iter()
        .position(|event| *event == "rebuild-after-service-release")
        .unwrap();
    assert!(select_service < failed_reconcile);
    assert!(failed_reconcile < fallback);
}

#[tokio::test]
async fn startup_retires_service_core_and_confirms_stop_before_local_reconcile() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RetiringCore {
            inner: RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            },
            selected: Mutex::new(RunType::Normal),
        }),
        Arc::new(StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: true,
            runtime_owned: true,
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    let events = events.lock().unwrap();

    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    let retire = events
        .iter()
        .position(|event| *event == "service-core-retire")
        .unwrap();
    let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
    assert!(retire < rebuild);
    // Service mutations also publish a completion-time status refresh, so
    // assert the proof/retire/confirm/rebuild order instead of a fixed
    // total probe count.
    let service_probes = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| (*event == "service-probe").then_some(index))
        .collect::<Vec<_>>();
    assert!(service_probes.iter().any(|index| *index < retire));
    assert!(
        service_probes
            .iter()
            .any(|index| *index > retire && *index < rebuild)
    );
}

#[tokio::test]
async fn startup_hands_back_foreign_service_core_without_stopping_it() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RetiringCore {
            inner: RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Service,
            },
            selected: Mutex::new(RunType::Service),
        }),
        Arc::new(StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: false,
            runtime_owned: false,
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    let events = events.lock().unwrap();

    assert_eq!(completed.report.outcome, StartupOutcome::Ready);
    let select_local = events
        .iter()
        .position(|event| *event == "select-local")
        .unwrap();
    let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
    assert!(select_local < rebuild);
    assert!(events.contains(&"rebuild"));
    assert!(!events.contains(&"rebuild-after-service-release"));
    assert!(!events.contains(&"service-core-retire"));
}

#[tokio::test]
async fn startup_does_not_mutate_or_handoff_unreadable_service_owner() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RetiringCore {
            inner: RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Service,
            },
            selected: Mutex::new(RunType::Service),
        }),
        Arc::new(UnreadableService {
            events: events.clone(),
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();
    let events = events.lock().unwrap();

    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::RecoveryRequired { .. }
    ));
    assert!(!events.contains(&"service-core-retire"));
    assert!(!events.contains(&"rebuild"));
    assert!(!events.contains(&"rebuild-after-service-release"));
}

#[tokio::test]
async fn startup_holds_service_transition_before_entering_lifecycle_actor() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = Arc::new(StartupTransitionRaceService {
        inner: StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: true,
            runtime_owned: true,
        },
        transition_lock: Arc::new(tokio::sync::Mutex::new(())),
        transition_attempts: Arc::new(AtomicUsize::new(0)),
        probe_entered: Arc::new(Notify::new()),
        release_probe: Arc::new(Notify::new()),
    });
    let client = spawn_startup_client(
        Arc::new(RetiringCore {
            inner: RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            },
            selected: Mutex::new(RunType::Normal),
        }),
        service.clone(),
    )
    .await;

    let startup_client = client.clone();
    let startup = tokio::spawn(async move { startup_client.startup_reconcile().await });
    tokio::time::timeout(Duration::from_secs(1), service.probe_entered.notified())
        .await
        .expect("startup should reach its Service probe while holding the lease");

    let service_client = client.clone();
    let service_command =
        tokio::spawn(async move { service_client.start_service(Duration::from_secs(1)).await });
    tokio::time::timeout(Duration::from_secs(1), async {
        while service.transition_attempts.load(AtomicOrdering::Acquire) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the queued Service command should contend for the transition lease");

    service.release_probe.notify_one();
    let startup = tokio::time::timeout(Duration::from_secs(2), startup)
        .await
        .expect("startup and the queued Service command must not deadlock")
        .expect("startup task should not panic")
        .expect("startup should reach a terminal report");
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    tokio::time::timeout(Duration::from_secs(2), service_command)
        .await
        .expect("queued Service command should run after startup releases the lease")
        .expect("Service task should not panic")
        .expect("Service command should succeed");

    let events = events.lock().unwrap();
    let rebuild = events.iter().position(|event| *event == "rebuild").unwrap();
    let service_start = events
        .iter()
        .position(|event| *event == "service-start")
        .unwrap();
    assert!(rebuild < service_start);
}

#[tokio::test]
async fn startup_does_not_start_local_when_service_stop_is_unconfirmed() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = spawn_startup_client(
        Arc::new(RetiringCore {
            inner: RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
            },
            selected: Mutex::new(RunType::Normal),
        }),
        Arc::new(StartupServiceProbe {
            events: events.clone(),
            probes: AtomicU8::new(0),
            confirms_stopped: false,
            runtime_owned: true,
        }),
    )
    .await;

    let completed = client.startup_reconcile().await.unwrap();

    assert!(matches!(
        completed.report.outcome,
        StartupOutcome::ReadyDegraded {
            health: crate::client::convergence::ConvergenceHealth::WaitingDependency,
            ..
        }
    ));
    assert!(client.status().uncertain);
    assert!(events.lock().unwrap().contains(&"service-core-retire"));
    assert!(!events.lock().unwrap().contains(&"rebuild"));
    assert!(
        !events
            .lock()
            .unwrap()
            .contains(&"rebuild-after-service-release")
    );
}

#[tokio::test]
async fn timed_out_startup_reconcile_has_no_terminal_completion() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let client = spawn_startup_client(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: stop_started.clone(),
            release_stop: release_stop.clone(),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        Arc::new(RecordingService {
            events: events.clone(),
        }),
    )
    .await;

    let stop_client = client.clone();
    let stop = tokio::spawn(async move { stop_client.stop_core().await });
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("stop should occupy the lifecycle actor");

    let startup = client
        .startup_reconcile_with_timeout(Some(Duration::from_millis(1)))
        .await;
    assert!(startup.is_err());
    assert!(!events.lock().unwrap().contains(&"rebuild"));

    release_stop.notify_one();
    stop.await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().unwrap().contains(&"rebuild") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("timed-out startup reconcile remains queued and later completes");

    let eventual = client
        .cached_startup_report()
        .await
        .expect("startup report query should succeed")
        .expect("queued startup should cache a terminal report");
    assert_eq!(eventual.outcome, StartupOutcome::Ready);
    let repeated = client
        .startup_reconcile()
        .await
        .expect("a repeated startup request should return the cached report");
    assert_eq!(repeated.report.operation_id, eventual.operation_id);
    assert_eq!(repeated.report.outcome, eventual.outcome);
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| **event == "rebuild")
            .count(),
        1,
        "the eventual report must remain idempotent after the original caller timed out"
    );
}

#[tokio::test]
async fn crash_recovery_signal_enters_lifecycle_mailbox() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let recovery_notify = Arc::new(Notify::new());
    let _client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: recovery_notify.clone(),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    recovery_notify.notify_one();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().unwrap().contains(&"rebuild") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("recovery signal should be admitted by the lifecycle actor");
}

#[tokio::test]
async fn timed_out_wait_does_not_cancel_admitted_operation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let stop_started = Arc::new(Notify::new());
    let release_stop = Arc::new(Notify::new());
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: stop_started.clone(),
            release_stop: release_stop.clone(),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let error = client
        .execute_with_timeout(Command::StopCore, Duration::from_millis(1))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("operation 1 timed out"));
    tokio::time::timeout(Duration::from_secs(1), stop_started.notified())
        .await
        .expect("timed-out operation should still be executing");

    assert_eq!(client.status().active, Some(1));
    let queued_client = client.clone();
    let queued = tokio::spawn(async move {
        queued_client.select_core(ClashCore::Mihomo).await.unwrap();
    });
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.status().queued.contains(&2) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second lifecycle operation should be visible in the queue");
    assert_eq!(client.status().active, Some(1));
    release_stop.notify_one();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if client.status().completed.iter().any(|entry| entry.id == 2) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("timed-out lifecycle operation should still settle");

    let status = client.status();
    let completed = status
        .completed
        .iter()
        .find(|entry| entry.id == 1)
        .expect("operation should be recorded");
    assert!(completed.error.is_none());
    assert!(status.active.is_none());
    assert!(status.queued.is_empty());
    queued.await.unwrap();
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["stop-start", "stop-end", "select"]
    );
}

#[tokio::test]
async fn committed_source_versions_share_one_mailbox_without_replaying_success() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    use crate::client::reconciler::committed::SourceDomain;
    client.source_committed(SourceDomain::Application, 4, false);
    client.source_committed(SourceDomain::Clash, 8, false);
    client.source_committed(SourceDomain::Profiles, 2, false);
    client.source_committed(SourceDomain::Clash, 7, false);

    let ClientInner::Actor { actor_ref, .. } = client.0.as_ref() else {
        panic!("this test requires the live lifecycle actor");
    };
    let seen = match actor_ref
        .call(
            |reply| Message::ReadSourceVersionsForTest { reply },
            Some(Duration::from_secs(1)),
        )
        .await
        .unwrap()
    {
        ractor::rpc::CallResult::Success(seen) => seen,
        other => panic!("failed to read source versions: {other:?}"),
    };
    assert_eq!(seen.version(SourceDomain::Application), Some(4));
    assert_eq!(seen.version(SourceDomain::Clash), Some(8));
    assert_eq!(seen.version(SourceDomain::Profiles), Some(2));
    assert!(
        events.lock().unwrap().is_empty(),
        "a commit notice is not an extra Core apply"
    );

    // An older retryable notification still owes convergence of the
    // latest target; never drop it simply because a newer version was
    // already observed. The dirty window coalesces the repeated signals.
    client.source_committed(SourceDomain::Clash, 7, true);
    client.source_committed(SourceDomain::Application, 4, true);
    client.source_committed(SourceDomain::Clash, 8, true);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events.lock().unwrap().contains(&"rebuild") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("a deferred source must request the existing runtime owner");
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(50)).await;
    assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
    assert_eq!(client.status().completed.len(), 1);
}

struct PausedProfilesCore {
    seen: Mutex<Vec<Vec<String>>>,
    first_started: Mutex<Option<oneshot::Sender<()>>>,
    release_first: Mutex<Option<oneshot::Receiver<()>>>,
}

#[async_trait]
impl CoreLifecyclePort for PausedProfilesCore {
    async fn reconcile_prepared(
        &self,
        prepared: ports::PreparedRuntime,
        _: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        let profiles = submitted_profiles(&prepared)?;
        self.seen.lock().unwrap().push(profiles.valid.clone());
        let started = self.first_started.lock().unwrap().take();
        if let Some(started) = started {
            let release = self.release_first.lock().unwrap().take().unwrap();
            let _ = started.send(());
            release
                .await
                .map_err(|_| anyhow::anyhow!("source test release dropped"))?;
        }
        Ok(())
    }

    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("a typed Profiles source must use reconcile_profiles")
    }

    async fn reconcile_profiles(
        &self,
        _: ClashConfig,
        _: ClashCore,
        _: RunType,
        profiles: Arc<chimera_config::profile::Profiles>,
        _: chimera_config::application::ChimeraAppConfig,
        _: std::collections::BTreeMap<String, String>,
        _: Option<String>,
    ) -> anyhow::Result<()> {
        self.seen.lock().unwrap().push(profiles.valid.clone());
        let started = self.first_started.lock().unwrap().take();
        if let Some(started) = started {
            let release = self.release_first.lock().unwrap().take().unwrap();
            let _ = started.send(());
            release
                .await
                .map_err(|_| anyhow::anyhow!("source test release dropped"))?;
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            run_type: RunType::Normal,
        })
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

#[tokio::test]
async fn source_advancing_during_apply_reconciles_latest_in_a_new_round() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let snapshot = manager.snapshot_handle();
    let mut first = manager.snapshot().as_ref().clone();
    first.valid.push("first-committed".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), first.clone())
        .await
        .unwrap();

    let (started, first_started) = oneshot::channel();
    let (release_first, resume) = oneshot::channel();
    let core = Arc::new(PausedProfilesCore {
        seen: Mutex::new(Vec::new()),
        first_started: Mutex::new(Some(started)),
        release_first: Mutex::new(Some(resume)),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        snapshot,
    )
    .await;

    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        1,
        true,
    );
    tokio::time::timeout(Duration::from_secs(3), first_started)
        .await
        .expect("first runtime round must reach the Core port")
        .unwrap();

    let mut latest = first.clone();
    latest.valid.push("next-committed".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(1), latest.clone())
        .await
        .unwrap();
    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        2,
        true,
    );
    release_first.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core
                .seen
                .lock()
                .unwrap()
                .last()
                .is_some_and(|valid| valid == &latest.valid)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("superseded apply must be followed by a latest-target round");
    assert_eq!(core.seen.lock().unwrap().first().unwrap(), &first.valid);
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if client.status().completed.iter().any(|round| {
                round.error.as_deref().is_some_and(|reason| {
                    reason.contains("Profiles source advanced while reconciling runtime")
                })
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the superseded round must explicitly report its version conflict");
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn a_new_committed_target_unblocks_a_previous_core_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let mut first = manager.snapshot().as_ref().clone();
    first.valid.push("initial-refused-target".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), first.clone())
        .await
        .unwrap();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(true),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core.attempts.load(AtomicOrdering::Acquire) == 1 && client.status().active.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the first target must be refused");

    let mut second = first;
    second.valid.push("changed-target-is-eligible".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(1), second.clone())
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 2, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core.attempts.load(AtomicOrdering::Acquire) >= 2
                && core
                    .active_profiles
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|profiles| profiles.valid == second.valid)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a genuinely changed target must not inherit the old refusal");
    assert_eq!(core.attempts.load(AtomicOrdering::Acquire), 2);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn refused_committed_target_waits_until_explicit_retry_without_repeating_core_apply() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let mut committed = manager.snapshot().as_ref().clone();
    committed.valid.push("rejected-runtime-target".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), committed.clone())
        .await
        .unwrap();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(true),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core.attempts.load(AtomicOrdering::Acquire) == 1 && client.status().active.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("initial committed target should be attempted and refused");
    assert!(core.active_profiles.lock().unwrap().is_none());

    // A second publication of the exact same target is not an
    // instruction to repeatedly hammer a Core that already refused it.
    let completed = client.status().completed.len();
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client
                .status()
                .completed
                .iter()
                .skip(completed)
                .any(|round| {
                    round.error.as_deref().is_some_and(|message| {
                        message.contains("committed Runtime target is not in effect")
                            && message.contains("injected current Profiles reconcile failure")
                    })
                })
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("second source notification should report a blocked attempt");
    assert_eq!(core.attempts.load(AtomicOrdering::Acquire), 1);
    client.reconcile().await.unwrap();
    assert_eq!(core.attempts.load(AtomicOrdering::Acquire), 2);
    assert_eq!(
        core.active_profiles.lock().unwrap().as_ref().unwrap().valid,
        committed.valid
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn failed_postcommit_profiles_apply_preserves_the_source_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let snapshot = manager.snapshot_handle();
    let mut committed = manager.snapshot().as_ref().clone();
    committed.valid.push("committed-before-core-failure".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), committed.clone())
        .await
        .unwrap();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(true),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        snapshot.clone(),
    )
    .await;
    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        1,
        true,
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if client.status().completed.iter().any(|round| {
                round.error.as_deref().is_some_and(|message| {
                    message.contains("injected current Profiles reconcile failure")
                })
            }) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("runtime apply failure must be observable separately from source commit");
    assert!(core.active_profiles.lock().unwrap().is_none());
    assert_eq!(
        snapshot.load().version,
        chimera_core::state::Version::new(1)
    );
    assert_eq!(
        snapshot.load().state.valid,
        committed.valid,
        "a postcommit Core refusal must not roll back the Profiles source"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_stop_intent_survives_profiles_source_and_runtime_dirty_until_start() {
    let directory = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(directory.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.stop_core().await.unwrap();
    assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 1);
    let mut candidate = manager.snapshot().as_ref().clone();
    candidate
        .valid
        .push("committed-while-explicitly-stopped".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), candidate.clone())
        .await
        .unwrap();
    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        1,
        true,
    );
    client.request_runtime_rebuild();
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    assert!(
        core.active_profiles.lock().unwrap().is_none(),
        "source notifications and dirty ticks must not undo explicit stop"
    );
    assert_eq!(core.stops.load(AtomicOrdering::SeqCst), 1);
    client.start_core().await.unwrap();
    assert_eq!(
        core.active_profiles.lock().unwrap().as_ref().unwrap().valid,
        candidate.valid
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn explicit_retry_republishes_confirmed_product_without_another_core_apply() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(PendingProductCore {
        lifecycle: Arc::new(crate::client::runtime::RuntimeLifecycle::default()),
        confirmed: Mutex::new(None),
        submissions: AtomicUsize::new(0),
        publications: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;

    let error = client
        .start_core()
        .await
        .expect_err("first Core application is accepted, but the RuntimeProduct was not published");
    assert!(error.to_string().contains("RuntimeProduct"));
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    let pending = client.reconciler_status();
    assert!(
        pending
            .maintenance
            .as_deref()
            .is_some_and(|reason| reason.contains("RuntimeProduct publication pending"))
    );
    assert_eq!(pending.degradations.len(), 1);
    assert_eq!(
        pending.degradations[0].flat().code,
        "runtime_product_publish_failed"
    );
    assert_eq!(
        pending.degradations[0].flat().phase,
        chimera_contracts::runtime::DegradationPhase::RuntimeBuild
    );
    assert!(pending.degradations[0].flat().retryable);

    client
        .reconcile()
        .await
        .expect("explicit Retry must finish the identical accepted RuntimeProduct");
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.publications.load(AtomicOrdering::Acquire), 1);
    assert!(core.lifecycle.pending_confirmed_product().is_none());
    assert!(client.reconciler_status().maintenance.is_none());
    assert!(client.reconciler_status().degradations.is_empty());
    assert!(matches!(
        client.reconciler_status().conclusion,
        Conclusion::Converged
    ));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_convergence_never_stops_a_foreign_or_unowned_service() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let ready = Arc::new(AtomicBool::new(true));
    let service = Arc::new(ReleaseOwedService {
        inner: ToggleStartupServiceProbe {
            events: Arc::new(Mutex::new(Vec::new())),
            ready: ready.clone(),
            core_running: Arc::new(AtomicBool::new(false)),
        },
        runtime_owned: false,
        attempts: attempts.clone(),
        fail_next: Arc::new(AtomicBool::new(false)),
    });
    let client = spawn_startup_client_with_profiles(
        Arc::new(ReceiptProofCore {
            active: Mutex::new(None),
            publish_wrong_target: false,
            change_instance_between_reads: false,
            status_reads: AtomicUsize::new(0),
            submissions: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            service_to_retire: None,
            retire_calls: AtomicUsize::new(0),
        }),
        service,
        manager.snapshot_handle(),
    )
    .await;
    client.start_core().await.unwrap();
    signal_service_readiness_change(&client);
    tokio::time::sleep(Duration::from_millis(90)).await;
    assert_eq!(attempts.load(AtomicOrdering::Acquire), 0);
    assert!(ready.load(AtomicOrdering::Acquire));
    assert!(client.reconciler_status().maintenance.is_none());
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn release_owed_is_independent_and_only_retry_replays_a_failed_daemon_stop() {
    use chimera_core::client::reconciler::plan::Conclusion;

    for fails_first in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
        let manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let ready = Arc::new(AtomicBool::new(true));
        let attempts = Arc::new(AtomicUsize::new(0));
        let service = Arc::new(ReleaseOwedService {
            inner: ToggleStartupServiceProbe {
                events: events.clone(),
                ready: ready.clone(),
                core_running: Arc::new(AtomicBool::new(false)),
            },
            runtime_owned: true,
            attempts: attempts.clone(),
            fail_next: Arc::new(AtomicBool::new(fails_first)),
        });
        let core = Arc::new(ReceiptProofCore {
            active: Mutex::new(None),
            publish_wrong_target: false,
            change_instance_between_reads: false,
            status_reads: AtomicUsize::new(0),
            submissions: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            service_to_retire: None,
            retire_calls: AtomicUsize::new(0),
        });
        let client =
            spawn_prepared_client_with_profiles(core, service, manager.snapshot_handle()).await;
        client.start_core().await.unwrap();

        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if attempts.load(AtomicOrdering::Acquire) == 1
                    && client.status().completed.iter().any(|item| {
                        item.error.as_deref().is_some_and(|reason| {
                            reason.contains("injected Service daemon stop failure")
                        }) == fails_first
                    })
                    && client.status().active.is_none()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the first daemon release attempt must finish");

        assert!(matches!(
            client.reconciler_status().conclusion,
            Conclusion::Converged
        ));
        if fails_first {
            assert!(ready.load(AtomicOrdering::Acquire));
            assert!(
                client
                    .reconciler_status()
                    .maintenance
                    .as_deref()
                    .is_some_and(|reason| reason.contains("injected Service daemon stop failure"))
            );
            signal_service_readiness_change(&client);
            tokio::time::sleep(Duration::from_millis(80)).await;
            assert_eq!(
                attempts.load(AtomicOrdering::Acquire),
                1,
                "normal readiness does not hammer a failed daemon stop"
            );
            client.reconcile().await.unwrap();
            tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    if attempts.load(AtomicOrdering::Acquire) == 2
                        && !ready.load(AtomicOrdering::Acquire)
                        && client.status().active.is_none()
                        && client.reconciler_status().maintenance.is_none()
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("explicit Retry must settle the retained release debt");
        } else {
            assert!(!ready.load(AtomicOrdering::Acquire));
            assert!(client.reconciler_status().maintenance.is_none());
        }
        let events = events.lock().unwrap().clone();
        assert!(events.contains(&"service-stop"));
        assert!(events.contains(&"service-confirm"));
        assert_eq!(
            attempts.load(AtomicOrdering::Acquire),
            if fails_first { 2 } else { 1 }
        );
        client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn source_committed_does_not_stop_a_core_whose_instance_changed_during_s2_sampling() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: true,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.start_core().await.unwrap();
    let mut changed = manager.snapshot().as_ref().clone();
    changed
        .valid
        .push("source-must-not-stop-changed-core".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), changed)
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 1, true);
    let settled = tokio::time::timeout(
        Duration::from_secs(3),
        client.settled(SourceDomain::Profiles, 1),
    )
    .await
    .expect("a changing Core instance must settle to a retryable ownership wait");
    assert!(matches!(settled.conclusion, Conclusion::Waiting { .. }));
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn source_committed_never_mutates_core_when_service_probe_is_unreadable() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(UnreadableService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    // Explicit Start is not an automatic SourceCommitted round. Once it
    // is running, an unreadable Service probe must block a source-driven
    // attempt to stop/restart the selected Core.
    client.start_core().await.unwrap();
    let mut changed = manager.snapshot().as_ref().clone();
    changed
        .valid
        .push("blocked-until-service-owner-proven".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), changed)
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 1, true);
    let settled = tokio::time::timeout(
        Duration::from_secs(3),
        client.settled(SourceDomain::Profiles, 1),
    )
    .await
    .expect("source round must publish an explicit ownership wait");
    assert!(matches!(settled.conclusion, Conclusion::Waiting { .. }));
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);
    client.shutdown().await.unwrap();
}

/// A concrete ref S2 owner step from SourceCommitted is continued by
/// the same serialized CoreLifecycle actor. Once its leased FollowService
/// settles, it must re-sample the latest source without a second notice.
#[tokio::test]
async fn source_owner_handoff_resumes_latest_committed_target_automatically() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let other_service_core = Arc::new(AtomicBool::new(false));
    let service_ready = Arc::new(AtomicBool::new(false));
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: Some(other_service_core.clone()),
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(ToggleStartupServiceProbe {
            events,
            ready: service_ready.clone(),
            core_running: other_service_core.clone(),
        }),
        manager.snapshot_handle(),
    )
    .await;
    let startup = client.startup_reconcile().await.unwrap();
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

    let mut latest = manager.snapshot().as_ref().clone();
    latest.valid.push("latest-after-s2-handoff".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), latest)
        .await
        .unwrap();
    service_ready.store(true, AtomicOrdering::Release);
    other_service_core.store(true, AtomicOrdering::Release);
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            let source_rounds = status
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count();
            if core.retire_calls.load(AtomicOrdering::Acquire) == 1
                && core.submissions.load(AtomicOrdering::Acquire) >= 2
                && source_rounds >= 2
                && status.active.is_none()
                && matches!(status.conclusion, Conclusion::Converged)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        let status = client.reconciler_status();
        panic!(
            "S2 handoff stalled: retire={} submits={} stops={} service_running={} active={:?} completed={:?} rounds={:?} maintenance={:?}",
            core.retire_calls.load(AtomicOrdering::Acquire),
            core.submissions.load(AtomicOrdering::Acquire),
            core.stops.load(AtomicOrdering::Acquire),
            other_service_core.load(AtomicOrdering::Acquire),
            client.status().active,
            client.status().completed.iter().map(|item| (&item.id, &item.error)).collect::<Vec<_>>(),
            status.recent.iter().map(|item| (&item.trigger, &item.conclusion)).collect::<Vec<_>>(),
            status.maintenance,
        );
    });
    assert!(!other_service_core.load(AtomicOrdering::Acquire));
    assert_eq!(core.retire_calls.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
    client.shutdown().await.unwrap();
}

/// A concrete ref S2 owner step from SourceCommitted is continued by
/// the same serialized CoreLifecycle actor. Once its leased FollowService
/// settles, it must re-sample the latest source without a second notice.
#[tokio::test]
async fn source_handoff_lease_wait_resumes_after_explicit_start_recovery() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let other_service_core = Arc::new(AtomicBool::new(false));
    let service_ready = Arc::new(AtomicBool::new(false));
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(LeaseBlockedToggleService {
            inner: ToggleStartupServiceProbe {
                events,
                ready: service_ready.clone(),
                core_running: other_service_core.clone(),
            },
            block_next: AtomicBool::new(true),
        }),
        manager.snapshot_handle(),
    )
    .await;
    let startup = client.startup_reconcile().await.unwrap();
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

    let mut latest = manager.snapshot().as_ref().clone();
    latest.valid.push("latest-after-failed-lease".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), latest)
        .await
        .unwrap();
    service_ready.store(true, AtomicOrdering::Release);
    other_service_core.store(true, AtomicOrdering::Release);
    client.source_committed(SourceDomain::Profiles, 1, true);
    // S2 is actionable but the nonblocking transition lease was busy.
    // No unproven host may stop or restart the current Core.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            if matches!(status.conclusion, Conclusion::Waiting { .. })
                && status
                    .maintenance
                    .as_deref()
                    .is_some_and(|text| text.contains("lease"))
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("an unavailable S2 lease must retain its source continuation");
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.retire_calls.load(AtomicOrdering::Acquire), 0);

    // The other Service owner stops externally. A later explicit Start
    // reestablishes a verified runtime. The pending source must then
    // schedule a new SourceCommitted round without a new source notice.
    other_service_core.store(false, AtomicOrdering::Release);
    service_ready.store(false, AtomicOrdering::Release);
    client
        .start_core()
        .await
        .expect("explicit Start can repair ownership");

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            let source_rounds = status
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count();
            if core.retire_calls.load(AtomicOrdering::Acquire) == 0
                && core.submissions.load(AtomicOrdering::Acquire) >= 2
                && source_rounds >= 2
                && status.active.is_none()
                && matches!(status.conclusion, Conclusion::Converged)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        let status = client.reconciler_status();
        panic!(
            "S2 handoff stalled: retire={} submits={} stops={} service_running={} active={:?} completed={:?} rounds={:?} maintenance={:?}",
            core.retire_calls.load(AtomicOrdering::Acquire),
            core.submissions.load(AtomicOrdering::Acquire),
            core.stops.load(AtomicOrdering::Acquire),
            other_service_core.load(AtomicOrdering::Acquire),
            client.status().active,
            client.status().completed.iter().map(|item| (&item.id, &item.error)).collect::<Vec<_>>(),
            status.recent.iter().map(|item| (&item.trigger, &item.conclusion)).collect::<Vec<_>>(),
            status.maintenance,
        );
    });
    assert!(!other_service_core.load(AtomicOrdering::Acquire));
    assert_eq!(core.retire_calls.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
    client.shutdown().await.unwrap();
}

/// The public source-commit entry must not erase its reconcile request
/// when it has already scheduled Startup owner recovery. This exercises
/// the actual client entry rather than injecting an actor message.
#[tokio::test]
async fn source_commits_during_real_startup_recovery_lease_keep_latest_intent() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(PausedStartupRecoveryLeaseService {
            inner: RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            },
            gate: gate.clone(),
        }),
        manager.snapshot_handle(),
    )
    .await;
    assert_eq!(
        client.startup_reconcile().await.unwrap().report.outcome,
        StartupOutcome::Ready
    );
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

    let ClientInner::Actor {
        startup_owner_unproven,
        startup_recovery_pending,
        ..
    } = client.0.as_ref()
    else {
        panic!("this test requires the actual actor entry");
    };
    let held_lease = gate.lock().await;
    startup_owner_unproven.store(true, Ordering::Release);
    for version in 1_u64..=2 {
        let mut next = manager.snapshot().as_ref().clone();
        next.valid
            .push(format!("latest-recovery-generation-{version}"));
        manager
            .replace_if_version(chimera_core::state::Version::new(version - 1), next)
            .await
            .unwrap();
        client.source_committed(SourceDomain::Profiles, version, true);
    }
    assert!(
        startup_recovery_pending.load(Ordering::Acquire),
        "source notification must first schedule Startup owner recovery"
    );
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);

    drop(held_lease);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            if !startup_recovery_pending.load(Ordering::Acquire)
                && core.submissions.load(AtomicOrdering::Acquire) == 2
                && core.stops.load(AtomicOrdering::Acquire) == 1
                && status.seen.version(SourceDomain::Profiles) == Some(2)
                && status
                    .recent
                    .iter()
                    .filter(|round| round.trigger == "SourceCommitted")
                    .count()
                    == 1
                && status.active.is_none()
                && matches!(status.conclusion, Conclusion::Converged)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatically recovered owner must replan latest committed source");
    client.shutdown().await.unwrap();
}

/// A delayed automatic Startup recovery may only obtain its Service
/// lease *after* the user explicitly stopped Core. The late recovery
/// must not convert Stopped intent into an implicit start.
#[tokio::test]
async fn late_startup_recovery_after_explicit_stop_cannot_restart_core() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_startup_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    assert_eq!(
        client.startup_reconcile().await.unwrap().report.outcome,
        StartupOutcome::Ready
    );
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    client.stop_core().await.unwrap();
    assert!(core.active.lock().unwrap().is_none());
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 1);

    // Equivalent to an in-flight Startup worker finishing lease
    // acquisition and enqueuing RecoverStartup after StopCore settled.
    client.recover_startup().await.unwrap();

    // The delayed startup retry timer and a Service readiness handoff
    // are also automatic commands, not explicit Start intent.
    let events = Arc::new(Mutex::new(Vec::new()));
    client
        .execute(Command::RetryStartup {
            operation_id: 1001,
            identity: "stale-after-stop".into(),
            transition: Box::new(RecordingServiceTransition {
                events: events.clone(),
            }),
        })
        .await
        .unwrap();
    client
        .execute(Command::FollowService {
            operation_id: 1002,
            transition: Box::new(RecordingServiceTransition { events }),
        })
        .await
        .unwrap();
    assert_eq!(
        core.submissions.load(AtomicOrdering::Acquire),
        1,
        "late Startup and Service commands must never reapply Core after StopCore"
    );
    assert!(core.active.lock().unwrap().is_none());
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 1);
    assert!(matches!(
        client.reconciler_status().conclusion,
        chimera_core::client::reconciler::plan::Conclusion::Inactive
    ));

    // Only an explicit StartCore can resume the stopped runtime.
    client.start_core().await.unwrap();
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
    client.shutdown().await.unwrap();
}

/// A source dirty tick can be scheduled before Startup recovery obtains
/// its owner lease. If the lease becomes pending before that tick fires,
/// the tick must not dispatch S3/S4 against an unproven transition.
#[tokio::test]
async fn queued_source_dirty_tick_waits_for_startup_owner_recovery() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    assert_eq!(
        client.startup_reconcile().await.unwrap().report.outcome,
        StartupOutcome::Ready
    );
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    let ClientInner::Actor {
        actor_ref,
        startup_recovery_pending,
        ..
    } = client.0.as_ref()
    else {
        panic!("dirty-tick gate test requires the actor");
    };

    let mut next = manager.snapshot().as_ref().clone();
    next.valid.push("dirty-tick-crosses-startup-lease".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), next)
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 1, true);
    let seen = match actor_ref
        .call(
            |reply| Message::ReadSourceVersionsForTest { reply },
            Some(Duration::from_secs(1)),
        )
        .await
        .unwrap()
    {
        ractor::rpc::CallResult::Success(seen) => seen,
        other => panic!("failed to observe queued source: {other:?}"),
    };
    assert_eq!(seen.version(SourceDomain::Profiles), Some(1));

    startup_recovery_pending.store(true, Ordering::Release);
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    assert_eq!(
        core.submissions.load(AtomicOrdering::Acquire),
        1,
        "a scheduled dirty tick cannot bypass an outstanding Startup owner lease"
    );
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);

    startup_recovery_pending.store(false, Ordering::Release);
    actor_ref
        .cast(Message::StartupRecoveryFinished { succeeded: true })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = client.reconciler_status();
            if core.submissions.load(AtomicOrdering::Acquire) == 2
                && core.stops.load(AtomicOrdering::Acquire) == 0
                && status
                    .recent
                    .iter()
                    .filter(|round| round.trigger == "SourceCommitted")
                    .count()
                    == 1
                && status.active.is_none()
                && matches!(status.conclusion, Conclusion::Converged)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending latest-source dirty must resume after verified owner recovery");
    client.shutdown().await.unwrap();
}

/// Source commits are durable even if the caller has already requested
/// a Startup owner recovery. The two notifications must coalesce behind
/// its lease, then re-read the latest Profiles generation after recovery
/// settles. An explicit Stop cancels the continuation instead.
#[tokio::test]
async fn deferred_startup_source_commits_resume_once_or_cancel_on_stop() {
    use chimera_core::client::reconciler::plan::Conclusion;

    for stop_before_recovery in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
        let mut manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .unwrap();
        let core = Arc::new(ReceiptProofCore {
            active: Mutex::new(None),
            publish_wrong_target: false,
            change_instance_between_reads: false,
            status_reads: AtomicUsize::new(0),
            submissions: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            service_to_retire: None,
            retire_calls: AtomicUsize::new(0),
        });
        let client = spawn_prepared_client_with_profiles(
            core.clone(),
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            manager.snapshot_handle(),
        )
        .await;
        let startup = client.startup_reconcile().await.unwrap();
        assert_eq!(startup.report.outcome, StartupOutcome::Ready);
        assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

        let ClientInner::Actor {
            actor_ref,
            startup_recovery_pending,
            ..
        } = client.0.as_ref()
        else {
            panic!("source recovery test requires the actor");
        };
        // Deterministically model the recovery lease in flight so
        // source messages cannot race an actual OS Service transition.
        startup_recovery_pending.store(true, Ordering::Release);
        for version in 1_u64..=2 {
            let mut latest = manager.snapshot().as_ref().clone();
            latest
                .valid
                .push(format!("committed-during-startup-{version}"));
            manager
                .replace_if_version(chimera_core::state::Version::new(version - 1), latest)
                .await
                .unwrap();
            actor_ref
                .cast(Message::SourceCommitted {
                    domain: SourceDomain::Profiles,
                    version,
                    reconcile_requested: true,
                    startup_recovery_required: true,
                })
                .unwrap();
        }
        // The actor has processed both messages but no source apply may
        // run while the caller's Startup owner recovery is unresolved.
        tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
        assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
        assert_eq!(
            client
                .reconciler_status()
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count(),
            0
        );

        // A readiness change and even a stale successful completion from
        // another generation must NOT bypass an active Startup recovery
        // lease. They can wake the source only after this gate clears.
        signal_service_readiness_change(&client);
        actor_ref
            .cast(Message::StartupRecoveryFinished { succeeded: true })
            .unwrap();
        tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
        assert_eq!(
            core.submissions.load(AtomicOrdering::Acquire),
            1,
            "readiness/stale success must not apply while Startup recovery is pending"
        );
        assert_eq!(
            client
                .reconciler_status()
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count(),
            0,
            "Startup recovery must finish before the latest source is admitted"
        );

        if stop_before_recovery {
            client.stop_core().await.unwrap();
        }
        startup_recovery_pending.store(false, Ordering::Release);
        // A failed lease or recovery completion must not turn a retained
        // source into an unauthorized Core Stop/Apply, even when the
        // shared owner flag currently appears settled.
        actor_ref
            .cast(Message::StartupRecoveryFinished { succeeded: false })
            .unwrap();
        tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
        assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
        assert_eq!(
            client
                .reconciler_status()
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count(),
            0,
            "failed recovery must retain source debt without applying"
        );
        actor_ref
            .cast(Message::StartupRecoveryFinished { succeeded: true })
            .unwrap();

        if stop_before_recovery {
            tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
            assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
            assert_eq!(
                client
                    .reconciler_status()
                    .recent
                    .iter()
                    .filter(|round| round.trigger == "SourceCommitted")
                    .count(),
                0,
                "explicit StopCore must cancel pending source continuation"
            );
            client.start_core().await.unwrap();
            tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
            assert_eq!(
                client
                    .reconciler_status()
                    .recent
                    .iter()
                    .filter(|round| round.trigger == "SourceCommitted")
                    .count(),
                0,
                "a later Start must not resurrect cancelled source debt"
            );
        } else {
            tokio::time::timeout(Duration::from_secs(4), async {
                loop {
                    let status = client.reconciler_status();
                    if core.submissions.load(AtomicOrdering::Acquire) == 2
                        && core.stops.load(AtomicOrdering::Acquire) == 0
                        && status.seen.version(SourceDomain::Profiles) == Some(2)
                        && status
                            .recent
                            .iter()
                            .filter(|round| round.trigger == "SourceCommitted")
                            .count()
                            == 1
                        && status.active.is_none()
                        && matches!(status.conclusion, Conclusion::Converged)
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("latest committed source must replan after startup recovery finishes");
            tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
            assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
            assert_eq!(
                client
                    .reconciler_status()
                    .recent
                    .iter()
                    .filter(|round| round.trigger == "SourceCommitted")
                    .count(),
                1,
                "concurrent source commits should coalesce into one latest round"
            );
        }
        client.shutdown().await.unwrap();
    }
}

/// An external Service owner may exit after the S2 transition lease was
/// refused. A *readiness update alone* must wake the retained committed
/// source without a new command, speculative Core stop or polling loop.
#[tokio::test]
async fn source_handoff_lease_wait_resumes_on_service_readiness_change() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let other_service_core = Arc::new(AtomicBool::new(false));
    let service_ready = Arc::new(AtomicBool::new(false));
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(LeaseBlockedToggleService {
            inner: ToggleStartupServiceProbe {
                events,
                ready: service_ready.clone(),
                core_running: other_service_core.clone(),
            },
            block_next: AtomicBool::new(true),
        }),
        manager.snapshot_handle(),
    )
    .await;
    let startup = client.startup_reconcile().await.unwrap();
    assert_eq!(startup.report.outcome, StartupOutcome::Ready);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

    let mut latest = manager.snapshot().as_ref().clone();
    latest
        .valid
        .push("latest-after-service-status-change".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), latest)
        .await
        .unwrap();
    service_ready.store(true, AtomicOrdering::Release);
    other_service_core.store(true, AtomicOrdering::Release);
    client.source_committed(SourceDomain::Profiles, 1, true);
    // S2 is actionable but the nonblocking transition lease was busy.
    // No unproven host may stop or restart the current Core.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            if matches!(status.conclusion, Conclusion::Waiting { .. })
                && status
                    .maintenance
                    .as_deref()
                    .is_some_and(|text| text.contains("lease"))
                && client.status().active.is_none()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("an unavailable S2 lease must retain its source continuation");
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.retire_calls.load(AtomicOrdering::Acquire), 0);

    // The other Service owner stops externally. No operation completes
    // here and no second source notification is sent; the ServiceActor's
    // readiness signal must request a fail-closed S2/S3/S4 replan.
    other_service_core.store(false, AtomicOrdering::Release);
    service_ready.store(false, AtomicOrdering::Release);
    signal_service_readiness_change(&client);

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = client.reconciler_status();
            let source_rounds = status
                .recent
                .iter()
                .filter(|round| round.trigger == "SourceCommitted")
                .count();
            if core.retire_calls.load(AtomicOrdering::Acquire) == 0
                && core.submissions.load(AtomicOrdering::Acquire) >= 2
                && source_rounds >= 2
                && status.active.is_none()
                && matches!(status.conclusion, Conclusion::Converged)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        let status = client.reconciler_status();
        panic!(
            "S2 handoff stalled: retire={} submits={} stops={} service_running={} active={:?} completed={:?} rounds={:?} maintenance={:?}",
            core.retire_calls.load(AtomicOrdering::Acquire),
            core.submissions.load(AtomicOrdering::Acquire),
            core.stops.load(AtomicOrdering::Acquire),
            other_service_core.load(AtomicOrdering::Acquire),
            client.status().active,
            client.status().completed.iter().map(|item| (&item.id, &item.error)).collect::<Vec<_>>(),
            status.recent.iter().map(|item| (&item.trigger, &item.conclusion)).collect::<Vec<_>>(),
            status.maintenance,
        );
    });
    assert!(!other_service_core.load(AtomicOrdering::Acquire));
    assert_eq!(core.retire_calls.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
    assert_eq!(
        core.stops.load(AtomicOrdering::Acquire),
        0,
        "ref S3 must not stop a verified running owner before S4 applies a new target"
    );
    assert!(client.reconciler_status().maintenance.is_none());
    // A successful latest-source round already consumed the continuation.
    // It must not schedule a redundant third SourceCommitted after 500ms.
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(70)).await;
    let source_rounds = client
        .reconciler_status()
        .recent
        .iter()
        .filter(|round| round.trigger == "SourceCommitted")
        .count();
    assert_eq!(source_rounds, 2);
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 2);
    client.shutdown().await.unwrap();
}

/// One actual SourceCommitted Actor round must run ref S2 before S3/S4.
/// Until another owned Service Core is cleared, neither Stop nor Apply
/// may be submitted to the Local Core, even if the source is newer.
#[tokio::test]
async fn source_committed_waits_for_second_service_owner_before_core_apply() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let other_service_core = Arc::new(AtomicBool::new(false));
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(ToggleStartupServiceProbe {
            events: Arc::new(Mutex::new(Vec::new())),
            ready: Arc::new(AtomicBool::new(true)),
            core_running: other_service_core.clone(),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.start_core().await.unwrap();
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);

    let mut latest = manager.snapshot().as_ref().clone();
    latest.valid.push("must-wait-for-S2-owner".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), latest)
        .await
        .unwrap();
    other_service_core.store(true, AtomicOrdering::Release);
    client.source_committed(SourceDomain::Profiles, 1, true);
    let waiting = tokio::time::timeout(
        Duration::from_secs(3),
        client.settled(SourceDomain::Profiles, 1),
    )
    .await
    .expect("S2 ownership decision must settle the source round as Waiting");
    assert!(matches!(waiting.conclusion, Conclusion::Waiting { .. }));
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);

    // A new, fresh Service probe proves the other owned Core stopped.
    // The source is still durable; a repeated notification now proceeds
    // through S3/S4 to apply to the verified Local Core without stopping it.
    other_service_core.store(false, AtomicOrdering::Release);
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core.submissions.load(AtomicOrdering::Acquire) == 2
                && core.stops.load(AtomicOrdering::Acquire) == 0
                && client.status().active.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cleared S2 ownership must allow the latest target to apply once");
    assert!(matches!(
        client.reconciler_status().conclusion,
        Conclusion::Converged
    ));
    client.shutdown().await.unwrap();
}

/// Ref S3/S4 is now called from the real committed-source actor route:
/// the same proven target is a no-op; a different target is submitted once
/// to the verified running owner without a speculative S3 Stop.
#[tokio::test]
async fn ref_plan_skips_identical_commits_and_applies_new_target_to_verified_core() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;

    client.start_core().await.unwrap();
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    let round_count = client.reconciler_status().recent.len();

    // A duplicate notification of the same durable revision should not
    // restart an already proven, unchanged runtime.
    client.source_committed(SourceDomain::Profiles, 0, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = client.reconciler_status();
            if status.recent.len() > round_count && status.active.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("duplicate source notification must finish a planned round");
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    assert_eq!(core.stops.load(AtomicOrdering::Acquire), 0);
    assert!(matches!(
        client.reconciler_status().conclusion,
        Conclusion::Converged
    ));

    let mut latest = manager.snapshot().as_ref().clone();
    latest.valid.push("next-committed-target".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), latest)
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 1, true);
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core.submissions.load(AtomicOrdering::Acquire) == 2
                && core.stops.load(AtomicOrdering::Acquire) == 0
                && client.status().active.is_none()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("changed committed target must apply once without stopping the verified Core");
    assert!(matches!(
        client.reconciler_status().conclusion,
        Conclusion::Converged
    ));
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn running_core_only_converges_when_receipt_proves_exact_committed_target() {
    use chimera_core::client::reconciler::plan::Conclusion;

    for (wrong_target, changed_instance) in [(false, false), (true, false), (false, true)] {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
        let manager = PersistentStateManagerSetup::<
            chimera_config::profile::Profiles,
            crate::core::migration::modules::profiles::ProfilesFormat,
        >::builder()
        .config_path(path)
        .assemble()
        .from_state(chimera_config::profile::Profiles::default())
        .await
        .unwrap();
        let core = Arc::new(ReceiptProofCore {
            active: Mutex::new(None),
            publish_wrong_target: wrong_target,
            change_instance_between_reads: changed_instance,
            status_reads: AtomicUsize::new(0),
            submissions: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            service_to_retire: None,
            retire_calls: AtomicUsize::new(0),
        });
        let client = spawn_startup_client_with_profiles(
            core,
            Arc::new(RecordingService {
                events: Arc::new(Mutex::new(Vec::new())),
            }),
            manager.snapshot_handle(),
        )
        .await;

        client.start_core().await.unwrap();
        let status = client.reconciler_status();
        if wrong_target || changed_instance {
            assert!(
                matches!(status.conclusion, Conclusion::Waiting { .. }),
                "an unmatched target or Core instance is not convergence: {wrong_target:?}/{changed_instance:?}"
            );
        } else {
            assert!(
                matches!(status.conclusion, Conclusion::Converged),
                "the same stable Core and exact committed target must converge"
            );
        }
        client.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn reconciler_status_waits_for_committed_version_and_keeps_stop_inactive() {
    use chimera_core::client::reconciler::plan::Conclusion;

    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_startup_client_with_profiles(
        core,
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        manager.snapshot_handle(),
    )
    .await;
    client.stop_core().await.unwrap();
    let stopped = client.reconciler_status();
    assert!(matches!(stopped.conclusion, Conclusion::Inactive));
    assert_eq!(stopped.recent.back().unwrap().trigger, "Stop");

    let mut next = manager.snapshot().as_ref().clone();
    next.valid.push("saved-after-stop".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), next)
        .await
        .unwrap();
    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        1,
        true,
    );
    let settled = tokio::time::timeout(
        Duration::from_secs(2),
        client.settled(
            crate::client::reconciler::committed::SourceDomain::Profiles,
            1,
        ),
    )
    .await
    .expect("stopped runtime must still acknowledge a durable source version");
    assert_eq!(
        settled
            .seen
            .version(crate::client::reconciler::committed::SourceDomain::Profiles),
        Some(1)
    );
    assert!(matches!(settled.conclusion, Conclusion::Inactive));
    assert!(settled.active.is_none());
    client.start_core().await.unwrap();
    let resumed = client.reconciler_status();
    assert!(
        matches!(resumed.conclusion, Conclusion::Waiting { .. }),
        "a successful apply without an observed Running state is not Converged"
    );
    assert_eq!(resumed.recent.back().unwrap().trigger, "Start");
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn profiles_source_commit_applies_latest_document_not_notified_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap();
    let mut manager = PersistentStateManagerSetup::<
        chimera_config::profile::Profiles,
        crate::core::migration::modules::profiles::ProfilesFormat,
    >::builder()
    .config_path(path)
    .assemble()
    .from_state(chimera_config::profile::Profiles::default())
    .await
    .unwrap();
    let snapshot = manager.snapshot_handle();
    let mut first = manager.snapshot().as_ref().clone();
    first.valid.push("first-committed".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(0), first.clone())
        .await
        .unwrap();
    let mut latest = first;
    latest.valid.push("latest-committed".into());
    manager
        .replace_if_version(chimera_core::state::Version::new(1), latest.clone())
        .await
        .unwrap();

    let core = Arc::new(ProfileTccCore {
        validations: AtomicUsize::new(0),
        attempts: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        fail_next_reconcile: AtomicBool::new(false),
        active_profiles: Mutex::new(None),
    });
    let client = spawn_prepared_client_with_profiles(
        core.clone(),
        Arc::new(RecordingService {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        snapshot,
    )
    .await;
    // An older SourceCommitted notification starts a round against the
    // latest source snapshot; the old candidate is never replayed.
    client.source_committed(
        crate::client::reconciler::committed::SourceDomain::Profiles,
        1,
        true,
    );
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if core
                .active_profiles
                .lock()
                .unwrap()
                .as_ref()
                .is_some_and(|current| current.valid == latest.valid)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("a source notification should apply the latest committed profiles");
    assert_eq!(core.validations.load(AtomicOrdering::SeqCst), 0);
    let settled = tokio::time::timeout(
        Duration::from_secs(2),
        client.settled(
            crate::client::reconciler::committed::SourceDomain::Profiles,
            1,
        ),
    )
    .await
    .expect("committed source round must publish its sampled version");
    assert_eq!(
        settled.recent.back().unwrap().trigger,
        "SourceCommitted",
        "source-driven work should be distinct from explicit retry"
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn burst_runtime_dirty_requests_coalesce_into_one_reconcile() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    for _ in 0..20 {
        client.request_runtime_rebuild();
    }

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().unwrap().contains(&"rebuild") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("coalesced runtime rebuild should execute");
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(50)).await;

    assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
    assert_eq!(client.status().completed.len(), 1);
}

#[tokio::test]
async fn runtime_dirty_after_window_schedules_a_later_reconcile() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    client.request_runtime_rebuild();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().unwrap().len() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("first runtime rebuild should execute");

    client.request_runtime_rebuild();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if events.lock().unwrap().len() == 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("later runtime rebuild should execute");

    assert_eq!(events.lock().unwrap().as_slice(), ["rebuild", "rebuild"]);
    assert_eq!(client.status().completed.len(), 2);
}

#[tokio::test]
async fn workflow_panic_latches_uncertain_and_blocks_follow_up_mutations() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(PanicOnStopCore {
            events: events.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let error = client.stop_core().await.unwrap_err();
    assert!(error.to_string().contains("execution state is uncertain"));
    assert!(client.status().uncertain);

    client.request_runtime_rebuild();
    tokio::time::sleep(DIRTY_WINDOW + Duration::from_millis(50)).await;

    let progress_events = Arc::new(Mutex::new(Vec::new()));
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let error = client
        .replace_core_binary(PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source: staging.path().join("source"),
            destination: staging.path().join("destination"),
            staging,
            progress: Arc::new(RecordingProgress {
                events: progress_events.clone(),
            }),
        })
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("previous core lifecycle operation has an uncertain outcome")
    );
    assert_eq!(
        progress_events.lock().unwrap().as_slice(),
        ["finished-error"]
    );

    let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("previous core lifecycle operation has an uncertain outcome")
    );
    assert_eq!(events.lock().unwrap().as_slice(), ["panic-stop"]);

    let status = client.status();
    assert!(status.uncertain);
    assert_eq!(status.completed.len(), 3);
    assert!(
        status.completed[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("execution state is uncertain"))
    );
    assert!(
        status.completed[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("previous core lifecycle operation"))
    );
    assert!(
        status.completed[2]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("previous core lifecycle operation"))
    );
}

#[tokio::test]
async fn endpoint_down_restart_budget_latches_exhausted_in_cached_status() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let service = Arc::new(RestartBudgetService {
        events: events.clone(),
        attempts: AtomicU8::new(0),
        exhausted: AtomicBool::new(false),
    });
    let client = RuntimeReconcilerClient::spawn_with_service(
        Arc::new(RecordingCore {
            events: Arc::new(Mutex::new(Vec::new())),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: Arc::new(Mutex::new(Vec::new())),
        }),
        service,
    )
    .await
    .unwrap();

    for expected_attempts in 1..=3 {
        client.request_service_endpoint_down().await;
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let status = client.service_status();
                if status.restart_attempts == expected_attempts {
                    assert_eq!(status.phase, ServicePhase::DaemonStopped);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("endpoint-down recovery should publish the restart attempt");
    }

    client.request_service_endpoint_down().await;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let status = client.service_status();
            if status.phase == ServicePhase::Exhausted {
                assert_eq!(status.restart_attempts, 3);
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("spent restart budget should latch exhausted");

    client.observe_service_status(chimera_ipc::types::StatusInfo {
        name: std::borrow::Cow::Borrowed("chimera-service"),
        version: std::borrow::Cow::Borrowed("1.0.0"),
        status: chimera_ipc::types::ServiceStatus::Stopped,
        server: None,
    });
    tokio::time::sleep(Duration::from_millis(20)).await;
    let status = client.service_status();
    assert_eq!(status.phase, ServicePhase::Exhausted);
    assert_eq!(status.restart_attempts, 3);
    assert_eq!(
        events.lock().unwrap().as_slice(),
        [
            "endpoint-down",
            "service-probe",
            "endpoint-down",
            "service-probe",
            "endpoint-down",
            "service-probe",
            "endpoint-down",
            "service-probe"
        ]
    );
}

#[tokio::test]
async fn lower_outcome_uncertain_latches_actor_and_blocks_follow_up_mutations() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(LowerUncertainCore {
            events: events.clone(),
            uncertain: AtomicBool::new(false),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let error = client.stop_core().await.unwrap_err();
    assert!(error.to_string().contains("lower mutation reply lost"));
    assert!(client.status().uncertain);

    let error = client.select_core(ClashCore::Mihomo).await.unwrap_err();
    assert!(
        error
            .to_string()
            .contains("previous core lifecycle operation has an uncertain outcome")
    );
    assert_eq!(events.lock().unwrap().as_slice(), ["lower-stop"]);
}

#[test]
fn workflow_attempt_cannot_clear_while_the_lower_core_action_is_unsettled() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let core = Arc::new(LowerUncertainCore {
        events: events.clone(),
        uncertain: AtomicBool::new(true),
    });
    let lifecycle = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let mut workflow = ApplicationWorkflow::with_lifecycle(lifecycle);
    workflow.seed_unresolved_lifecycle_attempt_for_tests();

    assert!(!workflow.conclude_attempt(None));
    assert!(
        workflow.has_unresolved_attempt(),
        "a successful outer result must not erase a pending lower action"
    );

    // The fake lower owner becomes settled. No host writes were used to
    // produce this transition: it tests only the Workflow clearance fence.
    core.uncertain.store(false, AtomicOrdering::Release);
    assert!(workflow.conclude_attempt(None));
    assert!(!workflow.has_unresolved_attempt());
}

fn checked_runtime_fixture(target: &str) -> ports::PreparedRuntime {
    use chimera_core::actor_v2::intent::RuntimeIntentBuilder;
    use chimera_core_manager::{LocalIpcPolicy, LocalIpcSettings};
    let snapshot = Arc::new(crate::client::runtime::RuntimeSnapshot::new(
        crate::client::runtime::RuntimeRevisionAllocator::new()
            .allocate()
            .unwrap(),
        ClashCore::Mihomo,
        b"mode: rule\n".to_vec(),
        serde_yaml::Mapping::new(),
    ));
    let intent = Arc::new(RuntimeIntentBuilder::build(
        (&ClashCore::Mihomo).into(),
        Arc::from(std::str::from_utf8(snapshot.product_bytes()).unwrap()),
        LocalIpcSettings {
            policy: LocalIpcPolicy::Prefer,
            keep_http_controller: true,
        },
    ));
    ports::PreparedRuntime {
        snapshot,
        intent,
        ports: crate::client::ports::CandidatePortBindings::default(),
        target: Some(target.into()),
    }
}

#[test]
fn a_new_source_notice_between_core_checks_and_completion_invalidates_old_s2_round() {
    let mut started = SourceVersions::default();
    started.observe(SourceDomain::Profiles, 4);
    started.observe(SourceDomain::Application, 2);
    let mut latest = started.clone();
    assert!(!source_committed_during_round(
        "SourceCommitted",
        &started,
        &latest
    ));
    latest.observe(SourceDomain::Application, 3);
    assert!(
        source_committed_during_round("SourceCommitted", &started, &latest),
        "cross-domain commit received during source round cannot settle its old target"
    );
    assert!(
        !source_committed_during_round("ServiceReadiness", &started, &latest),
        "Service ownership events have their own independent settlement"
    );
    assert!(
        !source_committed_during_round("Stop", &started, &latest),
        "explicit stop must settle independently of source updates"
    );
    assert!(
        !source_committed_during_round("ServiceRelease", &started, &latest),
        "Service cleanup is maintenance independent of Source versions"
    );
    latest.observe(SourceDomain::Profiles, 5);
    assert!(source_committed_during_round(
        "SourceCommitted",
        &started,
        &latest
    ));
}

#[test]
fn checked_mode_effects_and_clash_notifications_require_actor_completion_evidence() {
    assert!(checked_mode_effect_can_finish(&Ok(()), true, false));
    assert!(!checked_mode_effect_can_finish(&Ok(()), false, false));
    assert!(!checked_mode_effect_can_finish(&Ok(()), true, true));
    assert!(!checked_mode_effect_can_finish(
        &Err(anyhow::anyhow!("Core refused")),
        true,
        false
    ));

    assert_eq!(checked_clash_notice(false, &Ok(()), true), None);
    assert_eq!(checked_clash_notice(true, &Ok(()), true), Some(Ok(())));
    let pending = checked_clash_notice(true, &Ok(()), false)
        .unwrap()
        .unwrap_err();
    assert!(pending.contains("unverified") || pending.contains("not verified"));
    let error = checked_clash_notice(true, &Err(anyhow::anyhow!("Core refused")), false)
        .unwrap()
        .unwrap_err();
    assert!(error.contains("Core refused"));

    assert!(checked_mode_effect_deferred(None, 1, true, false, true));
    assert!(checked_mode_effect_deferred(Some(1), 2, true, false, true));
    assert!(!checked_mode_effect_deferred(Some(1), 1, true, false, true));
    assert!(!checked_mode_effect_deferred(Some(3), 2, true, false, true));
    assert!(!checked_mode_effect_deferred(Some(1), 2, true, true, true));
    assert!(!checked_mode_effect_deferred(
        Some(1),
        2,
        false,
        false,
        true
    ));
    assert!(!checked_mode_effect_deferred(
        Some(1),
        2,
        true,
        false,
        false
    ));
}

#[test]
fn superseded_source_does_not_publish_stale_effect_ports_or_assert_convergence() {
    let obsolete = Err(anyhow::Error::new(
        crate::client::reconciler::attempt::CommittedTargetAdvanced,
    ));
    assert!(committed_target_advanced(&obsolete));
    assert!(
        !refresh_effect_ports(true, false, false, true),
        "an advanced source must not receive its previous Core listener as a current effect"
    );
    assert!(
        refresh_effect_ports(true, true, false, true),
        "a separate Service observation must still publish its binding"
    );
    assert!(
        refresh_effect_ports(false, false, true, true),
        "Shutdown always needs to clear the previous runtime binding"
    );
    assert!(
        refresh_effect_ports(true, false, false, false),
        "a stable confirmed runtime continues publishing its ports"
    );
    let mut status = ReconcilerStatus::default();
    let mut observed = SourceVersions::default();
    observed.observe(SourceDomain::Profiles, 3);
    status.begin(1);
    status.finish(
        1,
        "SourceCommitted",
        &observed,
        RunIntent::Running,
        &obsolete,
        true,
        false,
        false,
        false,
        true,
    );
    assert!(
        matches!(
            status.conclusion,
            chimera_core::client::reconciler::plan::Conclusion::Waiting { .. }
        ),
        "an obsolete successful physical Core mutation is not the desired source convergence"
    );
}

fn checked_entry(domain: SourceDomain, target: &str) -> CheckedRuntimeCandidate {
    CheckedRuntimeCandidate {
        domain,
        request: None,
        interrupt_mode: false,
        prepared: checked_runtime_fixture(target),
        core_spec: checked_mihomo_spec(),
    }
}

#[test]
fn checked_inbox_keeps_cross_domain_candidates_and_never_selects_the_wrong_source() {
    let mut inbox = CheckedRuntimeInbox::default();
    inbox
        .accept(checked_entry(SourceDomain::Profiles, "profiles-A"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Application, "application-B"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Clash, "clash-C"))
        .unwrap();
    assert_eq!(inbox.len(), 3);
    let (selection, checked) = inbox.take_unique(SourceDomain::Application, None);
    assert_eq!(selection, CheckedSelection::Matched);
    assert_eq!(
        checked.unwrap().prepared.target.as_deref(),
        Some("application-B")
    );
    assert_eq!(inbox.len(), 2);
    assert_eq!(
        inbox.matching_count(SourceDomain::Profiles, Some("clash-C")),
        0,
        "the target and source domain must both match"
    );
    let (selection, checked) = inbox.take_unique(SourceDomain::Profiles, Some("profiles-A"));
    assert_eq!(selection, CheckedSelection::Matched);
    assert_eq!(
        checked.unwrap().prepared.target.as_deref(),
        Some("profiles-A")
    );
    let (selection, checked) = inbox.take_unique(SourceDomain::Clash, None);
    assert_eq!(selection, CheckedSelection::Matched);
    assert_eq!(checked.unwrap().prepared.target.as_deref(), Some("clash-C"));
    assert!(inbox.is_empty());
}

#[test]
fn checked_inbox_ambiguous_same_source_falls_back_without_consuming_other_domains() {
    let mut inbox = CheckedRuntimeInbox::default();
    inbox
        .accept(checked_entry(SourceDomain::Profiles, "profiles-old"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Clash, "clash-other"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Profiles, "profiles-new"))
        .unwrap();
    let (selection, checked) = inbox.take_unique(SourceDomain::Profiles, None);
    assert_eq!(selection, CheckedSelection::Ambiguous);
    assert!(
        checked.is_none(),
        "commit notification contains no candidate identity"
    );
    assert_eq!(inbox.matching_count(SourceDomain::Profiles, None), 0);
    assert_eq!(inbox.matching_count(SourceDomain::Clash, None), 1);
    let (selection, checked) = inbox.take_unique(SourceDomain::Clash, None);
    assert_eq!(selection, CheckedSelection::Matched);
    assert_eq!(
        checked.unwrap().prepared.target.as_deref(),
        Some("clash-other")
    );
}

#[test]
fn checked_mode_interruption_is_scoped_to_exact_clash_request() {
    let mut inbox = CheckedRuntimeInbox::default();
    let mut mode = checked_entry(SourceDomain::Clash, "mode-global");
    mode.request = Some("mode-request".into());
    mode.interrupt_mode = true;
    inbox.accept(mode).unwrap();
    let mut ordinary = checked_entry(SourceDomain::Clash, "other-clash");
    ordinary.request = Some("ordinary-request".into());
    inbox.accept(ordinary).unwrap();
    let mut unrelated = checked_entry(SourceDomain::Profiles, "profile-target");
    unrelated.request = Some("profile-request".into());
    inbox.accept(unrelated).unwrap();
    assert!(
        inbox
            .take_request(SourceDomain::Clash, "ordinary-request")
            .is_some_and(|checked| !checked.interrupt_mode),
        "a different same-domain Checked commit must not inherit the mode effect"
    );
    assert!(
        inbox
            .take_request(SourceDomain::Clash, "mode-request")
            .is_some_and(|checked| checked.interrupt_mode),
        "the exact mode CAS receipt must carry its one-shot effect"
    );
    assert_eq!(inbox.matching_count(SourceDomain::Profiles, None), 1);

    let mut aborted = checked_entry(SourceDomain::Clash, "cas-conflicted");
    aborted.request = Some("failed-cas".into());
    aborted.interrupt_mode = true;
    inbox.accept(aborted).unwrap();
    assert_eq!(inbox.discard_request(SourceDomain::Clash, "failed-cas"), 1);
    assert_eq!(inbox.matching_count(SourceDomain::Clash, None), 0);
    assert_eq!(inbox.matching_count(SourceDomain::Profiles, None), 1);

    let mut invalid = checked_entry(SourceDomain::Application, "invalid-effect");
    invalid.interrupt_mode = true;
    assert!(inbox.accept(invalid).is_err());
}

#[test]
fn checked_inbox_rejects_duplicate_request_identity_without_touching_other_checks() {
    let mut inbox = CheckedRuntimeInbox::default();
    let mut checked = checked_entry(SourceDomain::Clash, "clash-a");
    checked.request = Some("same-request".into());
    inbox.accept(checked).unwrap();

    let mut duplicate = checked_entry(SourceDomain::Clash, "clash-b");
    duplicate.request = Some("same-request".into());
    assert!(inbox.accept(duplicate).is_err());
    assert_eq!(inbox.matching_count(SourceDomain::Clash, None), 1);

    // The identity is scoped to its source domain; it cannot authorize
    // applying a checked product belonging to a different domain.
    let mut application = checked_entry(SourceDomain::Application, "app-a");
    application.request = Some("same-request".into());
    inbox.accept(application).unwrap();
    assert_eq!(
        inbox.discard_request(SourceDomain::Clash, "same-request"),
        1
    );
    assert_eq!(inbox.matching_count(SourceDomain::Application, None), 1);
}

#[test]
fn failed_source_cas_discards_only_its_domain_checked_proposals() {
    let mut inbox = CheckedRuntimeInbox::default();
    inbox
        .accept(checked_entry(SourceDomain::Profiles, "profile-proposal"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Clash, "uncommitted-A"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Clash, "uncommitted-B"))
        .unwrap();
    inbox
        .accept(checked_entry(SourceDomain::Application, "app-proposal"))
        .unwrap();

    assert_eq!(inbox.discard_domain(SourceDomain::Clash), 2);
    assert_eq!(inbox.discard_domain(SourceDomain::Clash), 0);
    assert_eq!(inbox.matching_count(SourceDomain::Clash, None), 0);
    assert_eq!(inbox.matching_count(SourceDomain::Profiles, None), 1);
    assert_eq!(inbox.matching_count(SourceDomain::Application, None), 1);
    assert_eq!(inbox.len(), 2);
}

#[test]
fn checked_inbox_ack_is_bounded_and_missing_target_is_not_authorized() {
    let mut inbox = CheckedRuntimeInbox::default();
    let mut without_target = checked_entry(SourceDomain::Application, "some-target");
    without_target.prepared.target = None;
    assert!(matches!(inbox.accept(without_target), Err(error)
        if matches!(error.as_ref(), chimera_contracts::runtime_error::RuntimeError::CheckUnavailable { .. })));
    assert!(inbox.is_empty());
    for index in 0..MAX_CHECKED_PENDING {
        inbox
            .accept(checked_entry(
                SourceDomain::Application,
                &format!("pending-{index}"),
            ))
            .unwrap();
    }
    let error = inbox
        .accept(checked_entry(SourceDomain::Profiles, "overflow"))
        .unwrap_err();
    assert!(matches!(
        error.as_ref(),
        chimera_contracts::runtime_error::RuntimeError::CheckUnavailable { .. }
    ));
    assert_eq!(
        inbox.len(),
        MAX_CHECKED_PENDING,
        "failed ACK must never replace or evict a previously authorized candidate"
    );
}

#[tokio::test]
async fn aborted_source_cas_removes_proposal_in_real_core_owner_mailbox() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    client
        .checked_candidate_for_domain(
            SourceDomain::Profiles,
            checked_runtime_fixture("profile-still-authorized"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    client
        .checked_candidate_for_domain(
            SourceDomain::Clash,
            checked_runtime_fixture("clash-cas-failed"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    client.abort_uncommitted_checked(SourceDomain::Clash);

    // The RPC follows the abort cast on this exact actor mailbox.
    let (target, _, _) = client
        .checked_for_tests()
        .await
        .expect("another source domain remains authorized");
    assert_eq!(target.as_deref(), Some("profile-still-authorized"));
    client.abort_uncommitted_checked(SourceDomain::Profiles);
    assert!(client.checked_for_tests().await.is_none());
    assert!(submits.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ref_checked_message_is_acknowledged_by_the_single_owner_before_source_notice() {
    use crate::client::reconciler::committed::SourceDomain;
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    let prepared = checked_runtime_fixture("precommit-source-a");
    let expected = (
        prepared.target.clone(),
        prepared.intent.digest.clone(),
        prepared.snapshot.product_bytes().to_vec(),
    );
    client
        .checked_candidate(prepared, checked_mihomo_spec())
        .await
        .unwrap();
    assert_eq!(client.checked_for_tests().await, Some(expected.clone()));
    assert!(
        events.lock().unwrap().is_empty(),
        "a checked message must not apply Core"
    );

    // The upstream ordering is Checked then SourceCommitted. The legacy
    // owner is deliberately not told to rebuild until the actual v2 apply
    // consumer is switched on, so this tests ordering without dual apply.
    client.source_committed(SourceDomain::Profiles, 7, false);
    assert_eq!(client.checked_for_tests().await, Some(expected));
    assert_eq!(
        client
            .reconciler_status()
            .seen
            .version(SourceDomain::Profiles),
        Some(7),
    );
    assert!(
        events.lock().unwrap().is_empty(),
        "notice did not start another Core executor"
    );

    client
        .checked_candidate(
            checked_runtime_fixture("precommit-source-b"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    let retained = client.checked_for_tests().await.unwrap();
    assert_eq!(
        retained.0.as_deref(),
        Some("precommit-source-a"),
        "a second precommit check must not overwrite an acknowledged earlier product"
    );
    assert_eq!(retained.2, b"mode: rule\n");
}

#[tokio::test]
async fn ref_checked_message_is_refused_after_owner_shutdown() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ConfirmedStoppedCore {
            events,
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    client
        .checked_candidate(
            checked_runtime_fixture("never-committed"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    client.shutdown().await.unwrap();
    let error = client
        .checked_candidate(
            checked_runtime_fixture("after-close"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap_err();
    assert!(matches!(
        error.as_ref(),
        chimera_contracts::runtime_error::RuntimeError::ShuttingDown
    ));
    assert!(client.checked_for_tests().await.is_none());
}

struct TestSourceAuthority {
    current: Mutex<(SourceDomain, u64, String)>,
    reads: AtomicUsize,
    selection: Option<crate::client::reconciler::committed::Selection>,
    inferred_interruption: Option<crate::client::reconciler::committed::SelectionChange>,
}

impl TestSourceAuthority {
    fn for_source(domain: SourceDomain, version: u64, target: &str) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new((domain, version, target.to_owned())),
            reads: AtomicUsize::new(0),
            selection: None,
            inferred_interruption: None,
        })
    }

    fn for_inferred_interruption(
        domain: SourceDomain,
        version: u64,
        target: &str,
        change: crate::client::reconciler::committed::SelectionChange,
    ) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new((domain, version, target.to_owned())),
            reads: AtomicUsize::new(0),
            selection: None,
            inferred_interruption: Some(change),
        })
    }

    fn for_selected_source(
        domain: SourceDomain,
        version: u64,
        target: &str,
        selection: crate::client::reconciler::committed::Selection,
    ) -> Arc<Self> {
        Arc::new(Self {
            current: Mutex::new((domain, version, target.to_owned())),
            reads: AtomicUsize::new(0),
            selection: Some(selection),
            inferred_interruption: None,
        })
    }
}

#[async_trait]
impl crate::client::reconciler::authority::RuntimeSourceAuthority for TestSourceAuthority {
    async fn verify(&self, domain: SourceDomain, version: u64, target: &str) -> anyhow::Result<()> {
        self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        let current = self.current.lock().unwrap();
        if current.0 != domain || current.1 != version || current.2 != target {
            return Err(anyhow::Error::new(
                crate::client::reconciler::attempt::CommittedTargetAdvanced,
            )
            .context("the authoritative current source does not match the checked target"));
        }
        Ok(())
    }

    async fn verify_selection(
        &self,
        domain: SourceDomain,
        version: u64,
        target: &str,
    ) -> anyhow::Result<Option<crate::client::reconciler::committed::Selection>> {
        self.verify(domain, version, target).await?;
        Ok(self.selection.clone())
    }

    async fn verify_interruption(
        &self,
        domain: SourceDomain,
        version: u64,
        target: &str,
        _previous: Option<&crate::client::reconciler::committed::Selection>,
    ) -> anyhow::Result<Option<crate::client::reconciler::committed::SelectionChange>> {
        self.verify(domain, version, target).await?;
        Ok(self.inferred_interruption)
    }

    async fn selection_for_applied_target(
        &self,
        target: &str,
    ) -> anyhow::Result<Option<crate::client::reconciler::committed::Selection>> {
        self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        Ok((self.current.lock().unwrap().2 == target)
            .then(|| self.selection.clone())
            .flatten())
    }

    async fn verify_committed_prepared_target(
        &self,
        target: &str,
        _previous: Option<&crate::client::reconciler::committed::Selection>,
    ) -> anyhow::Result<
        Option<(
            crate::client::reconciler::committed::Selection,
            Option<crate::client::reconciler::committed::SelectionChange>,
        )>,
    > {
        self.reads.fetch_add(1, AtomicOrdering::Relaxed);
        if self.current.lock().unwrap().2 != target {
            return Err(anyhow::Error::new(
                crate::client::reconciler::attempt::CommittedTargetAdvanced,
            ));
        }
        Ok(self
            .selection
            .clone()
            .map(|selection| (selection, self.inferred_interruption)))
    }
}

/// A build-only gate for the legacy stable-Local bridge. Production uses
/// RuntimeBuildService from the active Core-owned preparation session;
/// this fake records whether the workflow submitted *prepared* bytes.
struct PreparedProfilesBuild;

#[async_trait]
impl crate::client::reconciler::ports::RuntimeBuildPort for PreparedProfilesBuild {
    async fn capture_content(
        &self,
        _: &chimera_config::profile::Profiles,
    ) -> crate::client::reconciler::inputs::FrozenProfileContent {
        Default::default()
    }

    fn core_spec(
        &self,
        core: &ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, chimera_core::client::core_spec::CoreSpecError>
    {
        PreparedLocalBuild.core_spec(core)
    }

    async fn build(
        &self,
        revision: crate::client::runtime::RuntimeRevision,
        inputs: crate::client::reconciler::inputs::RuntimeInputs,
        _: chimera_config::runtime::executor::ResolvedPortBindings,
        _: bool,
    ) -> Result<
        Arc<crate::client::runtime::RuntimeSnapshot>,
        chimera_core::client::runtime_build::RuntimeBuildError,
    > {
        let bytes = serde_yaml::to_string(inputs.profiles.as_ref())
            .unwrap()
            .into_bytes();
        Ok(Arc::new(crate::client::runtime::RuntimeSnapshot::new(
            revision,
            inputs.app.core,
            bytes,
            serde_yaml::Mapping::new(),
        )))
    }

    async fn publish(
        &self,
        _: &crate::client::runtime::RuntimeSnapshot,
    ) -> Result<(), chimera_contracts::publish::PublishRuntimeError> {
        panic!("the Core owns publication after confirming a prepared apply")
    }
}

fn submitted_profiles(
    prepared: &ports::PreparedRuntime,
) -> anyhow::Result<chimera_config::profile::Profiles> {
    anyhow::ensure!(
        prepared.intent.config_text.as_bytes() == prepared.snapshot.product_bytes(),
        "submitted bytes differ from the prepared product"
    );
    Ok(serde_yaml::from_slice(prepared.snapshot.product_bytes())?)
}

struct PreparedLocalBuild;

#[async_trait]
impl crate::client::reconciler::ports::RuntimeBuildPort for PreparedLocalBuild {
    async fn capture_content(
        &self,
        _: &chimera_config::profile::Profiles,
    ) -> crate::client::reconciler::inputs::FrozenProfileContent {
        Default::default()
    }

    fn core_spec(
        &self,
        _: &ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, chimera_core::client::core_spec::CoreSpecError>
    {
        Ok(chimera_core_manager::CoreSpec {
            kind: chimera_core_manager::CoreKind::Mihomo,
            binary_path: "/tmp/prepared-local-core".into(),
            version: None,
            features: Vec::new(),
        })
    }

    async fn build(
        &self,
        revision: crate::client::runtime::RuntimeRevision,
        inputs: crate::client::reconciler::inputs::RuntimeInputs,
        _: chimera_config::runtime::executor::ResolvedPortBindings,
        _: bool,
    ) -> Result<
        Arc<crate::client::runtime::RuntimeSnapshot>,
        chimera_core::client::runtime_build::RuntimeBuildError,
    > {
        Ok(Arc::new(crate::client::runtime::RuntimeSnapshot::new(
            revision,
            inputs.app.core,
            b"mode: rule\n".to_vec(),
            serde_yaml::Mapping::new(),
        )))
    }

    async fn publish(
        &self,
        _: &crate::client::runtime::RuntimeSnapshot,
    ) -> Result<(), chimera_contracts::publish::PublishRuntimeError> {
        panic!("the Core owns publication after confirming a prepared apply")
    }
}

struct UnusedPreparedCheck;
#[async_trait]
impl crate::client::reconciler::ports::RuntimeValidatorPort for UnusedPreparedCheck {
    async fn check(
        &self,
        _: crate::client::reconciler::ports::RuntimeCheckRequest<'_>,
    ) -> crate::client::reconciler::ports::RuntimeCheckOutcome {
        panic!("source-committed preparation is not a precommit candidate check")
    }
}

struct UnusedPreparedInbox;
#[async_trait]
impl crate::client::reconciler::check::CheckedCandidatePort for UnusedPreparedInbox {
    async fn checked(
        &self,
        _: chimera_contracts::mutation::ConfigDomain,
        _: ports::PreparedRuntime,
        _: chimera_core_manager::CoreSpec,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        panic!("legacy prepared apply must not enroll a second Checked request")
    }
    fn source_committed(
        &self,
        _: chimera_contracts::mutation::ConfigDomain,
        _: chimera_core::state::Version,
    ) {
        panic!("legacy prepared apply cannot publish a source")
    }
    fn abort_uncommitted(&self, _: chimera_contracts::mutation::ConfigDomain) {}
    async fn checked_for_request(
        &self,
        _: chimera_contracts::mutation::ConfigDomain,
        _: ports::PreparedRuntime,
        _: chimera_core_manager::CoreSpec,
        _: String,
        _: bool,
    ) -> Result<(), Arc<chimera_contracts::runtime_error::RuntimeError>> {
        panic!("legacy prepared apply must not enroll a Checked receipt")
    }
    fn source_committed_for_request(
        &self,
        _: chimera_contracts::mutation::ConfigDomain,
        _: chimera_core::state::Version,
        _: String,
    ) {
        panic!("legacy prepared apply cannot publish a source")
    }
    fn abort_uncommitted_for_request(
        &self,
        _: chimera_contracts::mutation::ConfigDomain,
        _: String,
    ) {
    }
}

/// The source-released Service owner must be handed to Local before the
/// prepared runtime can be submitted. The legacy inline fallback is a
/// test failure: it would hide an extra Core mutation.
struct ServiceReleasedPreparedCore {
    inner: Arc<ReceiptProofCore>,
    session: crate::client::ports::RuntimePreparationSession,
    selected_local: AtomicBool,
    block_handoff: AtomicBool,
    handbacks: AtomicUsize,
}

#[async_trait]
impl CoreLifecyclePort for ServiceReleasedPreparedCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    fn runtime_preparation_session(
        &self,
    ) -> Option<crate::client::ports::RuntimePreparationSession> {
        Some(self.session.clone())
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("source-released Service fallback cannot use legacy inline reconcile")
    }

    async fn reconcile_profiles_after_service_release(
        &self,
        _: ClashConfig,
        _: ClashCore,
        _: Arc<chimera_config::profile::Profiles>,
        _: chimera_config::application::ChimeraAppConfig,
        _: std::collections::BTreeMap<String, String>,
        _: Option<String>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("source-released fallback must never replay legacy Core apply")
    }

    async fn reconcile_prepared_outcome(
        &self,
        prepared: ports::PreparedRuntime,
        spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<ports::CheckedApplyOutcome> {
        anyhow::ensure!(
            self.selected_local.load(AtomicOrdering::Acquire),
            "Local owner was never selected"
        );
        self.inner.reconcile_prepared_outcome(prepared, spec).await
    }

    async fn select_local_after_confirmed_service_release(&self) -> anyhow::Result<()> {
        if self.block_handoff.load(AtomicOrdering::Acquire) {
            anyhow::bail!("injected source-released handoff failure");
        }
        self.handbacks.fetch_add(1, AtomicOrdering::AcqRel);
        self.selected_local.store(true, AtomicOrdering::Release);
        Ok(())
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        if !self.selected_local.load(AtomicOrdering::Acquire) {
            return Ok(None);
        }
        self.inner.active_runtime_receipt().await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        if self.selected_local.load(AtomicOrdering::Acquire) {
            self.inner.status().await
        } else {
            Ok(CoreStatusSnapshot {
                instance_id: None,
                state: CoreState::Stopped(None),
                state_changed_at: 1,
                run_type: RunType::Service,
            })
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.inner.stop().await
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

/// A selected Service host delegates the exact prepared Core bytes to
/// the same CoreLifecycle owner. The test does not provide an inline
/// reconcile fallback: that would conceal a second Core writer.
struct SelectedServicePreparedCore {
    inner: Arc<ReceiptProofCore>,
}

#[async_trait]
impl CoreLifecyclePort for SelectedServicePreparedCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("selected Service startup must not invoke legacy inline Core reconcile")
    }

    async fn reconcile_profiles(
        &self,
        _: ClashConfig,
        _: ClashCore,
        _: RunType,
        _: Arc<chimera_config::profile::Profiles>,
        _: chimera_config::application::ChimeraAppConfig,
        _: std::collections::BTreeMap<String, String>,
        _: Option<String>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("selected Service startup must use the prepared Core owner")
    }

    async fn reconcile_prepared_outcome(
        &self,
        prepared: ports::PreparedRuntime,
        spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<ports::CheckedApplyOutcome> {
        let result = self
            .inner
            .reconcile_prepared_outcome(prepared, spec)
            .await?;
        let mut lock = self.inner.active.lock().unwrap();
        let mut receipt = lock
            .as_ref()
            .expect("the prepared Core submission produced its receipt")
            .as_ref()
            .clone();
        receipt.host = crate::core::actor_v2::control_endpoint::ExecutionHost::Service;
        *lock = Some(Arc::new(receipt));
        Ok(result)
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        self.inner.active_runtime_receipt().await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let active = self.inner.active.lock().unwrap().is_some();
        Ok(CoreStatusSnapshot {
            instance_id: active.then(|| "9af54a67-fc31-4dcb-8422-26644c6277c8".into()),
            state: if active {
                CoreState::Running
            } else {
                CoreState::Stopped(None)
            },
            state_changed_at: if active { 2 } else { 1 },
            run_type: RunType::Service,
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.inner.stop().await
    }

    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }

    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }

    async fn on_profile_change(&self, _: bool) {}
}

#[tokio::test]
async fn service_startup_submits_one_prepared_core_only_for_ready_owned_service() {
    use crate::client::{
        ports::{RuntimePreparationSession, SessionPortResolver},
        reconciler::{
            check::{Args, CandidateCheck},
            committed::Selection,
            inputs::{FrozenProfileContent, RuntimeInputs},
        },
    };

    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| Utf8PathBuf::from_path_buf(dir.path().join(name)).unwrap();
    let app = chimera_config::application::ChimeraAppConfig {
        core: ClashCore::Mihomo,
        enable_builtin_enhanced: false,
        enable_service_mode: true,
        ..Default::default()
    };
    let clash = ClashConfig::default();
    let profiles = chimera_config::profile::Profiles::default();
    let source = RuntimeInputs {
        app: app.clone(),
        clash: clash.clone(),
        profiles: Arc::new(profiles.clone()),
        content: FrozenProfileContent::default(),
    };
    let target = source.checked_target_key().unwrap();
    let selection = Selection::of(&source);
    let app_manager = chimera_state_engine::PersistentStateManagerSetup::<
        chimera_config::application::ChimeraAppConfig,
    >::builder()
    .config_path(path("application.yaml"))
    .assemble()
    .from_state(app.clone());
    let clash_manager = chimera_state_engine::PersistentStateManagerSetup::<ClashConfig>::builder()
        .config_path(path("clash.yaml"))
        .assemble()
        .from_state(clash.clone());
    let profile_manager =
        PersistentStateManagerSetup::<chimera_config::profile::Profiles>::builder()
            .config_path(path("profiles.yaml"))
            .assemble()
            .from_state(profiles)
            .await
            .unwrap();
    let profile_snapshot = profile_manager.snapshot_handle();
    let lifecycle = Arc::new(crate::client::runtime::RuntimeLifecycle::default());
    let gate = Arc::new(CandidateCheck::new(Args {
        application: app_manager.snapshot_handle(),
        clash: clash_manager.snapshot_handle(),
        profile_content: crate::state::mutation::ProfileContentReader::new(
            profile_snapshot.clone(),
            crate::state::mutation::ProfileContentLock::default(),
        ),
        builder: Arc::new(PreparedLocalBuild),
        session: RuntimePreparationSession {
            revisions: lifecycle.revision_allocator(),
            ports: Arc::new(SessionPortResolver::new(lifecycle)),
        },
        validator: Arc::new(UnusedPreparedCheck),
        workflow: Arc::new(UnusedPreparedInbox),
    }));
    let inner = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let core = Arc::new(SelectedServicePreparedCore {
        inner: inner.clone(),
    });
    let authority = TestSourceAuthority::for_selected_source(
        SourceDomain::Profiles,
        19,
        &target,
        selection.clone(),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let ready = Arc::new(AtomicBool::new(false));
    let service = Arc::new(ToggleStartupServiceProbe {
        events: events.clone(),
        ready: ready.clone(),
        core_running: Arc::new(AtomicBool::new(false)),
    });
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app.clone()),
        ClashConfigClient::legacy_with(clash).unwrap(),
        Some(profile_snapshot),
        core.clone(),
        Arc::new(RecordingInstaller { events }),
        RuntimePaths::from_config_root(dir.path().to_path_buf()),
        service,
    )
    .with_source_authority(Some(authority.clone()));
    workflow.install_checked_gate(gate).unwrap();
    assert!(workflow.startup_service_refusal_stopped().await);

    let unavailable = workflow
        .startup_reconcile_application(app.clone(), true)
        .await
        .expect_err("a disconnected Service must not submit Core bytes");
    assert!(
        unavailable
            .downcast_ref::<crate::client::reconciler::attempt::OwnershipTransitionPending>()
            .is_some()
    );
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 0);

    ready.store(true, AtomicOrdering::Release);
    workflow
        .startup_reconcile_application(app.clone(), true)
        .await
        .expect("S2-confirmed ready Service must reuse exact prepared bytes");
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 1);
    let receipt = inner.active.lock().unwrap().clone().unwrap();
    assert_eq!(
        receipt.host,
        crate::core::actor_v2::control_endpoint::ExecutionHost::Service
    );
    assert_eq!(receipt.target.as_deref(), Some(target.as_str()));
    assert_eq!(
        receipt.config_digest,
        chimera_core_manager::payload_digest(b"mode: rule\n")
    );
    assert_eq!(workflow.applied_selection_for_tests(), Some(selection));
    assert_eq!(authority.reads.load(AtomicOrdering::Relaxed), 4);
    assert!(workflow.take_checked_connection_effect().is_none());
    assert!(
        !workflow.startup_service_refusal_stopped().await,
        "an accepted and running Service Core cannot be released on a later publish error"
    );
    // The injected Service probe still describes a stopped old Core; it
    // cannot authorize a second running Service mutation.
    assert!(
        workflow
            .startup_reconcile_application(app, true)
            .await
            .is_err()
    );
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 1);
}

#[tokio::test]
async fn service_release_selects_local_before_one_prepared_core_apply() {
    use crate::client::{
        ports::{RuntimePreparationSession, SessionPortResolver},
        reconciler::{
            check::{Args, CandidateCheck},
            committed::Selection,
            inputs::{FrozenProfileContent, RuntimeInputs},
        },
    };
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| Utf8PathBuf::from_path_buf(dir.path().join(name)).unwrap();
    let app = chimera_config::application::ChimeraAppConfig {
        core: ClashCore::Mihomo,
        enable_builtin_enhanced: false,
        ..Default::default()
    };
    let clash = ClashConfig::default();
    let profiles = chimera_config::profile::Profiles::default();
    let inputs = RuntimeInputs {
        app: app.clone(),
        clash: clash.clone(),
        profiles: Arc::new(profiles.clone()),
        content: FrozenProfileContent::default(),
    };
    let target = inputs.checked_target_key().unwrap();
    let selection = Selection::of(&inputs);
    let app_manager = chimera_state_engine::PersistentStateManagerSetup::<
        chimera_config::application::ChimeraAppConfig,
    >::builder()
    .config_path(path("application.yaml"))
    .assemble()
    .from_state(app.clone());
    let clash_manager = chimera_state_engine::PersistentStateManagerSetup::<ClashConfig>::builder()
        .config_path(path("clash.yaml"))
        .assemble()
        .from_state(clash.clone());
    let profile_manager =
        PersistentStateManagerSetup::<chimera_config::profile::Profiles>::builder()
            .config_path(path("profiles.yaml"))
            .assemble()
            .from_state(profiles)
            .await
            .unwrap();
    let profile_snapshot = profile_manager.snapshot_handle();
    let lifecycle = Arc::new(crate::client::runtime::RuntimeLifecycle::default());
    let session = RuntimePreparationSession {
        revisions: lifecycle.revision_allocator(),
        ports: Arc::new(SessionPortResolver::new(lifecycle)),
    };
    let gate = Arc::new(CandidateCheck::new(Args {
        application: app_manager.snapshot_handle(),
        clash: clash_manager.snapshot_handle(),
        profile_content: crate::state::mutation::ProfileContentReader::new(
            profile_snapshot.clone(),
            crate::state::mutation::ProfileContentLock::default(),
        ),
        builder: Arc::new(PreparedLocalBuild),
        session: session.clone(),
        validator: Arc::new(UnusedPreparedCheck),
        workflow: Arc::new(UnusedPreparedInbox),
    }));
    let inner = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let core = Arc::new(ServiceReleasedPreparedCore {
        inner: inner.clone(),
        session,
        selected_local: AtomicBool::new(false),
        block_handoff: AtomicBool::new(true),
        handbacks: AtomicUsize::new(0),
    });
    let authority = TestSourceAuthority::for_selected_source(
        SourceDomain::Profiles,
        1,
        &target,
        selection.clone(),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app.clone()),
        ClashConfigClient::legacy_with(clash).unwrap(),
        Some(profile_snapshot),
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(dir.path().to_path_buf()),
        Arc::new(RecordingService { events }),
    )
    .with_source_authority(Some(authority.clone()));
    workflow.install_checked_gate(gate).unwrap();

    let failure = workflow
        .startup_reconcile_application_after_service_release(app.clone())
        .await
        .unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("injected source-released handoff failure")
    );
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.handbacks.load(AtomicOrdering::Acquire), 0);
    assert!(!core.selected_local.load(AtomicOrdering::Acquire));

    // Even with the previous Service stop already proved, a newer
    // source must abort before selecting or mutating the Local owner.
    authority.current.lock().unwrap().2 = "a-newer-committed-target".into();
    let advanced = workflow
        .startup_reconcile_application_after_service_release(app.clone())
        .await
        .unwrap_err();
    assert!(
        advanced
            .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
            .is_some()
    );
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 0);
    assert_eq!(core.handbacks.load(AtomicOrdering::Acquire), 0);

    authority.current.lock().unwrap().2 = target.clone();
    core.block_handoff.store(false, AtomicOrdering::Release);
    workflow
        .startup_reconcile_application_after_service_release(app)
        .await
        .expect("a released Service owner must hand back before submitting prepared bytes");
    assert_eq!(core.handbacks.load(AtomicOrdering::Acquire), 1);
    assert_eq!(inner.submissions.load(AtomicOrdering::Acquire), 1);
    assert!(core.selected_local.load(AtomicOrdering::Acquire));
    let receipt = inner.active.lock().unwrap().clone().unwrap();
    assert_eq!(receipt.target.as_deref(), Some(target.as_str()));
    assert_eq!(
        receipt.config_digest,
        chimera_core_manager::payload_digest(b"mode: rule\n")
    );
    assert_eq!(workflow.applied_selection_for_tests(), Some(selection));
    assert!(workflow.take_checked_connection_effect().is_none());
}

#[tokio::test]
async fn stable_local_reconcile_uses_one_prepared_core_submission_and_source_fences() {
    use crate::client::{
        ports::{RuntimePreparationSession, SessionPortResolver},
        reconciler::{
            check::{Args, CandidateCheck},
            committed::Selection,
            inputs::{FrozenProfileContent, RuntimeInputs},
        },
    };
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| Utf8PathBuf::from_path_buf(dir.path().join(name)).unwrap();
    let app = chimera_config::application::ChimeraAppConfig {
        core: ClashCore::Mihomo,
        enable_builtin_enhanced: false,
        ..Default::default()
    };
    let clash = ClashConfig::default();
    let profiles = chimera_config::profile::Profiles::default();
    let target_inputs = RuntimeInputs {
        app: app.clone(),
        clash: clash.clone(),
        profiles: Arc::new(profiles.clone()),
        content: FrozenProfileContent::default(),
    };
    let target = target_inputs.checked_target_key().unwrap();
    let selection = Selection::of(&target_inputs);
    let app_manager = chimera_state_engine::PersistentStateManagerSetup::<
        chimera_config::application::ChimeraAppConfig,
    >::builder()
    .config_path(path("application.yaml"))
    .assemble()
    .from_state(app.clone());
    let clash_manager = chimera_state_engine::PersistentStateManagerSetup::<ClashConfig>::builder()
        .config_path(path("clash.yaml"))
        .assemble()
        .from_state(clash.clone());
    let profile_manager =
        PersistentStateManagerSetup::<chimera_config::profile::Profiles>::builder()
            .config_path(path("profiles.yaml"))
            .assemble()
            .from_state(profiles)
            .await
            .unwrap();
    let profile_snapshot = profile_manager.snapshot_handle();
    let runtime_lifecycle = Arc::new(crate::client::runtime::RuntimeLifecycle::default());
    let gate = Arc::new(CandidateCheck::new(Args {
        application: app_manager.snapshot_handle(),
        clash: clash_manager.snapshot_handle(),
        profile_content: crate::state::mutation::ProfileContentReader::new(
            profile_snapshot.clone(),
            crate::state::mutation::ProfileContentLock::default(),
        ),
        builder: Arc::new(PreparedLocalBuild),
        session: RuntimePreparationSession {
            revisions: runtime_lifecycle.revision_allocator(),
            ports: Arc::new(SessionPortResolver::new(runtime_lifecycle)),
        },
        validator: Arc::new(UnusedPreparedCheck),
        workflow: Arc::new(UnusedPreparedInbox),
    }));
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(None),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let authority = TestSourceAuthority::for_selected_source(
        SourceDomain::Profiles,
        7,
        &target,
        selection.clone(),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::static_state(app.clone()),
        ClashConfigClient::legacy_with(clash.clone()).unwrap(),
        Some(profile_snapshot),
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(dir.path().to_path_buf()),
        Arc::new(RecordingService { events }),
    )
    .with_source_authority(Some(authority.clone()));
    workflow.install_checked_gate(gate).unwrap();

    assert!(
        workflow
            .try_reconcile_prepared_local(&target, &app, &clash, RunType::Service)
            .await
            .is_none(),
        "Service owner selection still belongs to S2, never the Local prepared helper"
    );
    workflow
        .startup_reconcile_application(app.clone(), false)
        .await
        .expect("S2-proven Local Startup must reuse the prepared Core path");
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
    let receipt = core.active.lock().unwrap().clone().unwrap();
    assert_eq!(receipt.target.as_deref(), Some(target.as_str()));
    assert_eq!(
        receipt.config_digest,
        chimera_core_manager::payload_digest(b"mode: rule\n")
    );
    assert_eq!(workflow.applied_selection_for_tests(), Some(selection));
    assert!(workflow.take_checked_connection_effect().is_none());
    assert_eq!(
        authority.reads.load(AtomicOrdering::Relaxed),
        4,
        "source target must be proved before build, after build, immediately before Core, and after apply"
    );

    // A later source promotion cannot replay old prepared bytes or send
    // a second Core command, even while the old accepted Core is running.
    authority.current.lock().unwrap().2 = "superseded-source".into();
    let err = workflow
        .try_reconcile_prepared_local(&target, &app, &clash, RunType::Normal)
        .await
        .expect("source rejection is a real result, not a fallback license")
        .unwrap_err();
    assert!(
        err.downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
            .is_some()
    );
    assert_eq!(core.submissions.load(AtomicOrdering::Acquire), 1);
}

#[tokio::test]
async fn legacy_running_receipt_recovers_only_matching_authoritative_selection() {
    use crate::client::reconciler::{committed::Selection, inputs::RuntimeInputs};

    let selection = Selection::of(&RuntimeInputs {
        app: chimera_config::application::ChimeraAppConfig::default(),
        clash: ClashConfig::default(),
        profiles: Arc::new(chimera_config::profile::Profiles::default()),
        content: Default::default(),
    });
    let authority = TestSourceAuthority::for_selected_source(
        SourceDomain::Profiles,
        12,
        "running-target",
        selection.clone(),
    );
    let core = Arc::new(ReceiptProofCore {
        active: Mutex::new(Some(startup_test_receipt(Some("running-target".into())))),
        publish_wrong_target: false,
        change_instance_between_reads: false,
        status_reads: AtomicUsize::new(0),
        submissions: AtomicUsize::new(0),
        stops: AtomicUsize::new(0),
        service_to_retire: None,
        retire_calls: AtomicUsize::new(0),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    )
    .with_source_authority(Some(authority.clone()));

    workflow.recover_applied_selection_from_receipt().await;
    assert_eq!(
        workflow.applied_selection_for_tests(),
        Some(selection.clone())
    );
    assert_eq!(authority.reads.load(AtomicOrdering::Relaxed), 2);
    assert_eq!(
        core.submissions.load(AtomicOrdering::Relaxed),
        0,
        "recovering selection is read-only; never apply Core a second time"
    );

    // A later committed source invalidates the old applied target even
    // while its instance-bound Core receipt is still Running.
    authority.current.lock().unwrap().2 = "newer-target".into();
    workflow.recover_applied_selection_from_receipt().await;
    assert_eq!(workflow.applied_selection_for_tests(), None);

    authority.current.lock().unwrap().2 = "running-target".into();
    workflow.recover_applied_selection_from_receipt().await;
    assert_eq!(workflow.applied_selection_for_tests(), Some(selection));

    // A stopped/retired Core has no prior applied selection, even when
    // the committed documents still match the old receipt.
    *core.active.lock().unwrap() = None;
    workflow.recover_applied_selection_from_receipt().await;
    assert_eq!(workflow.applied_selection_for_tests(), None);
    assert_eq!(core.submissions.load(AtomicOrdering::Relaxed), 0);
}

struct ExactCheckedCore {
    submits: Arc<Mutex<Vec<(String, Vec<u8>, String, String)>>>,
}

#[async_trait]
impl CoreLifecyclePort for ExactCheckedCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("legacy runtime must never rebuild a checked candidate")
    }

    async fn reconcile_prepared(
        &self,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            core_spec.kind
                == chimera_core::client::core_spec::core_kind(prepared.snapshot.target_core),
            "wrong core spec"
        );
        anyhow::ensure!(
            prepared.intent.config_text.as_bytes() == prepared.snapshot.product_bytes(),
            "prepared runtime bytes diverged"
        );
        self.submits.lock().unwrap().push((
            prepared.target.unwrap(),
            prepared.snapshot.product_bytes().to_vec(),
            prepared.intent.digest.clone(),
            core_spec.binary_path.to_string(),
        ));
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }
    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }
    async fn on_profile_change(&self, _: bool) {}
}

/// The source patch's explicit mode intent must survive an absent legacy
/// startup selection and reach the pre-apply old Core API capture.
struct ExplicitModeEffectCore {
    captures: AtomicUsize,
    applies: AtomicUsize,
}

#[async_trait]
impl CoreLifecyclePort for ExplicitModeEffectCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("checked mode must not use legacy reconcile")
    }
    async fn reconcile_prepared_outcome(
        &self,
        _prepared: ports::PreparedRuntime,
        _core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<ports::CheckedApplyOutcome> {
        self.applies.fetch_add(1, AtomicOrdering::Relaxed);
        Ok(ports::CheckedApplyOutcome {
            replaced: Some(false),
        })
    }
    async fn capture_interruption_source(
        &self,
    ) -> anyhow::Result<Option<chimera_core::actor_v2::api::ApiClient>> {
        self.captures.fetch_add(1, AtomicOrdering::Relaxed);
        anyhow::bail!("fixture: previous Core API unavailable")
    }
    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }
    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }
    async fn on_profile_change(&self, _: bool) {}
}

#[tokio::test]
async fn explicit_clash_mode_effect_prepares_old_instance_without_applied_selection() {
    let core = Arc::new(ExplicitModeEffectCore {
        captures: AtomicUsize::new(0),
        applies: AtomicUsize::new(0),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    assert!(workflow.applied_selection_for_tests().is_none());

    let authority = TestSourceAuthority::for_source(SourceDomain::Clash, 12, "mode-global");
    workflow
        .execute(
            Command::ApplyChecked {
                prepared: checked_runtime_fixture("mode-global"),
                core_spec: checked_mihomo_spec(),
                domain: SourceDomain::Clash,
                version: 12,
                authority: authority.clone(),
                interrupt_mode: true,
            },
            &mut None,
        )
        .await
        .unwrap();
    assert_eq!(core.captures.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(core.applies.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(authority.reads.load(AtomicOrdering::Relaxed), 3);
    assert!(workflow.applied_selection_for_tests().is_none());
    let effect = workflow
        .take_checked_connection_effect()
        .expect("mode interruption must remain unexecuted until Actor Completed");
    assert!(workflow.take_checked_connection_effect().is_none());
    let degraded = effect
        .finish()
        .await
        .expect("injected old API capture failure");
    assert!(degraded.message.contains("previous Core API unavailable"));
    assert!(matches!(
        degraded.reason,
        chimera_contracts::runtime::DegradationReason::ModeInterruptionFailed {
            cause: chimera_contracts::runtime::InterruptFailure::Unavailable,
        }
    ));
    assert_eq!(
        degraded.phase,
        chimera_contracts::runtime::DegradationPhase::SystemEffect
    );
    assert!(!degraded.retryable);

    // Ordinary Checked work does not inherit a prior mode request.
    let ordinary = TestSourceAuthority::for_source(SourceDomain::Clash, 13, "ordinary-update");
    workflow
        .execute(
            Command::ApplyChecked {
                prepared: checked_runtime_fixture("ordinary-update"),
                core_spec: checked_mihomo_spec(),
                domain: SourceDomain::Clash,
                version: 13,
                authority: ordinary,
                interrupt_mode: false,
            },
            &mut None,
        )
        .await
        .unwrap();
    assert_eq!(core.captures.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(core.applies.load(AtomicOrdering::Relaxed), 2);
    assert!(workflow.take_checked_connection_effect().is_none());

    // Source-version rejection must precede both API capture and Core.
    let stale = TestSourceAuthority::for_source(SourceDomain::Clash, 15, "newer-target");
    assert!(
        workflow
            .execute(
                Command::ApplyChecked {
                    prepared: checked_runtime_fixture("obsolete-mode"),
                    core_spec: checked_mihomo_spec(),
                    domain: SourceDomain::Clash,
                    version: 14,
                    authority: stale,
                    interrupt_mode: true,
                },
                &mut None,
            )
            .await
            .is_err()
    );
    assert_eq!(core.captures.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(core.applies.load(AtomicOrdering::Relaxed), 2);
    assert!(workflow.take_checked_connection_effect().is_none());
}

#[tokio::test]
async fn checked_profile_selection_interrupts_only_after_confirmed_core_apply() {
    use crate::client::reconciler::committed::SelectionChange;
    let core = Arc::new(ExplicitModeEffectCore {
        captures: AtomicUsize::new(0),
        applies: AtomicUsize::new(0),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let authority = TestSourceAuthority::for_inferred_interruption(
        SourceDomain::Profiles,
        19,
        "selected-profile",
        SelectionChange::Profile,
    );
    workflow
        .execute(
            Command::ApplyChecked {
                prepared: checked_runtime_fixture("selected-profile"),
                core_spec: checked_mihomo_spec(),
                domain: SourceDomain::Profiles,
                version: 19,
                authority: authority.clone(),
                interrupt_mode: false,
            },
            &mut None,
        )
        .await
        .unwrap();
    assert_eq!(core.captures.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(core.applies.load(AtomicOrdering::Relaxed), 1);
    assert_eq!(authority.reads.load(AtomicOrdering::Relaxed), 3);
    let effect = workflow
        .take_checked_connection_effect()
        .expect("Profile selection change must retain pre-apply Core API");
    assert!(workflow.take_checked_connection_effect().is_none());
    let degradation = effect
        .finish()
        .await
        .expect("injected old Core API failure");
    assert!(matches!(
        degradation.reason,
        chimera_contracts::runtime::DegradationReason::ProfileInterruptionFailed {
            cause: chimera_contracts::runtime::InterruptFailure::Unavailable,
        }
    ));
    assert_eq!(
        crate::client::runtime::Degradation::from_shared(degradation).code,
        "profile_interruption_failed"
    );
}

/// Model a Core apply which has already accepted the exact checked bytes
/// but has not yet answered; source publishers remain free to commit while
/// the sole lifecycle Core operation is in flight.
struct DelayedCheckedCore {
    accepted: Arc<Notify>,
    release: Arc<Notify>,
    submits: Mutex<Vec<(Vec<u8>, String)>>,
}

#[async_trait]
impl CoreLifecyclePort for DelayedCheckedCore {
    fn effective_clash_info(&self) -> chimera_core::client::clash_info::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn reconcile(&self, _: ClashConfig, _: ClashCore, _: RunType) -> anyhow::Result<()> {
        anyhow::bail!("an explicit Checked apply cannot use the old builder")
    }
    async fn reconcile_prepared(
        &self,
        prepared: ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        self.submits.lock().unwrap().push((
            prepared.snapshot.product_bytes().to_vec(),
            core_spec.binary_path.to_string(),
        ));
        self.accepted.notify_one();
        self.release.notified().await;
        Ok(())
    }
    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn change_core(&self, _: ClashCore) -> anyhow::Result<()> {
        Ok(())
    }
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(CoreStatusSnapshot {
            instance_id: None,
            state: CoreState::Stopped(None),
            state_changed_at: 0,
            run_type: RunType::Normal,
        })
    }
    fn recovery_notify(&self) -> Option<Arc<Notify>> {
        None
    }
    async fn on_profile_change(&self, _: bool) {}
}

#[tokio::test]
async fn checked_runtime_refuses_latest_source_convergence_after_core_succeeded_on_old_bytes() {
    let accepted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let core = Arc::new(DelayedCheckedCore {
        accepted: accepted.clone(),
        release: release.clone(),
        submits: Mutex::new(Vec::new()),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let authority =
        TestSourceAuthority::for_source(SourceDomain::Profiles, 7, "checked-before-core");
    let observed = authority.clone();
    let prepared = checked_runtime_fixture("checked-before-core");
    let original_bytes = prepared.snapshot.product_bytes().to_vec();
    let spec = checked_mihomo_spec();
    let binary = spec.binary_path.to_string();
    let applying = tokio::spawn(async move {
        workflow
            .execute(
                Command::ApplyChecked {
                    prepared,
                    core_spec: spec,
                    domain: SourceDomain::Profiles,
                    version: 7,
                    authority,
                    interrupt_mode: false,
                },
                &mut None,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), accepted.notified())
        .await
        .expect("the original bytes should have reached Core");
    assert_eq!(
        observed.reads.load(AtomicOrdering::Relaxed),
        2,
        "interruption-policy verification and the immediate pre-Core source fence must precede mutation"
    );
    // An independent source commit occurred while Core was applying.
    observed.current.lock().unwrap().1 = 8;
    release.notify_one();
    let error = tokio::time::timeout(Duration::from_secs(2), applying)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("authoritative current source"));
    assert!(
        error
            .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
            .is_some(),
        "the proven older Core receipt is not an applied latest target"
    );
    assert_eq!(observed.reads.load(AtomicOrdering::Relaxed), 3);
    assert_eq!(
        core.submits.lock().unwrap().as_slice(),
        &[(original_bytes, binary)],
        "the only Core owner submitted the original Checked bytes exactly once"
    );
}

#[tokio::test]
async fn checked_apply_records_only_authoritatively_confirmed_selection() {
    use crate::client::reconciler::{committed::Selection, inputs::RuntimeInputs};
    let selection = Selection::of(&RuntimeInputs {
        app: chimera_config::application::ChimeraAppConfig::default(),
        clash: ClashConfig::default(),
        profiles: Arc::new(chimera_config::profile::Profiles::default()),
        content: Default::default(),
    });
    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let selected = TestSourceAuthority::for_selected_source(
        SourceDomain::Clash,
        3,
        "checked-selected",
        selection.clone(),
    );
    workflow
        .execute(
            Command::ApplyChecked {
                prepared: checked_runtime_fixture("checked-selected"),
                core_spec: checked_mihomo_spec(),
                domain: SourceDomain::Clash,
                version: 3,
                authority: selected.clone(),
                interrupt_mode: false,
            },
            &mut None,
        )
        .await
        .unwrap();
    assert_eq!(selected.reads.load(AtomicOrdering::Relaxed), 3);
    assert_eq!(submits.lock().unwrap().len(), 1);
    assert_eq!(workflow.applied_selection_for_tests(), Some(selection));

    // The second candidate is stale before its Core submission, so the
    // previous selection cannot be used as a new interruption baseline.
    let stale = TestSourceAuthority::for_source(SourceDomain::Clash, 4, "newer-target");
    assert!(
        workflow
            .execute(
                Command::ApplyChecked {
                    prepared: checked_runtime_fixture("obsolete-target"),
                    core_spec: checked_mihomo_spec(),
                    domain: SourceDomain::Clash,
                    version: 4,
                    authority: stale,
                    interrupt_mode: false,
                },
                &mut None,
            )
            .await
            .is_err()
    );
    assert_eq!(workflow.applied_selection_for_tests(), None);
    assert_eq!(submits.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn profile_tcc_try_without_core_owned_gate_fails_before_any_core_submit() {
    use crate::client::{
        application_workflow::mutation::{CheckRecord, RuntimeCheckFailure},
        reconciler::inputs::{FrozenProfileContent, RuntimeInputs},
    };

    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let input = RuntimeInputs {
        app: chimera_config::application::ChimeraAppConfig::default(),
        clash: ClashConfig::default(),
        profiles: Arc::new(chimera_config::profile::Profiles::default()),
        content: FrozenProfileContent::default(),
    };
    let error = ProfileRuntime::prepare_profile_runtime(
        &workflow,
        input,
        &chimera_core_manager::OperationId::generate(),
    )
    .await
    .expect_err("a missing ref gate cannot trigger a second legacy YAML/Core submit");
    let failure = error.downcast_ref::<RuntimeCheckFailure>().unwrap();
    assert!(
        matches!(failure.record(), CheckRecord::Unserviceable(reason)
        if reason.contains("installed Core-owned strict candidate gate"))
    );
    assert!(
        submits.lock().unwrap().is_empty(),
        "a stopped or unconfigured Core must never receive fabricated Profile bytes"
    );
}

#[tokio::test]
async fn legacy_lifecycle_operation_invalidates_checked_selection_baseline() {
    use crate::client::reconciler::{committed::Selection, inputs::RuntimeInputs};
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        Arc::new(ExactCheckedCore {
            submits: Arc::new(Mutex::new(Vec::new())),
        }),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let selection = Selection::of(&RuntimeInputs {
        app: chimera_config::application::ChimeraAppConfig::default(),
        clash: ClashConfig::default(),
        profiles: Arc::new(chimera_config::profile::Profiles::default()),
        content: Default::default(),
    });
    workflow.set_applied_selection_for_tests(selection);
    // A legacy Retry may rebuild or replace the Core without returning
    // the ref AppliedCandidate selection. Clear the Checked baseline
    // even if the legacy retry itself cannot converge.
    let _ = workflow.execute(Command::Reconcile, &mut None).await;
    assert!(workflow.applied_selection_for_tests().is_none());
}

#[tokio::test]
async fn checked_runtime_can_converge_only_after_matching_pre_and_post_core_source_reads() {
    let accepted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let core = Arc::new(DelayedCheckedCore {
        accepted: accepted.clone(),
        release: release.clone(),
        submits: Mutex::new(Vec::new()),
    });
    let events = Arc::new(Mutex::new(Vec::new()));
    let workflow = CoreLifecycleWorkflow::new(
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        None,
        core.clone(),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingService { events }),
    );
    let authority = TestSourceAuthority::for_source(SourceDomain::Profiles, 9, "stable-target");
    let observed = authority.clone();
    let applying = tokio::spawn(async move {
        workflow
            .execute(
                Command::ApplyChecked {
                    prepared: checked_runtime_fixture("stable-target"),
                    core_spec: checked_mihomo_spec(),
                    domain: SourceDomain::Profiles,
                    version: 9,
                    authority,
                    interrupt_mode: false,
                },
                &mut None,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), accepted.notified())
        .await
        .unwrap();
    assert_eq!(observed.reads.load(AtomicOrdering::Relaxed), 2);
    release.notify_one();
    tokio::time::timeout(Duration::from_secs(2), applying)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(observed.reads.load(AtomicOrdering::Relaxed), 3);
    assert_eq!(core.submits.lock().unwrap().len(), 1);
}

/// The worker may finish both authority reads successfully while another
/// source publication was already accepted by the mailbox. Completion
/// itself must fence that notification and replan latest through the sole
/// CoreLifecycle owner. No unverified older source may settle Converged.
#[tokio::test]
async fn checked_core_completion_with_new_mailbox_source_requires_latest_reconcile() {
    let accepted = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let core = Arc::new(DelayedCheckedCore {
        accepted: accepted.clone(),
        release: release.clone(),
        submits: Mutex::new(Vec::new()),
    });
    let client = RuntimeReconcilerClient::spawn(
        core.clone(),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    let selection = crate::client::reconciler::committed::Selection::of(
        &crate::client::reconciler::inputs::RuntimeInputs {
            app: chimera_config::application::ChimeraAppConfig::default(),
            clash: ClashConfig::default(),
            profiles: Arc::new(chimera_config::profile::Profiles::default()),
            content: Default::default(),
        },
    );
    let authority = TestSourceAuthority::for_selected_source(
        SourceDomain::Profiles,
        7,
        "old-checked-target",
        selection,
    );
    let original = checked_runtime_fixture("old-checked-target");
    let bytes = original.snapshot.product_bytes().to_vec();
    client
        .checked_candidate(original, checked_mihomo_spec())
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 7, false);
    let waiter = client.clone();
    let running = tokio::spawn(async move {
        waiter
            .submit_checked_with_authority(
                SourceDomain::Profiles,
                7,
                "old-checked-target".to_owned(),
                authority.clone(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), accepted.notified())
        .await
        .expect("checked bytes were submitted to Core");
    // A different domain is committed and *observed by the same Actor*
    // while Core is in flight. The test authority still reports the old
    // checked target; the completion mailbox fence, not the worker's
    // pre/post authority callback, must detect the new source notice.
    client.source_committed(SourceDomain::Application, 1, false);
    let ClientInner::Actor { actor_ref, .. } = client.0.as_ref() else {
        panic!("a real CoreLifecycle actor is required");
    };
    let observed = match actor_ref
        .call(
            |reply| Message::ReadSourceVersionsForTest { reply },
            Some(Duration::from_secs(2)),
        )
        .await
        .unwrap()
    {
        ractor::rpc::CallResult::Success(seen) => seen,
        other => panic!("failed to observe durable Source notice: {other:?}"),
    };
    assert_eq!(
        observed.version(SourceDomain::Application),
        Some(1),
        "latest Source notice must reach the Actor before the Checked Core completes"
    );
    release.notify_one();
    let error = tokio::time::timeout(Duration::from_secs(3), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(
        error
            .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
            .is_some(),
        "mailbox completion cannot settle the older successful Core operation"
    );
    assert_eq!(core.submits.lock().unwrap().len(), 1);
    assert_eq!(core.submits.lock().unwrap()[0].0, bytes);
    assert!(
        client.applied_selection_for_tests().await.is_none(),
        "the Completed mailbox supersession fence must revoke the prior checked selection",
    );
    // Unlike a plain Retry, the invalidated command must arm one fresh
    // committed-source round in the same Actor, even without another
    // source commit or external Service event.
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let rounds = client.reconciler_status().recent;
            if rounds
                .iter()
                .filter(|r| r.trigger == "SourceCommitted")
                .count()
                >= 2
            {
                assert!(
                    rounds.iter().any(|r| r.conclusion == "Waiting"),
                    "obsolete physical Core success cannot conclude convergence"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("latest source should be replanned by the sole owner");
    assert_eq!(core.submits.lock().unwrap().len(), 1);
    client.shutdown().await.unwrap();
}

fn checked_mihomo_spec() -> chimera_core_manager::CoreSpec {
    chimera_core::client::core_spec::resolve_with(&ClashCore::Mihomo, |_, _| {
        Ok(std::path::PathBuf::from("/tmp/chimera-checked-mihomo"))
    })
    .unwrap()
}

#[tokio::test]
async fn deferred_checked_mode_effect_remains_visible_when_exact_owner_is_unavailable() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    client
        .checked_candidate_for_domain_with_request_effect(
            SourceDomain::Clash,
            checked_runtime_fixture("committed-mode-target"),
            checked_mihomo_spec(),
            "mode-cas-receipt".to_owned(),
            true,
        )
        .await
        .unwrap();
    // This test-only Core owner has no LiveSourceAuthority. The source
    // is durable but the checked product cannot be safely submitted.
    // Its one-shot Mode effect must not be lost with the fallback.
    client.checked_source_committed_for_request(
        SourceDomain::Clash,
        17,
        "mode-cas-receipt".to_owned(),
    );
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let status = client.reconciler_status();
            if let Some(reason) = status.maintenance.as_deref()
                && reason.contains("Mode interruption is not confirmed")
            {
                assert!(reason.contains("17"));
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Mode effect debt must be visible after the source-only fallback");
    assert!(submits.lock().unwrap().is_empty());
    let reported = client.reconciler_status().degradations;
    assert_eq!(reported.len(), 1);
    assert_eq!(reported[0].flat().code, "mode_interruption_failed");
    assert_eq!(
        reported[0].flat().phase,
        chimera_contracts::runtime::DegradationPhase::SystemEffect
    );
    assert!(!reported[0].flat().retryable);
    // Duplicate delivery cannot create another effect or clear the debt.
    client.checked_source_committed_for_request(
        SourceDomain::Clash,
        17,
        "mode-cas-receipt".to_owned(),
    );
    assert!(
        client
            .reconciler_status()
            .maintenance
            .as_deref()
            .is_some_and(|reason| reason.contains("Mode interruption is not confirmed"))
    );
    client.shutdown().await.unwrap();
}

#[tokio::test]
async fn checked_source_dispatch_uses_single_lifecycle_core_queue_and_exact_bytes() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    let authority = TestSourceAuthority::for_source(SourceDomain::Profiles, 8, "checked-source-a");
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    let prepared = checked_runtime_fixture("checked-source-a");
    let digest = prepared.intent.digest.clone();
    let bytes = prepared.snapshot.product_bytes().to_vec();
    let mut validated_spec = checked_mihomo_spec();
    validated_spec.binary_path = camino::Utf8PathBuf::from("/tmp/only-this-binary-was-checked");
    client
        .checked_candidate(prepared, validated_spec.clone())
        .await
        .unwrap();
    assert!(
        client
            .submit_checked_with_authority(
                SourceDomain::Profiles,
                8,
                "checked-source-a".into(),
                authority.clone(),
            )
            .await
            .is_err(),
        "a candidate cannot apply before its durable source notice"
    );
    assert!(submits.lock().unwrap().is_empty());
    client.source_committed(SourceDomain::Profiles, 8, false);
    assert!(
        client
            .submit_checked_with_authority(
                SourceDomain::Profiles,
                8,
                "another-target".into(),
                authority.clone(),
            )
            .await
            .is_err(),
        "target identity must match the checked product"
    );
    assert!(submits.lock().unwrap().is_empty());
    client
        .submit_checked_with_authority(
            SourceDomain::Profiles,
            8,
            "checked-source-a".into(),
            authority.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        submits.lock().unwrap().as_slice(),
        &[(
            "checked-source-a".to_owned(),
            bytes,
            digest,
            validated_spec.binary_path.to_string()
        )]
    );
    assert!(
        client.checked_for_tests().await.is_none(),
        "accepted candidate is consumed exactly once"
    );
    assert!(
        client
            .submit_checked_with_authority(
                SourceDomain::Profiles,
                8,
                "checked-source-a".into(),
                authority.clone(),
            )
            .await
            .is_err(),
        "an already used checked value cannot be replayed"
    );
    assert_eq!(submits.lock().unwrap().len(), 1);
    assert_eq!(
        authority.reads.load(AtomicOrdering::Relaxed),
        3,
        "the executed candidate requires interruption-policy, immediate pre-Core, and post-Core source reads"
    );
}

#[tokio::test]
async fn newer_source_notification_refuses_stale_checked_execution() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    let authority = TestSourceAuthority::for_source(SourceDomain::Profiles, 4, "checked-v4");
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    client
        .checked_candidate(checked_runtime_fixture("checked-v4"), checked_mihomo_spec())
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 4, false);
    client.source_committed(SourceDomain::Profiles, 5, false);
    let result = client
        .submit_checked_with_authority(
            SourceDomain::Profiles,
            4,
            "checked-v4".into(),
            authority.clone(),
        )
        .await;
    assert!(
        result.is_err(),
        "a newer source version supersedes the old checked candidate"
    );
    assert!(submits.lock().unwrap().is_empty());
    assert!(client.checked_for_tests().await.is_some());
    assert_eq!(
        authority.reads.load(AtomicOrdering::Relaxed),
        0,
        "the mailbox version fence must reject before the final source probe"
    );
}

#[tokio::test]
async fn production_checked_apply_refuses_missing_authoritative_source_reader() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    // A compatibility fixture without the production Profiles document
    // lock and file source cannot silently treat the checked receipt as
    // authoritative. Only the composition root can install that reader.
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    client
        .checked_candidate(
            checked_runtime_fixture("checked-source-19"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 19, false);
    let error = client
        .apply_checked_committed(SourceDomain::Profiles, 19, "checked-source-19".into())
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("authoritative source verification is not connected")
    );
    assert!(submits.lock().unwrap().is_empty());
}

#[tokio::test]
async fn authoritative_source_change_refuses_apply_after_checked_admission() {
    let submits = Arc::new(Mutex::new(Vec::new()));
    let authority =
        TestSourceAuthority::for_source(SourceDomain::Profiles, 11, "checked-source-11");
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();
    client
        .checked_candidate(
            checked_runtime_fixture("checked-source-11"),
            checked_mihomo_spec(),
        )
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 11, false);
    // The Source mailbox's version may be unchanged even if managed
    // content changed on disk. The final authority read is mandatory.
    authority.current.lock().unwrap().2 = "new-content-target".into();
    let error = client
        .submit_checked_with_authority(
            SourceDomain::Profiles,
            11,
            "checked-source-11".into(),
            authority.clone(),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("authoritative current source"));
    assert_eq!(authority.reads.load(AtomicOrdering::Relaxed), 1);
    assert!(
        submits.lock().unwrap().is_empty(),
        "the Core must not receive stale checked bytes"
    );
}

struct EmptyManagedFiles;
impl chimera_config::runtime::executor::ProfileContentSource for EmptyManagedFiles {
    fn read(
        &self,
        _path: &chimera_config::profile::ManagedProfilePath,
    ) -> Result<String, chimera_config::runtime::executor::PortError> {
        Err("no managed files expected in this fixture".into())
    }
}

#[tokio::test]
async fn installed_production_authority_verifies_real_source_before_core_submit() {
    use chimera_config::profile::Profiles;
    let temp = tempfile::tempdir().unwrap();
    let mut manager = PersistentStateManagerSetup::<Profiles>::builder()
        .config_path(Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap())
        .assemble()
        .from_state(Profiles::default())
        .await
        .unwrap();
    let snapshot = manager.snapshot_handle();
    let reader = crate::state::mutation::ProfileContentReader::new(
        snapshot.clone(),
        crate::state::mutation::ProfileContentLock::default(),
    );
    let app = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = crate::client::reconciler::inputs::RuntimeInputs {
        app: app.get_typed(),
        clash: clash.get().unwrap(),
        profiles: Arc::new(Profiles::default()),
        content: Default::default(),
    }
    .checked_target_key()
    .unwrap();
    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let tasks = TaskTracker::new();
    let client = RuntimeReconcilerClient::spawn_with_profiles_and_content(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        app,
        clash,
        Some(snapshot),
        Some(reader),
        Some(Arc::new(EmptyManagedFiles)),
        RuntimePaths::from_config_root(temp.path().join("runtime")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService { events }),
        CancellationToken::new(),
        &tasks,
    )
    .await
    .unwrap();

    client
        .checked_candidate(checked_runtime_fixture(&target), checked_mihomo_spec())
        .await
        .unwrap();
    client.source_committed(SourceDomain::Profiles, 0, false);
    client
        .apply_checked_committed(SourceDomain::Profiles, 0, target.clone())
        .await
        .unwrap();
    assert_eq!(submits.lock().unwrap().len(), 1);
    assert_eq!(submits.lock().unwrap()[0].0, target);

    // The Profiles owner publishes a newer version, but the mailbox
    // notification is delayed. A version-only fence would still accept
    // the stale target. The installed authority must detect it directly.
    manager.upsert(Profiles::default()).await.unwrap();
    client
        .checked_candidate(checked_runtime_fixture(&target), checked_mihomo_spec())
        .await
        .unwrap();
    let error = client
        .apply_checked_committed(SourceDomain::Profiles, 0, target)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("committed Profiles version changed")
    );
    assert_eq!(
        submits.lock().unwrap().len(),
        1,
        "a commit whose notification has not arrived must not apply twice"
    );
}

#[tokio::test]
async fn same_domain_request_receipts_abort_one_and_apply_exact_committed_bytes() {
    use chimera_config::profile::Profiles;
    let temp = tempfile::tempdir().unwrap();
    let manager = PersistentStateManagerSetup::<Profiles>::builder()
        .config_path(Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap())
        .assemble()
        .from_state(Profiles::default())
        .await
        .unwrap();
    let snapshot = manager.snapshot_handle();
    let reader = crate::state::mutation::ProfileContentReader::new(
        snapshot.clone(),
        crate::state::mutation::ProfileContentLock::default(),
    );
    let app = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = crate::client::reconciler::inputs::RuntimeInputs {
        app: app.get_typed(),
        clash: clash.get().unwrap(),
        profiles: Arc::new(Profiles::default()),
        content: Default::default(),
    }
    .checked_target_key()
    .unwrap();
    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let tasks = TaskTracker::new();
    let client = RuntimeReconcilerClient::spawn_with_profiles_and_content(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        app,
        clash,
        Some(snapshot),
        Some(reader),
        Some(Arc::new(EmptyManagedFiles)),
        RuntimePaths::from_config_root(temp.path().join("runtime")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService { events }),
        CancellationToken::new(),
        &tasks,
    )
    .await
    .unwrap();

    let mut aborted = checked_mihomo_spec();
    aborted.binary_path = Utf8PathBuf::from("/tmp/aborted-clash-binary");
    let mut other = checked_mihomo_spec();
    other.binary_path = Utf8PathBuf::from("/tmp/uncommitted-clash-binary");
    let mut committed = checked_mihomo_spec();
    committed.binary_path = Utf8PathBuf::from("/tmp/committed-clash-binary");

    for (request, spec) in [
        ("abort-me", aborted),
        ("other-uncommitted", other),
        ("commit-me", committed.clone()),
    ] {
        client
            .checked_candidate_for_domain_with_request(
                SourceDomain::Clash,
                checked_runtime_fixture(&target),
                spec,
                request.into(),
            )
            .await
            .unwrap();
    }
    client.abort_uncommitted_checked_request(SourceDomain::Clash, "abort-me".into());
    client.checked_source_committed_for_request(SourceDomain::Clash, 0, "commit-me".into());

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if submits.lock().unwrap().len() == 1 && client.status().active.is_none() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the exact committed Clash request should be applied");

    let submitted = submits.lock().unwrap().clone();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].0, target);
    assert_eq!(submitted[0].3, committed.binary_path.to_string());
    assert_eq!(
        client.checked_for_tests().await.unwrap().0.as_deref(),
        Some(target.as_str()),
        "unrelated same-domain proposal remains available"
    );
    client.abort_uncommitted_checked_request(SourceDomain::Clash, "other-uncommitted".into());
    assert!(client.checked_for_tests().await.is_none());
    assert_eq!(submits.lock().unwrap().len(), 1, "no second Core apply");
}

#[tokio::test]
async fn checked_gate_postcommit_uses_one_core_apply_without_legacy_rebuild() {
    use chimera_config::profile::Profiles;
    let temp = tempfile::tempdir().unwrap();
    let mut manager = PersistentStateManagerSetup::<Profiles>::builder()
        .config_path(Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap())
        .assemble()
        .from_state(Profiles::default())
        .await
        .unwrap();
    let snapshot = manager.snapshot_handle();
    let reader = crate::state::mutation::ProfileContentReader::new(
        snapshot.clone(),
        crate::state::mutation::ProfileContentLock::default(),
    );
    let app = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = crate::client::reconciler::inputs::RuntimeInputs {
        app: app.get_typed(),
        clash: clash.get().unwrap(),
        profiles: Arc::new(Profiles::default()),
        content: Default::default(),
    }
    .checked_target_key()
    .unwrap();
    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let tasks = TaskTracker::new();
    let client = RuntimeReconcilerClient::spawn_with_profiles_and_content(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        app,
        clash,
        Some(snapshot),
        Some(reader),
        Some(Arc::new(EmptyManagedFiles)),
        RuntimePaths::from_config_root(temp.path().join("runtime")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService { events }),
        CancellationToken::new(),
        &tasks,
    )
    .await
    .unwrap();

    use crate::client::reconciler::{
        check::CheckedCandidatePort, composition::ReconcilerCheckedInbox,
    };
    use chimera_contracts::mutation::ConfigDomain;
    let inbox = ReconcilerCheckedInbox::new(client.clone());
    let mut spec = checked_mihomo_spec();
    spec.binary_path = Utf8PathBuf::from("/tmp/only-checked-spec-may-run");
    inbox
        .checked(
            ConfigDomain::Profiles,
            checked_runtime_fixture(&target),
            spec.clone(),
        )
        .await
        .unwrap();
    inbox.source_committed(ConfigDomain::Profiles, chimera_core::state::Version::new(0));
    tokio::time::timeout(Duration::from_secs(3), async {
        while submits.lock().unwrap().is_empty() || client.status().active.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("checked source notice should dispatch the one Core apply");
    let submitted = submits.lock().unwrap().clone();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].0, target);
    assert_eq!(submitted[0].3, spec.binary_path.to_string());
    assert!(client.checked_for_tests().await.is_none());
    let completed = client.status().completed.len();
    assert!(completed >= 1);
    assert!(
        client
            .status()
            .completed
            .iter()
            .all(|record| record.error.is_none()),
        "no parallel legacy rebuild can be permitted for a checked source"
    );
    // A duplicated source notification is not another runtime change.
    inbox.source_committed(ConfigDomain::Profiles, chimera_core::state::Version::new(0));
    client.checked_for_tests().await;
    assert_eq!(submits.lock().unwrap().len(), 1);
    assert_eq!(
        client.status().completed.len(),
        completed,
        "a repeated checked notification must not queue a legacy rebuild"
    );
}

#[tokio::test]
async fn interleaved_checked_domains_keep_original_core_specs_and_apply_independently() {
    use chimera_config::profile::Profiles;
    let temp = tempfile::tempdir().unwrap();
    let mut manager = PersistentStateManagerSetup::<Profiles>::builder()
        .config_path(Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap())
        .assemble()
        .from_state(Profiles::default())
        .await
        .unwrap();
    let snapshot = manager.snapshot_handle();
    let reader = crate::state::mutation::ProfileContentReader::new(
        snapshot.clone(),
        crate::state::mutation::ProfileContentLock::default(),
    );
    let app = ApplicationClient::legacy().unwrap();
    let clash = ClashConfigClient::legacy().unwrap();
    let target = crate::client::reconciler::inputs::RuntimeInputs {
        app: app.get_typed(),
        clash: clash.get().unwrap(),
        profiles: Arc::new(Profiles::default()),
        content: Default::default(),
    }
    .checked_target_key()
    .unwrap();
    let submits = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let tasks = TaskTracker::new();
    let client = RuntimeReconcilerClient::spawn_with_profiles_and_content(
        Arc::new(ExactCheckedCore {
            submits: submits.clone(),
        }),
        app,
        clash,
        Some(snapshot),
        Some(reader),
        Some(Arc::new(EmptyManagedFiles)),
        RuntimePaths::from_config_root(temp.path().join("runtime")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
        Arc::new(RecordingService { events }),
        CancellationToken::new(),
        &tasks,
    )
    .await
    .unwrap();

    use crate::client::reconciler::{
        check::CheckedCandidatePort, composition::ReconcilerCheckedInbox,
    };
    use chimera_contracts::mutation::ConfigDomain;
    let inbox = ReconcilerCheckedInbox::new(client.clone());
    let mut profiles_spec = checked_mihomo_spec();
    profiles_spec.binary_path = Utf8PathBuf::from("/tmp/checked-only-profiles-core");
    let mut application_spec = checked_mihomo_spec();
    application_spec.binary_path = Utf8PathBuf::from("/tmp/checked-only-application-core");
    // The earlier Profiles check cannot be replaced by a later
    // Application check. An Application commit can arrive first.
    inbox
        .checked(
            ConfigDomain::Profiles,
            checked_runtime_fixture(&target),
            profiles_spec.clone(),
        )
        .await
        .unwrap();
    inbox
        .checked(
            ConfigDomain::Application,
            checked_runtime_fixture(&target),
            application_spec.clone(),
        )
        .await
        .unwrap();
    inbox.source_committed(
        ConfigDomain::Application,
        chimera_core::state::Version::new(0),
    );
    tokio::time::timeout(Duration::from_secs(4), async {
        while submits.lock().unwrap().len() < 1 || client.status().active.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Application checked source should submit the originally validated CoreSpec");
    let first = submits.lock().unwrap().clone();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].0, target);
    assert_eq!(first[0].3, application_spec.binary_path.to_string());
    assert_eq!(
        client.checked_for_tests().await.unwrap().0.as_deref(),
        Some(target.as_str()),
        "the uncommitted Profiles candidate remains owned by the same Actor"
    );
    inbox.source_committed(ConfigDomain::Profiles, chimera_core::state::Version::new(0));
    tokio::time::timeout(Duration::from_secs(4), async {
        while submits.lock().unwrap().len() < 2 || client.status().active.is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Profiles checked source should submit the other validated CoreSpec");
    let both = submits.lock().unwrap().clone();
    assert_eq!(both.len(), 2);
    assert_eq!(both[1].0, target);
    assert_eq!(both[1].3, profiles_spec.binary_path.to_string());
    assert!(client.checked_for_tests().await.is_none());
    assert!(
        client
            .status()
            .completed
            .iter()
            .all(|entry| entry.error.is_none()),
        "no legacy rebuild or unrelated Core executor may run"
    );
    // Duplicates cannot replay either consumed source, regardless of
    // incoming order or domains.
    inbox.source_committed(
        ConfigDomain::Application,
        chimera_core::state::Version::new(0),
    );
    inbox.source_committed(ConfigDomain::Profiles, chimera_core::state::Version::new(0));
    client.checked_for_tests().await;
    assert_eq!(submits.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn reconcile_runs_inside_lifecycle_mailbox() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let client = RuntimeReconcilerClient::spawn(
        Arc::new(RecordingCore {
            events: events.clone(),
            stop_started: Arc::new(Notify::new()),
            release_stop: Arc::new(Notify::new()),
            recovery_notify: Arc::new(Notify::new()),
            run_type: RunType::Normal,
        }),
        ApplicationClient::legacy().unwrap(),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
    )
    .await
    .unwrap();

    client.reconcile().await.unwrap();

    assert_eq!(events.lock().unwrap().as_slice(), ["rebuild"]);
}

async fn run_binary_update(
    desired_core: ClashCore,
    state: CoreState,
    active_kind: Option<chimera_core_manager::CoreKind>,
    target: ClashCore,
    state_changed_at: i64,
    receipt_error: bool,
) -> Vec<&'static str> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.core = desired_core;
    let core = Arc::new(CoreUpdateRecordingCore {
        events: events.clone(),
        state,
        state_changed_at,
        active_kind,
        receipt_error,
    });
    let client = RuntimeReconcilerClient::spawn_with_installer(
        core,
        ApplicationClient::static_state(app),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();
    let staging = Arc::new(tempfile::tempdir().unwrap());
    client
        .replace_core_binary(PreparedCoreBinary {
            target,
            source: staging.path().join("staged-core"),
            destination: staging.path().join("installed-core"),
            staging,
            progress: Arc::new(RecordingProgress {
                events: events.clone(),
            }),
        })
        .await
        .unwrap();
    events.lock().unwrap().clone()
}

#[tokio::test]
async fn core_binary_workflow_copies_through_upstream_os_installer_without_elevation() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.core = ClashCore::Mihomo;
    let client = RuntimeReconcilerClient::spawn_with_installer(
        Arc::new(CoreUpdateRecordingCore {
            events: events.clone(),
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            active_kind: None,
            receipt_error: false,
        }),
        ApplicationClient::static_state(app),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
    )
    .await
    .unwrap();
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let source = staging.path().join("staged-mihomo");
    std::fs::write(&source, b"new-mihomo-binary").unwrap();
    let destination_dir = tempfile::tempdir().unwrap();
    let destination = destination_dir.path().join("mihomo");
    std::fs::write(&destination, b"old-mihomo-binary").unwrap();

    client
        .replace_core_binary(PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source,
            destination: destination.clone(),
            staging,
            progress: Arc::new(RecordingProgress {
                events: events.clone(),
            }),
        })
        .await
        .unwrap();

    assert_eq!(std::fs::read(destination).unwrap(), b"new-mihomo-binary");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["restarting", "rebuild", "finished"]
    );
}

#[tokio::test]
async fn refused_os_core_copy_keeps_target_and_does_not_restart() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut app = chimera_config::application::ChimeraAppConfig::default();
    app.core = ClashCore::Mihomo;
    let client = RuntimeReconcilerClient::spawn_with_installer(
        Arc::new(CoreUpdateRecordingCore {
            events: events.clone(),
            state: CoreState::Stopped(None),
            state_changed_at: 1,
            active_kind: None,
            receipt_error: false,
        }),
        ApplicationClient::static_state(app),
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(chimera_os::core_install::FsBinaryInstaller::new(Arc::new(
            chimera_os::elevation::RefuseElevation,
        ))),
    )
    .await
    .unwrap();

    let staging = Arc::new(tempfile::tempdir().unwrap());
    let source = staging.path().join("staged-mihomo");
    std::fs::write(&source, b"new-mihomo-binary").unwrap();
    let destination_dir = tempfile::tempdir().unwrap();
    let destination = destination_dir.path().join("mihomo");
    // A directory is a deterministic copy failure on all supported OSes;
    // no permissions are changed and no real elevation is attempted.
    std::fs::create_dir(&destination).unwrap();
    let marker = destination.join("existing-core-marker");
    std::fs::write(&marker, b"preserved").unwrap();

    let error = client
        .replace_core_binary(PreparedCoreBinary {
            target: ClashCore::Mihomo,
            source: source.clone(),
            destination: destination.clone(),
            staging,
            progress: Arc::new(RecordingProgress {
                events: events.clone(),
            }),
        })
        .await
        .expect_err("refused elevated fallback must fail the update");

    assert!(
        error.to_string().contains("could not authorize copying"),
        "unexpected failure: {error:#}"
    );
    assert_eq!(std::fs::read(&marker).unwrap(), b"preserved");
    // The staged source is owned by the update request. Once the failed
    // operation completes, its temporary directory is intentionally gone.
    assert!(!source.exists());
    assert!(destination.is_dir());
    assert_eq!(events.lock().unwrap().as_slice(), ["finished-error"]);
}

#[tokio::test]
async fn updating_an_inactive_core_does_not_stop_or_restart_the_confirmed_active_core() {
    let events = run_binary_update(
        ClashCore::Mihomo,
        CoreState::Running,
        Some(chimera_core_manager::CoreKind::Mihomo),
        ClashCore::ClashRs,
        1,
        false,
    )
    .await;

    assert_eq!(events, ["install", "finished"]);
}

#[tokio::test]
async fn updating_the_running_core_stops_before_install_even_when_it_is_not_selected() {
    let events = run_binary_update(
        ClashCore::Mihomo,
        CoreState::Running,
        Some(chimera_core_manager::CoreKind::ClashRust),
        ClashCore::ClashRs,
        1,
        false,
    )
    .await;

    assert_eq!(
        events,
        ["stop", "install", "restarting", "rebuild", "finished"]
    );
}

#[tokio::test]
async fn an_unidentified_running_core_is_stopped_before_binary_replacement() {
    let events = run_binary_update(
        ClashCore::Mihomo,
        CoreState::Running,
        None,
        ClashCore::ClashRs,
        1,
        false,
    )
    .await;

    assert_eq!(
        events,
        ["stop", "install", "restarting", "rebuild", "finished"]
    );
}

#[tokio::test]
async fn unavailable_service_status_is_not_treated_as_proven_stopped() {
    let events = run_binary_update(
        ClashCore::Mihomo,
        CoreState::Stopped(None),
        None,
        ClashCore::ClashRs,
        0,
        true,
    )
    .await;

    assert_eq!(
        events,
        ["stop", "install", "restarting", "rebuild", "finished"]
    );
}

#[tokio::test]
async fn binary_replacement_of_a_stopped_selected_core_installs_then_restarts() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let application = ApplicationClient::legacy().unwrap();
    let target = application.get_typed().core;
    let runtime_paths =
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root"));
    let client = RuntimeReconcilerClient::spawn_with_installer(
        Arc::new(ConfirmedStoppedCore {
            events: events.clone(),
            run_type: RunType::Service,
        }),
        application,
        ClashConfigClient::legacy().unwrap(),
        runtime_paths,
        Arc::new(RecordingInstaller {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let artifact = PreparedCoreBinary {
        target,
        source: staging.path().join("staged-core"),
        destination: staging.path().join("installed-core"),
        staging,
        progress: Arc::new(RecordingProgress {
            events: events.clone(),
        }),
    };

    client.replace_core_binary(artifact).await.unwrap();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["install", "restarting", "rebuild", "finished"]
    );
}

#[tokio::test]
async fn binary_install_failure_does_not_stop_or_restart_a_stopped_core() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let application = ApplicationClient::legacy().unwrap();
    let target = application.get_typed().core;
    let client = RuntimeReconcilerClient::spawn_with_installer(
        Arc::new(ConfirmedStoppedCore {
            events: events.clone(),
            run_type: RunType::Normal,
        }),
        application,
        ClashConfigClient::legacy().unwrap(),
        RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        Arc::new(FailingInstaller {
            events: events.clone(),
        }),
    )
    .await
    .unwrap();
    let staging = Arc::new(tempfile::tempdir().unwrap());
    let artifact = PreparedCoreBinary {
        target,
        source: staging.path().join("staged-core"),
        destination: staging.path().join("installed-core"),
        staging,
        progress: Arc::new(RecordingProgress {
            events: events.clone(),
        }),
    };

    assert!(client.replace_core_binary(artifact).await.is_err());

    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["install", "finished-error"]
    );
}
