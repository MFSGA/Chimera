//! Serialized Service process commands and fail-closed daemon ownership proofs.
//!
//! This is the command-tracking part of ref actor_v2/service_actor.rs. The
//! application workflow still owns host handoff and the ServiceHostStatus watch;
//! those responsibilities must move into the complete ServiceActor separately.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use chimera_ipc::{
    api::status::CoreStateDetail,
    types::{ServiceStatus, StatusInfo},
};
use tokio::{sync::Mutex, task::JoinHandle};

use super::service_process::{
    ServiceCommand, ServiceCommandError, ServiceProcessControl, parse_service_status,
};

const DEFAULT_SERVICE_COMMAND_TIMEOUT: Duration = Duration::from_secs(100);

struct UnsettledOnDrop<'a> {
    flag: &'a AtomicBool,
    armed: bool,
}

impl Drop for UnsettledOnDrop<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.flag.store(true, Ordering::Release);
        }
    }
}

#[derive(Default)]
struct ServiceActorState {
    outstanding: Option<(
        ServiceCommand,
        JoinHandle<std::result::Result<(), ServiceCommandError>>,
    )>,
}

impl ServiceActorState {
    /// An elevated command is not cancelled by abandoning its waiting caller.
    async fn reap_outstanding(&mut self) -> bool {
        match &self.outstanding {
            None => return true,
            Some((_, helper)) if !helper.is_finished() => return false,
            Some(_) => {}
        }
        let (command, helper) = self.outstanding.take().expect("checked above");
        match helper.await {
            Ok(Ok(())) => tracing::info!(%command, "service command finished after its bound"),
            Ok(Err(error)) => {
                tracing::warn!(%command, %error, "service command failed after its bound")
            }
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(error) => {
                    tracing::warn!(%command, %error, "service command task was cancelled")
                }
            },
        }
        true
    }
}

/// Ref-style process command lane, with a single turn and retained OS helpers.
/// It is core-owned; the Tauri host supplies the platform-specific executor.
pub struct ServiceCommandActor {
    process: Arc<dyn ServiceProcessControl>,
    state: Mutex<ServiceActorState>,
    outstanding: AtomicBool,
    unsettled: AtomicBool,
    command_timeout: Duration,
}

impl std::fmt::Debug for ServiceCommandActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceCommandActor")
            .field("outstanding", &self.has_outstanding())
            .field("unsettled", &self.has_unsettled_outcome())
            .finish_non_exhaustive()
    }
}

impl ServiceCommandActor {
    pub fn new(process: Arc<dyn ServiceProcessControl>) -> Self {
        Self::with_timeout(process, DEFAULT_SERVICE_COMMAND_TIMEOUT)
    }

    pub fn with_timeout(
        process: Arc<dyn ServiceProcessControl>,
        command_timeout: Duration,
    ) -> Self {
        Self {
            process,
            state: Mutex::new(ServiceActorState::default()),
            outstanding: AtomicBool::new(false),
            unsettled: AtomicBool::new(false),
            command_timeout,
        }
    }

    /// Fail closed while an elevated command might still be running.
    pub fn has_outstanding(&self) -> bool {
        self.outstanding.load(Ordering::Acquire)
    }

    /// Only a timed-out or abandoned caller is uncertain; a normally
    /// awaited command is not itself evidence of an unknown outcome.
    pub fn has_unsettled_outcome(&self) -> bool {
        self.unsettled.load(Ordering::Acquire)
    }

    async fn reap(&self, state: &mut ServiceActorState) -> bool {
        if !state.reap_outstanding().await {
            return false;
        }
        self.outstanding.store(false, Ordering::Release);
        self.unsettled.store(false, Ordering::Release);
        true
    }

    fn still_running(&self) -> ServiceCommandError {
        ServiceCommandError::StillRunning {
            limit_ms: self.command_timeout.as_millis() as u64,
        }
    }

