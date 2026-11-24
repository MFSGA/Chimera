use std::sync::atomic::{AtomicBool, Ordering};

use atomic_enum::atomic_enum;
use chimera_ipc::types::ServiceStatus;
use serde::Serialize;
use tokio::sync::{MutexGuard, Notify};
use tracing::instrument;

use crate::client::ChimeraClient;

use chimera_core::{
    actor_v2::{
        endpoint::ExecutionHost,
        router::{CoreStatusProjection, EndpointConnectivity},
        service_command_actor::ServiceHealthRounds,
    },
    service::compat::ServiceCompat,
};

#[derive(PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[atomic_enum]
pub enum IpcState {
    Connected,
    Disconnected,
}

impl IpcState {
    pub fn is_connected(&self) -> bool {
        *self == IpcState::Connected
    }
}

static IPC_STATE: AtomicIpcState = AtomicIpcState::new(IpcState::Disconnected);
pub(super) static KILL_FLAG: AtomicBool = AtomicBool::new(false);
pub(super) static HEALTH_CHECK_RUNNING: AtomicBool = AtomicBool::new(false);
static HEALTH_ROUNDS: ServiceHealthRounds = ServiceHealthRounds::new();
static SERVICE_TRANSITION_EPOCH: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);
static HEALTH_CHECK_WAKE: Notify = Notify::const_new();
static HEALTH_CHECK_CHANGED: Notify = Notify::const_new();
const IPC_TRANSITION_CORE_STATUS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

fn notify_health_check_changed() {
    HEALTH_CHECK_CHANGED.notify_waiters();
    HEALTH_CHECK_CHANGED.notify_one();
}

/// Invalidate outstanding health probes around a serialized Service transition
/// so they cannot publish an obsolete Service observation.
pub(super) fn stop_health_check() {
    HEALTH_ROUNDS.set_enabled(false);
    KILL_FLAG.store(true, Ordering::Release);
    HEALTH_CHECK_WAKE.notify_one();
    notify_health_check_changed();
}

/// Start a new Service transition generation and retire any health monitor
/// that observed the previous generation. Call while holding the host lock.
pub(super) fn retire_health_check() -> u64 {
    SERVICE_TRANSITION_EPOCH.fetch_add(1, Ordering::AcqRel);
    stop_health_check();
    service_transition_epoch()
}

pub(super) fn service_transition_epoch() -> u64 {
    SERVICE_TRANSITION_EPOCH.load(Ordering::Acquire)
}

fn transition_is_retired(rounds: &ServiceHealthRounds, kill_flag: &AtomicBool, round: u64) -> bool {
    kill_flag.load(Ordering::Acquire) || !rounds.is_current(round)
}

async fn acquire_host_transition<'a>(
    shutdown: &tokio_util::sync::CancellationToken,
    health_round: Option<u64>,
    rounds: &ServiceHealthRounds,
    kill_flag: &AtomicBool,
    wake: &Notify,
    transition_lock: &'a tokio::sync::Mutex<()>,
) -> Option<MutexGuard<'a, ()>> {
    loop {
        if shutdown.is_cancelled()
            || health_round.is_some_and(|round| transition_is_retired(rounds, kill_flag, round))
        {
            return None;
        }

        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return None,
            _ = wake.notified(), if health_round.is_some() => {
                if health_round.is_some_and(|round| transition_is_retired(rounds, kill_flag, round)) {
                    return None;
                }
            }
            transition = transition_lock.lock() => {
                if shutdown.is_cancelled()
                    || health_round.is_some_and(|round| transition_is_retired(rounds, kill_flag, round))
                {
                    return None;
                }
                return Some(transition);
            }
        }
    }
}