    async fn tracked(
        &self,
        state: &mut ServiceActorState,
        command: ServiceCommand,
    ) -> std::result::Result<(), ServiceCommandError> {
        if !self.reap(state).await {
            return Err(self.still_running());
        }
        let process = self.process.clone();
        state.outstanding = Some((
            command,
            tokio::spawn(async move {
                match command {
                    ServiceCommand::Install => process.install().await,
                    ServiceCommand::Update => process.update().await,
                    ServiceCommand::Uninstall => process.uninstall().await,
                    ServiceCommand::Start => process.start_daemon().await,
                    ServiceCommand::Stop => process.stop_daemon().await,
                    ServiceCommand::Restart => process.restart_daemon().await,
                }
            }),
        ));
        // The task and its JoinHandle are already stored in shared state.
        // Cancellation of this caller leaves them there for the next turn.
        self.outstanding.store(true, Ordering::Release);
        let mut guard = UnsettledOnDrop {
            flag: &self.unsettled,
            armed: true,
        };
        let result = {
            let (_, helper) = state.outstanding.as_mut().expect("just spawned");
            tokio::time::timeout(self.command_timeout, helper).await
        };
        let Ok(joined) = result else {
            return Err(self.still_running());
        };
        state.outstanding.take();
        self.outstanding.store(false, Ordering::Release);
        self.unsettled.store(false, Ordering::Release);
        guard.armed = false;
        match joined {
            Ok(result) => result,
            Err(error) => match error.try_into_panic() {
                Ok(panic) => std::panic::resume_unwind(panic),
                Err(_) => Err(ServiceCommandError::TaskCancelled),
            },
        }
    }

    /// A single serialized OS mutation. Timeouts return StillRunning and
    /// retain the helper, instead of silently cancelling its JoinHandle.
    pub async fn command(
        &self,
        command: ServiceCommand,
    ) -> std::result::Result<(), ServiceCommandError> {
        let mut state = self.state.lock().await;
        self.tracked(&mut state, command).await
    }

    async fn probe_inner(&self) -> std::result::Result<StatusInfo<'static>, ServiceCommandError> {
        match tokio::time::timeout(self.command_timeout, self.process.probe()).await {
            Ok(Ok(raw)) => parse_service_status(&raw),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(ServiceCommandError::TimedOut {
                limit_ms: self.command_timeout.as_millis() as u64,
            }),
        }
    }

    /// Serialized status read; errors never become a synthetic stopped state.
    pub async fn probe(&self) -> std::result::Result<StatusInfo<'static>, ServiceCommandError> {
        let _turn = self.state.lock().await;
        self.probe_inner().await
    }

    /// As in ref CommandSettled: false while a timed-out helper still runs,
    /// and one fresh observation when a finished helper is reaped.
    pub async fn command_settled(&self) -> bool {
        let mut state = self.state.lock().await;
        let reaping = state.outstanding.is_some();
        if !self.reap(&mut state).await {
            return false;
        }
        if reaping {
            let _ = self.probe_inner().await;
        }
        true
    }

    async fn daemon_not_running_inner(&self, state: &mut ServiceActorState) -> bool {
        if !self.reap(state).await {
            return false;
        }
        matches!(
            self.probe_inner().await,
            Ok(StatusInfo {
                status: ServiceStatus::Stopped | ServiceStatus::NotInstalled,
                ..
            })
        )
    }

    /// As in ref DaemonNotRunning: require a settled OS helper and a fresh,
    /// authoritative stopped/not-installed observation. Unknown is not proof.
    pub async fn daemon_not_running(&self) -> bool {
        let mut state = self.state.lock().await;
        self.daemon_not_running_inner(&mut state).await
    }

    /// Ref Uninstall's double guard: probe core detail, stop a daemon which
    /// proves it has no live core, re-probe its OS stop, then uninstall.
    /// All steps occupy one serialized turn, and each elevated step is tracked.
    pub async fn uninstall(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        if !self.reap(&mut state).await {
            bail!("a previous elevated service command is still running");
        }
        let status = self
            .probe_inner()
            .await
            .context("could not prove Service core state before uninstall; refusing uninstall")?;
        if uninstall_requires_stop(&status)? {
            self.tracked(&mut state, ServiceCommand::Stop)
                .await
                .context("stopping Service before uninstall failed")?;
            if !self.daemon_not_running_inner(&mut state).await {
                bail!("Service did not prove it stopped; refusing uninstall");
            }
        }
        self.tracked(&mut state, ServiceCommand::Uninstall)
            .await
            .context("uninstalling Service failed")?;
        // A final probe is advisory here; the caller must separately confirm
        // the resulting daemon state before switching the runtime host.
        let _ = self.probe_inner().await;
        Ok(())
    }
}