async fn wait_for_health_monitor_retirement(
    running: &AtomicBool,
    kill_flag: &AtomicBool,
    changed: &Notify,
    shutdown: &tokio_util::sync::CancellationToken,
) -> bool {
    loop {
        if shutdown.is_cancelled() {
            return false;
        }
        if !running.load(Ordering::Acquire) || !kill_flag.load(Ordering::Acquire) {
            return true;
        }

        let changed = changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        if !running.load(Ordering::Acquire) || !kill_flag.load(Ordering::Acquire) {
            continue;
        }
        tokio::select! {
            biased;
            _ = shutdown.cancelled() => return false,
            _ = &mut changed => {}
        }
    }
}

async fn await_core_status_or_shutdown<T>(
    shutdown: &tokio_util::sync::CancellationToken,
    status: impl std::future::Future<Output = anyhow::Result<T>>,
    timeout: std::time::Duration,
) -> Option<anyhow::Result<T>> {
    tokio::select! {
        biased;
        _ = shutdown.cancelled() => None,
        result = tokio::time::timeout(timeout, status) => Some(result.unwrap_or_else(|_| {
            Err(anyhow::anyhow!("core status read timed out after {} ms", timeout.as_millis()))
        })),
    }
}

pub fn get_ipc_state() -> IpcState {
    IPC_STATE.load(Ordering::Relaxed)
}

pub(crate) struct ServiceBackendObservation {
    pub status: ServiceStatus,
    pub compat: ServiceCompat,
    pub runtime_owned: bool,
}

impl ServiceBackendObservation {
    pub fn is_ready(&self) -> bool {
        self.status == ServiceStatus::Running
            && self.compat.allows_service_backend()
            && self.runtime_owned
    }
}

/// Probe the daemon synchronously and make the process-wide IPC classification
/// match that observation without scheduling a second rebuild. Callers that
/// need an immediate host transition can then perform exactly one explicit
/// rebuild against the freshly classified backend.
pub(crate) async fn refresh_state_now() -> anyhow::Result<ServiceBackendObservation> {
    let info = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        super::control::probe_service_status(),
    )
    .await
    {
        Ok(Ok(info)) => info,
        Ok(Err(error)) => return Err(error.into()),
        Err(_) => anyhow::bail!("service status probe timed out"),
    };
    let runtime_owned = super::is_service_runtime_owned(&info);
    let (state, compat) = target_ipc_state(&info, runtime_owned);
    IPC_STATE.store(state, Ordering::SeqCst);
    Ok(ServiceBackendObservation {
        status: info.status,
        compat,
        runtime_owned,
    })
}