/// A generation token for the host Service health loop, following upstream
/// ServiceActor's SetHealthCheck/HealthCheck { round } contract. The complete
/// ServiceActor mailbox is still pending: this prevents a returned stale
/// probe from publishing a new observation after stop or monitor replacement.
#[derive(Debug, Default)]
pub struct ServiceHealthRounds {
    revision: AtomicU64,
}

impl ServiceHealthRounds {
    pub const fn new() -> Self {
        Self {
            revision: AtomicU64::new(0),
        }
    }

    /// Idempotently enable/disable monitoring. Odd generations are enabled;
    /// each transition invalidates all tokens from earlier generations.
    pub fn set_enabled(&self, enabled: bool) -> Option<u64> {
        loop {
            let current = self.revision.load(Ordering::Acquire);
            let was_enabled = current & 1 == 1;
            if was_enabled == enabled {
                return enabled.then_some(current);
            }
            let next = current
                .checked_add(1)
                .expect("Service health generation must not wrap");
            if self
                .revision
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return enabled.then_some(next);
            }
        }
    }

    /// A delayed probe or queued tick can only publish in its original round.
    pub fn is_current(&self, round: u64) -> bool {
        round & 1 == 1 && self.revision.load(Ordering::Acquire) == round
    }
}

/// The coarse CoreState cannot prove a core is stopped: Starting/Restarting
/// can project as Stopped. Only the v2 terminal detail is an ownership proof.
fn uninstall_requires_stop(status: &StatusInfo<'_>) -> Result<bool> {
    match status.status {
        ServiceStatus::Stopped | ServiceStatus::NotInstalled => Ok(false),
        ServiceStatus::Running => match status.server.as_ref() {
            None => bail!("running Service omitted server status; refusing uninstall"),
            Some(server) => match server.core_infos.detail.as_ref() {
                Some(CoreStateDetail::Stopped { .. }) => Ok(true),
                Some(_) => bail!(
                    "Service still owns a running or transitioning core; hand off to Local first"
                ),
                None => bail!("running Service omitted v2 core detail; refusing uninstall"),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_ipc::api::status::{CoreInfos, CoreState, RuntimeInfos, StatusResBody};
    use std::{
        borrow::Cow,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    struct FakeProcess {
        status: std::sync::Mutex<ServiceStatus>,
        core_detail: std::sync::Mutex<Option<CoreStateDetail>>,
        calls: std::sync::Mutex<Vec<ServiceCommand>>,
        pause_start: AtomicBool,
        keep_running_after_stop: AtomicBool,
        invalid_probe: AtomicBool,
        starts: AtomicUsize,
        started: Notify,
        release: Notify,
    }

    impl FakeProcess {
        fn new(status: ServiceStatus, detail: Option<CoreStateDetail>) -> Self {
            Self {
                status: std::sync::Mutex::new(status),
                core_detail: std::sync::Mutex::new(detail),
                calls: std::sync::Mutex::new(Vec::new()),
                pause_start: AtomicBool::new(false),
                keep_running_after_stop: AtomicBool::new(false),
                invalid_probe: AtomicBool::new(false),
                starts: AtomicUsize::new(0),
                started: Notify::new(),
                release: Notify::new(),
            }
        }

        fn calls(&self) -> Vec<ServiceCommand> {
            self.calls.lock().unwrap().clone()
        }

        fn status_json(&self) -> String {
            let info = StatusInfo {
                name: Cow::Borrowed("chimera-service"),
                version: Cow::Borrowed("1.11.1"),
                status: *self.status.lock().unwrap(),
                server: Some(StatusResBody {
                    version: Cow::Borrowed("1.11.1"),
                    core_infos: CoreInfos {
                        instance_id: None,
                        r#type: None,
                        state: CoreState::Stopped(None),
                        state_changed_at: 0,
                        config_path: None,
                        controller: None,
                        health: None,
                        revision: None,
                        detail: self.core_detail.lock().unwrap().clone(),
                    },
                    runtime_infos: RuntimeInfos {
                        service_data_dir: Cow::Owned(PathBuf::new()),
                        service_config_dir: Cow::Owned(PathBuf::new()),
                        nyanpasu_config_dir: Cow::Owned(PathBuf::new()),
                        nyanpasu_data_dir: Cow::Owned(PathBuf::new()),
                    },
                }),
            };
            serde_json::to_string(&info).unwrap()
        }

        fn record(&self, command: ServiceCommand) {
            self.calls.lock().unwrap().push(command);
        }
    }

    #[async_trait::async_trait]
    impl ServiceProcessControl for FakeProcess {
        async fn probe(&self) -> std::result::Result<String, ServiceCommandError> {
            Ok(if self.invalid_probe.load(Ordering::Acquire) {
                "invalid JSON".into()
            } else {
                self.status_json()
            })
        }

        async fn install(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Install);
            Ok(())
        }

        async fn uninstall(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Uninstall);
            *self.status.lock().unwrap() = ServiceStatus::NotInstalled;
            Ok(())
        }

        async fn start_daemon(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Start);
            self.starts.fetch_add(1, Ordering::AcqRel);
            if self.pause_start.load(Ordering::Acquire) {
                self.started.notify_one();
                self.release.notified().await;
            }
            *self.status.lock().unwrap() = ServiceStatus::Running;
            Ok(())
        }

        async fn stop_daemon(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Stop);
            if !self.keep_running_after_stop.load(Ordering::Acquire) {
                *self.status.lock().unwrap() = ServiceStatus::Stopped;
            }
            Ok(())
        }

        async fn update(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Update);
            Ok(())
        }

        async fn restart_daemon(&self) -> std::result::Result<(), ServiceCommandError> {
            self.record(ServiceCommand::Restart);
            Ok(())
        }
    }

    #[tokio::test]
    async fn timed_out_helper_refuses_overlapping_mutation_and_false_stop_proof() {
        let fake = Arc::new(FakeProcess::new(ServiceStatus::Stopped, None));
        fake.pause_start.store(true, Ordering::Release);
        let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_millis(30));

        assert!(matches!(
            actor.command(ServiceCommand::Start).await,
            Err(ServiceCommandError::StillRunning { .. })
        ));
        assert!(actor.has_outstanding());
        assert!(!actor.command_settled().await);
        assert!(!actor.daemon_not_running().await);
        assert!(matches!(
            actor.command(ServiceCommand::Stop).await,
            Err(ServiceCommandError::StillRunning { .. })
        ));
        assert_eq!(fake.calls(), [ServiceCommand::Start]);
        fake.release.notify_one();

        tokio::time::timeout(Duration::from_secs(2), async {
            while !actor.command_settled().await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!actor.has_outstanding());
        assert!(
            !actor.daemon_not_running().await,
            "completed Start reports Running"
        );
        actor.command(ServiceCommand::Stop).await.unwrap();
        assert!(actor.daemon_not_running().await);
    }

    #[tokio::test]
    async fn caller_cancellation_retains_elevated_helper_until_reaped() {
        let fake = Arc::new(FakeProcess::new(ServiceStatus::Stopped, None));
        fake.pause_start.store(true, Ordering::Release);
        let actor = Arc::new(ServiceCommandActor::with_timeout(
            fake.clone(),
            Duration::from_millis(200),
        ));
        let started = fake.started.notified();
        let caller = {
            let actor = actor.clone();
            tokio::spawn(async move { actor.command(ServiceCommand::Start).await })
        };
        started.await;
        caller.abort();
        let _ = caller.await;

        assert!(actor.has_outstanding());
        assert!(!actor.command_settled().await);
        assert_eq!(fake.starts.load(Ordering::Acquire), 1);
        fake.release.notify_one();
        tokio::time::timeout(Duration::from_secs(2), async {
            while !actor.command_settled().await {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!actor.has_outstanding());
        assert_eq!(fake.calls(), [ServiceCommand::Start]);
    }

    #[tokio::test]
    async fn uninstall_rejects_a_running_core_and_missing_v2_proofs() {
        for detail in [
            None,
            Some(CoreStateDetail::Starting { epoch: 3 }),
            Some(CoreStateDetail::Running { epoch: 3, pid: 456 }),
        ] {
            let fake = Arc::new(FakeProcess::new(ServiceStatus::Running, detail));
            let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_secs(1));
            assert!(actor.uninstall().await.is_err());
            assert!(fake.calls().is_empty());
        }
    }

    #[tokio::test]
    async fn uninstall_stops_a_proven_idle_daemon_then_verifies_before_removal() {
        let fake = Arc::new(FakeProcess::new(
            ServiceStatus::Running,
            Some(CoreStateDetail::Stopped { reason: None }),
        ));
        let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_secs(1));
        actor.uninstall().await.unwrap();
        assert_eq!(
            fake.calls(),
            [ServiceCommand::Stop, ServiceCommand::Uninstall]
        );
        assert!(actor.daemon_not_running().await);
    }

    #[tokio::test]
    async fn no_uninstall_after_a_stop_command_that_did_not_stop_the_daemon() {
        let fake = Arc::new(FakeProcess::new(
            ServiceStatus::Running,
            Some(CoreStateDetail::Stopped { reason: None }),
        ));
        fake.keep_running_after_stop.store(true, Ordering::Release);
        let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_secs(1));
        assert!(actor.uninstall().await.is_err());
        assert_eq!(fake.calls(), [ServiceCommand::Stop]);
    }

    #[tokio::test]
    async fn failed_or_unparseable_probe_never_synthesizes_an_uninstall_proof() {
        let fake = Arc::new(FakeProcess::new(ServiceStatus::Stopped, None));
        fake.invalid_probe.store(true, Ordering::Release);
        let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_secs(1));
        assert!(!actor.daemon_not_running().await);
        assert!(actor.uninstall().await.is_err());
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn health_round_invalidates_stale_ticks_after_disable_and_restart() {
        let rounds = ServiceHealthRounds::new();
        assert!(!rounds.is_current(0));
        let first = rounds.set_enabled(true).unwrap();
        assert_eq!(
            rounds.set_enabled(true),
            Some(first),
            "enabling twice is idempotent"
        );
        assert!(rounds.is_current(first));

        assert_eq!(rounds.set_enabled(false), None);
        assert!(
            !rounds.is_current(first),
            "late probes from a stopped monitor must be ignored"
        );
        assert_eq!(rounds.set_enabled(false), None);
        let second = rounds.set_enabled(true).unwrap();
        assert_ne!(first, second);
        assert!(rounds.is_current(second));
        assert!(
            !rounds.is_current(first),
            "previous monitor must not resurrect"
        );
    }

    #[test]
    fn disabled_round_never_accepts_an_in_flight_result() {
        let rounds = ServiceHealthRounds::new();
        let round = rounds.set_enabled(true).unwrap();
        // Simulate a probe that began before the daemon was stopped.
        rounds.set_enabled(false);
        assert!(!rounds.is_current(round));
        assert_eq!(rounds.set_enabled(true), Some(round + 2));
        assert!(!rounds.is_current(round));
    }

    #[tokio::test]
    async fn already_stopped_daemon_is_uninstalled_without_an_invalid_stop_call() {
        let fake = Arc::new(FakeProcess::new(ServiceStatus::Stopped, None));
        let actor = ServiceCommandActor::with_timeout(fake.clone(), Duration::from_secs(1));
        actor.uninstall().await.unwrap();
        assert_eq!(fake.calls(), [ServiceCommand::Uninstall]);
    }
}