/// Wait for a just-started daemon to expose a compatible endpoint owned by
/// this Chimera runtime. Incompatible or foreign runtimes fail immediately;
/// startup/transient probe failures are retried within the bounded window.
pub(crate) async fn wait_until_ready(
    timeout: std::time::Duration,
) -> anyhow::Result<ServiceBackendObservation> {
    let started = std::time::Instant::now();
    loop {
        let last_error = match refresh_state_now().await {
            Ok(observation) if observation.is_ready() => return Ok(observation),
            Ok(observation) => {
                if matches!(
                    observation.compat,
                    ServiceCompat::Incompatible { .. } | ServiceCompat::Unparsable { .. }
                ) {
                    anyhow::bail!("service daemon is incompatible: {:?}", observation.compat);
                }
                if observation.status == ServiceStatus::Running
                    && observation.compat.allows_service_backend()
                    && !observation.runtime_owned
                {
                    anyhow::bail!("service daemon belongs to another runtime");
                }
                format!(
                    "service status is {:?}, compatibility is {:?}, runtime_owned={}",
                    observation.status, observation.compat, observation.runtime_owned
                )
            }
            Err(error) => error.to_string(),
        };

        if started.elapsed() >= timeout {
            anyhow::bail!("service backend did not become ready: {last_error}");
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

pub(crate) fn mark_disconnected_now() {
    IPC_STATE.store(IpcState::Disconnected, Ordering::SeqCst);
}

pub(super) fn set_ipc_state(state: IpcState, client: &ChimeraClient) {
    IPC_STATE.store(state, Ordering::Relaxed);
    on_ipc_state_changed(state, client, None);
}

fn dispatch_disconnected(client: &ChimeraClient, health_round: Option<u64>) -> bool {
    if IPC_STATE
        .compare_exchange(
            IpcState::Connected,
            IpcState::Disconnected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Disconnected, client, health_round);
        true
    } else {
        false
    }
}

fn dispatch_connected(client: &ChimeraClient, health_round: Option<u64>) {
    if IPC_STATE
        .compare_exchange(
            IpcState::Disconnected,
            IpcState::Connected,
            Ordering::SeqCst,
            Ordering::Relaxed,
        )
        .is_ok()
    {
        on_ipc_state_changed(IpcState::Connected, client, health_round)
    }
}

/// Service IPC is a health observation, not a second owner for core host
/// selection. Trigger a reconcile only when the committed ref CoreActor host
/// disagrees with Service availability. A degraded endpoint retains its
/// committed host and can need recovery, but an in-flight handoff must never
/// receive a competing reconcile from the legacy health monitor.
fn should_rebuild_for_ipc_transition(state: IpcState, projection: &CoreStatusProjection) -> bool {
    if matches!(
        projection.connectivity,
        EndpointConnectivity::HandingOff { .. } | EndpointConnectivity::ShutDown
    ) {
        return false;
    }
    match state {
        IpcState::Connected => projection.host == ExecutionHost::Local,
        IpcState::Disconnected => projection.host == ExecutionHost::Service,
    }
}

fn should_rebuild_for_ipc_event(
    observed_state: IpcState,
    current_state: IpcState,
    projection: &CoreStatusProjection,
) -> bool {
    observed_state == current_state && should_rebuild_for_ipc_transition(observed_state, projection)
}

#[instrument(skip(client))]
fn on_ipc_state_changed(state: IpcState, client: &ChimeraClient, health_round: Option<u64>) {
    tracing::info!("IPC state changed: {:?}", state);
    let client = client.clone();
    let shutdown = client.shutdown_child_token();
    let _ = client.clone().spawn_tracked(async move {
        let Some(_transition) = acquire_host_transition(
            &shutdown,
            health_round,
            &HEALTH_ROUNDS,
            &KILL_FLAG,
            &HEALTH_CHECK_WAKE,
            &super::HOST_TRANSITION_LOCK,
        )
        .await
        else {
            return;
        };

        let enabled_service = match client.get_app_config() {
            Ok(config) => config.enable_service_mode,
            Err(error) => {
                tracing::warn!(
                    "failed to read Service Mode during service IPC transition: {error}"
                );
                return;
            }
        };
        if !enabled_service || shutdown.is_cancelled() {
            return;
        }

        let current_state = get_ipc_state();
        if current_state != state {
            tracing::debug!(
                observed = ?state,
                current = ?current_state,
                "discarding stale service IPC transition"
            );
            return;
        }

        let Some(status) = await_core_status_or_shutdown(
            &shutdown,
            client.refreshed_routed_core_status(),
            IPC_TRANSITION_CORE_STATUS_TIMEOUT,
        )
        .await
        else {
            return;
        };
        let status = match status {
            Ok(status) => status,
            Err(err) => {
                tracing::warn!("failed to read ref CoreActor during service IPC transition: {err}");
                return;
            }
        };

        // refresh_status is asynchronous; Service may have changed again
        // while its Core endpoint answered. Never act on the earlier
        // current_state sample after that await.
        let latest_state = get_ipc_state();
        if !shutdown.is_cancelled()
            && !health_round.is_some_and(health_round_retired)
            && should_rebuild_for_ipc_event(state, latest_state, &status)
        {
            tracing::info!(
                host = ?status.host,
                generation = status.generation,
                connectivity = ?status.connectivity,
                "Marking runtime dirty due to Service IPC host mismatch"
            );
            client.request_runtime_rebuild();
        }
    });
}

/// Preserve the health-check recovery behavior after a synchronous Service
/// transition has already updated `IPC_STATE` but the immediate core handoff
/// failed. This deliberately reuses the same guarded reconcile path rather
/// than introducing a second host-switch implementation.
pub(crate) fn request_reconcile(client: &ChimeraClient) {
    on_ipc_state_changed(get_ipc_state(), client, None);
}

fn claim_health_check(running: &AtomicBool) -> bool {
    running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

fn health_round_retired(round: u64) -> bool {
    KILL_FLAG.load(Ordering::Acquire) || !HEALTH_ROUNDS.is_current(round)
}

fn spawn_health_check(client: ChimeraClient) {
    KILL_FLAG.store(false, Ordering::Release);
    let round = HEALTH_ROUNDS
        .set_enabled(true)
        .expect("a started health monitor owns an enabled round");
    notify_health_check_changed();
    // Keep admission owned until the task exits, including on unwind.
    struct RunningGuard;
    impl Drop for RunningGuard {
        fn drop(&mut self) {
            HEALTH_ROUNDS.set_enabled(false);
            HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
            notify_health_check_changed();
        }
    }

    let shutdown = client.shutdown_child_token();
    if !client.clone().spawn_tracked(async move {
        let _running = RunningGuard;
        let mut warned_ineligible = false;
        loop {
            if shutdown.is_cancelled() || health_round_retired(round) {
                // A successful stop/uninstall invalidates this round. Its late
                // exit must not overwrite a newer synchronous host handoff or
                // a replacement monitor's IPC state.
                break;
            }
            let Some(_transition) = acquire_host_transition(
                &shutdown,
                Some(round),
                &HEALTH_ROUNDS,
                &KILL_FLAG,
                &HEALTH_CHECK_WAKE,
                &super::HOST_TRANSITION_LOCK,
            )
            .await
            else {
                break;
            };
            // Let a started health transition finish before AppShutdown begins
            // core cleanup; cancellation is observed between complete probes.
            let (next_warned, endpoint_down) =
                health_check(warned_ineligible, &client, round).await;
            drop(_transition);
            if endpoint_down && !shutdown.is_cancelled() && !health_round_retired(round) {
                // Endpoint recovery takes its own Service transition lease, so
                // submit it only after releasing the probe's host lock. Do not
                // keep shutdown draining on a lease that has not been acquired.
                let recovery_shutdown = shutdown.clone();
                tokio::select! {
                    biased;
                    _ = shutdown.cancelled() => break,
                    _ = client.request_service_endpoint_down_if(move || {
                        !recovery_shutdown.is_cancelled() && !health_round_retired(round)
                    }) => {}
                }
            }
            warned_ineligible = next_warned;
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => break,
                _ = HEALTH_CHECK_WAKE.notified() => {
                    if health_round_retired(round) {
                        break;
                    }
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
            }
        }
    }) {
        HEALTH_ROUNDS.set_enabled(false);
        HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
        notify_health_check_changed();
    }
}

pub(crate) async fn ensure_health_check_for_epoch(client: ChimeraClient, expected_epoch: u64) {
    let shutdown = client.shutdown_child_token();
    loop {
        if shutdown.is_cancelled() || service_transition_epoch() != expected_epoch {
            return;
        }
        if claim_health_check(&HEALTH_CHECK_RUNNING) {
            let transition = tokio::select! {
                biased;
                _ = shutdown.cancelled() => None,
                transition = super::HOST_TRANSITION_LOCK.lock() => Some(transition),
            };
            let Some(_transition) = transition else {
                HEALTH_ROUNDS.set_enabled(false);
                HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                notify_health_check_changed();
                return;
            };
            if shutdown.is_cancelled() || service_transition_epoch() != expected_epoch {
                HEALTH_ROUNDS.set_enabled(false);
                HEALTH_CHECK_RUNNING.store(false, Ordering::Release);
                notify_health_check_changed();
                return;
            }
            spawn_health_check(client);
            return;
        }
        if !KILL_FLAG.load(Ordering::Acquire) {
            return;
        }
        if !wait_for_health_monitor_retirement(
            &HEALTH_CHECK_RUNNING,
            &KILL_FLAG,
            &HEALTH_CHECK_CHANGED,
            &shutdown,
        )
        .await
        {
            return;
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum WarnLevel {
    Warn,
    Debug,
    Silent,
}

fn next_ineligible_warning_state(warned: bool, ineligible_but_running: bool) -> (WarnLevel, bool) {
    match (ineligible_but_running, warned) {
        (true, false) => (WarnLevel::Warn, true),
        (true, true) => (WarnLevel::Debug, true),
        (false, _) => (WarnLevel::Silent, false),
    }
}

fn target_ipc_state(
    info: &chimera_ipc::types::StatusInfo<'_>,
    runtime_owned: bool,
) -> (IpcState, ServiceCompat) {
    let compat = ServiceCompat::classify(info);
    let state = match info.status {
        ServiceStatus::Running if compat.allows_service_backend() && runtime_owned => {
            IpcState::Connected
        }
        _ => IpcState::Disconnected,
    };
    (state, compat)
}

#[instrument(skip(client))]
async fn health_check(warned: bool, client: &ChimeraClient, round: u64) -> (bool, bool) {
    match super::control::probe_service_status().await {
        Ok(info) => {
            if !HEALTH_ROUNDS.is_current(round) {
                return (warned, false);
            }
            let runtime_owned = super::is_service_runtime_owned(&info);
            let (state, compat) = target_ipc_state(&info, runtime_owned);
            let ineligible_but_running =
                info.status == ServiceStatus::Running && (state == IpcState::Disconnected);
            let (level, next_warned) =
                next_ineligible_warning_state(warned, ineligible_but_running);

            match level {
                WarnLevel::Warn => tracing::warn!(
                    ?compat,
                    runtime_owned,
                    "service daemon is ineligible; core will continue on local backend"
                ),
                WarnLevel::Debug => tracing::debug!(
                    ?compat,
                    runtime_owned,
                    "service daemon remains ineligible; core will continue on local backend"
                ),
                WarnLevel::Silent => {}
            }

            let endpoint_down = match state {
                IpcState::Connected => {
                    dispatch_connected(client, Some(round));
                    false
                }
                IpcState::Disconnected => dispatch_disconnected(client, Some(round)),
            };
            if HEALTH_ROUNDS.is_current(round) {
                client.observe_service_status(info);
            }
            (next_warned, endpoint_down)
        }
        Err(e) => {
            if !HEALTH_ROUNDS.is_current(round) {
                return (warned, false);
            }
            tracing::error!("IPC health check failed: {}", e);
            client.observe_service_probe_failure();
            let endpoint_down = dispatch_disconnected(client, Some(round));
            let (_, next_warned) = next_ineligible_warning_state(warned, false);
            (next_warned, endpoint_down)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        borrow::Cow,
        path::PathBuf,
        sync::{
            Arc, Barrier,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
    };

    use chimera_ipc::{
        api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody},
        types::{ServiceStatus, StatusInfo},
    };

    use super::{
        HEALTH_ROUNDS, IpcState, KILL_FLAG, ServiceBackendObservation, ServiceHealthRounds,
        WarnLevel, acquire_host_transition, await_core_status_or_shutdown, claim_health_check,
        health_round_retired, next_ineligible_warning_state, retire_health_check,
        service_transition_epoch, should_rebuild_for_ipc_event, should_rebuild_for_ipc_transition,
        stop_health_check, target_ipc_state, wait_for_health_monitor_retirement,
    };
    use chimera_core::actor_v2::{
        endpoint::{CoreStatusSnapshot, ExecutionHost},
        router::{CoreStatusProjection, EndpointConnectivity},
    };
    use chimera_core::service::compat::{REQUIRED_SERVICE_MIN, ServiceCompat};
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    fn running_status(server_version: &str) -> StatusInfo<'static> {
        StatusInfo {
            name: Cow::Borrowed("chimera-service"),
            version: Cow::Borrowed("1.9.0"),
            status: ServiceStatus::Running,
            server: Some(StatusResBody {
                log_query_version: None,
                version: Cow::Owned(server_version.to_owned()),
                core_infos: CoreInfos {
                    instance_id: None,
                    r#type: None,
                    state: CoreState::Stopped(None),
                    state_changed_at: 0,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail: None,
                },
                runtime_infos: RuntimeInfos {
                    service_data_dir: Cow::Owned(PathBuf::new()),
                    service_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_data_dir: Cow::Owned(PathBuf::new()),
                },
            }),
        }
    }

    #[test]
    fn service_transition_retirement_invalidates_health_round_and_advances_epoch() {
        // This is the shared retirement hook used before explicit Service
        // commands. It invalidates the current health round and advances the
        // generation checked by monitor registration.
        let round = HEALTH_ROUNDS.set_enabled(true).unwrap();
        let expected_epoch = service_transition_epoch();
        KILL_FLAG.store(false, Ordering::Release);
        assert!(HEALTH_ROUNDS.is_current(round));
        assert!(!health_round_retired(round));
        let retired_epoch = retire_health_check();
        assert_ne!(retired_epoch, expected_epoch);
        assert_ne!(service_transition_epoch(), expected_epoch);
        assert!(!HEALTH_ROUNDS.is_current(round));
        assert!(KILL_FLAG.load(Ordering::Acquire));
        assert!(health_round_retired(round));
        // A retired monitor exits without dispatching a second disconnected
        // transition; the serialized Service handoff owns that publication.
        KILL_FLAG.store(false, Ordering::Release);
    }

    #[test]
    fn late_stop_completion_does_not_supersede_a_newer_start_transition() {
        // A Service stop completion can arrive after a later start has already
        // retired the previous monitor. It must signal retirement without
        // advancing the newer transition's expected epoch.
        let expected_epoch = retire_health_check();
        assert_eq!(service_transition_epoch(), expected_epoch);
        stop_health_check();
        assert_eq!(service_transition_epoch(), expected_epoch);
        KILL_FLAG.store(false, Ordering::Release);
    }

    /// Initial state: a health result owns the current generation but waits
    /// behind a serialized Service transition. Operation: stop invalidates the
    /// generation and wakes that waiter. Expected result: it cannot acquire the
    /// transition slot and publish the stale result; this fails if generation
    /// validation happens only before the lock wait.
    #[tokio::test]
    async fn retired_health_round_cannot_publish_after_waiting_for_host_lock() {
        let rounds = ServiceHealthRounds::new();
        let round = rounds.set_enabled(true).unwrap();
        let kill = AtomicBool::new(false);
        let wake = Notify::new();
        let transition_lock = tokio::sync::Mutex::new(());
        let shutdown = CancellationToken::new();
        let held = transition_lock.lock().await;

        let acquire = acquire_host_transition(
            &shutdown,
            Some(round),
            &rounds,
            &kill,
            &wake,
            &transition_lock,
        );
        tokio::pin!(acquire);
        tokio::select! {
            biased;
            _ = &mut acquire => panic!("probe acquired the host lock while the stop held it"),
            _ = tokio::task::yield_now() => {}
        }

        rounds.set_enabled(false);
        kill.store(true, Ordering::Release);
        wake.notify_one();
        drop(held);
        assert!(acquire.await.is_none());
    }

    /// Initial state: a stopped monitor still owns admission while its task is
    /// retiring. Operation: an immediate start waits for the retirement signal.
    /// Expected result: it can claim admission after the old task exits instead
    /// of silently leaving health monitoring disabled.
    #[tokio::test]
    async fn immediate_restart_waits_for_retiring_monitor_before_reclaiming_admission() {
        let running = AtomicBool::new(true);
        let kill = AtomicBool::new(true);
        let changed = Notify::new();
        let shutdown = CancellationToken::new();
        let wait = wait_for_health_monitor_retirement(&running, &kill, &changed, &shutdown);
        tokio::pin!(wait);

        tokio::select! {
            biased;
            _ = &mut wait => panic!("restart did not wait for the old monitor to retire"),
            _ = tokio::task::yield_now() => {}
        }

        running.store(false, Ordering::Release);
        changed.notify_waiters();
        changed.notify_one();
        assert!(wait.await);
        assert!(claim_health_check(&running));
    }

    #[tokio::test]
    async fn ipc_transition_status_read_stops_waiting_on_shutdown() {
        let shutdown = CancellationToken::new();
        let read = await_core_status_or_shutdown(
            &shutdown,
            std::future::pending::<anyhow::Result<()>>(),
            std::time::Duration::from_secs(5),
        );
        tokio::pin!(read);

        tokio::select! {
            biased;
            _ = &mut read => panic!("pending core status read unexpectedly completed"),
            _ = tokio::task::yield_now() => {}
        }

        shutdown.cancel();
        assert!(read.await.is_none());
    }

    #[tokio::test]
    async fn ipc_transition_status_read_has_a_finite_timeout() {
        let shutdown = CancellationToken::new();
        let status = await_core_status_or_shutdown(
            &shutdown,
            std::future::pending::<anyhow::Result<()>>(),
            std::time::Duration::from_millis(1),
        )
        .await
        .expect("shutdown was not requested");

        assert!(status.is_err());
        assert!(format!("{:#}", status.unwrap_err()).contains("timed out after 1 ms"));
    }

    #[test]
    fn concurrent_health_monitor_admission_has_one_owner_and_allows_restart() {
        // A second ensure call must not start another monitor while the first
        // thread is being spawned; the old load-then-store admitted both.
        let running = AtomicBool::new(false);
        let accepted = AtomicUsize::new(0);
        let callers = 12;
        let barrier = Arc::new(Barrier::new(callers));
        std::thread::scope(|scope| {
            for _ in 0..callers {
                let barrier = Arc::clone(&barrier);
                let running = &running;
                let accepted = &accepted;
                scope.spawn(move || {
                    barrier.wait();
                    if claim_health_check(running) {
                        accepted.fetch_add(1, Ordering::Relaxed);
                    }
                });
            }
        });
        assert_eq!(accepted.load(Ordering::Relaxed), 1);
        assert!(!claim_health_check(&running));

        // The monitor's exit guard relinquishes ownership for the next run.
        running.store(false, Ordering::Release);
        assert!(claim_health_check(&running));
    }

    fn core_projection(host: ExecutionHost) -> CoreStatusProjection {
        CoreStatusProjection {
            host,
            generation: 5,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("service-ipc-test-core".into()),
                controller: None,
                state: Some(chimera_ipc::api::status::CoreStateDetail::Running {
                    epoch: 2,
                    pid: 42,
                }),
                state_changed_at: 100,
                revision: None,
                source_hash: None,
                healthy: Some(true),
                applied_kind: None,
            }),
        }
    }

    #[test]
    fn service_ipc_transition_rebuilds_only_when_ref_core_host_changes() {
        let local = core_projection(ExecutionHost::Local);
        let service = core_projection(ExecutionHost::Service);
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Connected,
            &local
        ));
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Disconnected,
            &service
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Connected,
            &service
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Disconnected,
            &local
        ));

        // An unreachable Service endpoint retains the committed host. A
        // disconnected daemon must still request reconciliation/recovery.
        let mut degraded_service = service.clone();
        degraded_service.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Service,
            reason: "service IPC lost".into(),
        };
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Disconnected,
            &degraded_service
        ));
        assert!(!should_rebuild_for_ipc_transition(
            IpcState::Connected,
            &degraded_service
        ));

        let mut degraded_local = local.clone();
        degraded_local.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "local endpoint unavailable".into(),
        };
        assert!(should_rebuild_for_ipc_transition(
            IpcState::Connected,
            &degraded_local
        ));

        for mut projection in [local.clone(), service.clone()] {
            projection.connectivity = EndpointConnectivity::HandingOff {
                from: ExecutionHost::Local,
                to: ExecutionHost::Service,
            };
            assert!(!should_rebuild_for_ipc_transition(
                IpcState::Connected,
                &projection
            ));
            assert!(!should_rebuild_for_ipc_transition(
                IpcState::Disconnected,
                &projection
            ));
            projection.connectivity = EndpointConnectivity::ShutDown;
            assert!(!should_rebuild_for_ipc_transition(
                IpcState::Connected,
                &projection
            ));
            assert!(!should_rebuild_for_ipc_transition(
                IpcState::Disconnected,
                &projection
            ));
        }
    }

    #[test]
    fn stale_ipc_transition_is_discarded_after_serialized_service_restart() {
        let local = core_projection(ExecutionHost::Local);
        let service = core_projection(ExecutionHost::Service);
        assert!(!should_rebuild_for_ipc_event(
            IpcState::Disconnected,
            IpcState::Connected,
            &service
        ));
        assert!(should_rebuild_for_ipc_event(
            IpcState::Disconnected,
            IpcState::Disconnected,
            &service
        ));
        assert!(!should_rebuild_for_ipc_event(
            IpcState::Connected,
            IpcState::Disconnected,
            &local
        ));
        // The host generation belongs to the Actor, not the Service health
        // sample. A newer Actor revision with the same committed host must
        // not be confused with an old RunType guess.
        let mut new_generation = service;
        new_generation.generation += 1;
        assert!(should_rebuild_for_ipc_event(
            IpcState::Disconnected,
            IpcState::Disconnected,
            &new_generation
        ));
    }

    #[test]
    fn compatible_owned_daemon_reaches_service_backend() {
        let info = running_status(REQUIRED_SERVICE_MIN);
        let (state, compat) = target_ipc_state(&info, true);
        assert_eq!(state, IpcState::Connected);
        assert_eq!(
            compat,
            ServiceCompat::Compatible {
                server_version: REQUIRED_SERVICE_MIN.to_owned(),
            }
        );
        assert!(
            ServiceBackendObservation {
                status: info.status,
                compat,
                runtime_owned: true,
            }
            .is_ready()
        );
    }

    #[test]
    fn tun_service_readiness_requires_running_compatible_owned_daemon() {
        let compatible = ServiceCompat::Compatible {
            server_version: REQUIRED_SERVICE_MIN.to_owned(),
        };
        assert!(
            ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: compatible.clone(),
                runtime_owned: true,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Stopped,
                compat: compatible.clone(),
                runtime_owned: true,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: compatible,
                runtime_owned: false,
            }
            .is_ready()
        );
        assert!(
            !ServiceBackendObservation {
                status: ServiceStatus::Running,
                compat: ServiceCompat::Incompatible {
                    server_version: "2.0.0".to_owned(),
                    required_major: 1,
                    required_min: chimera_core::service::compat::REQUIRED_SERVICE_MIN.to_owned(),
                },
                runtime_owned: true,
            }
            .is_ready()
        );
    }

    #[test]
    fn incompatible_daemon_never_reaches_service_backend() {
        let info = running_status("2.0.0");
        let (state, compat) = target_ipc_state(&info, true);
        assert_eq!(state, IpcState::Disconnected);
        assert!(matches!(compat, ServiceCompat::Incompatible { .. }));
    }

    #[test]
    fn foreign_runtime_never_reaches_service_backend() {
        let info = running_status(REQUIRED_SERVICE_MIN);
        let (state, compat) = target_ipc_state(&info, false);
        assert_eq!(state, IpcState::Disconnected);
        assert!(compat.allows_service_backend());
    }

    #[test]
    fn ineligible_warning_is_latched() {
        assert_eq!(
            next_ineligible_warning_state(false, true),
            (WarnLevel::Warn, true)
        );
        assert_eq!(
            next_ineligible_warning_state(true, true),
            (WarnLevel::Debug, true)
        );
        assert_eq!(
            next_ineligible_warning_state(true, false),
            (WarnLevel::Silent, false)
        );
    }
}
