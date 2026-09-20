//! Service daemon command owner.
//!
//! Staged counterpart of ref `actor_v2::service_actor`. Elevated daemon
//! commands are serialized by one mailbox, so caller cancellation never drops
//! an admitted OS command. The higher lifecycle workflow still owns cross-host
//! convergence and `HOST_TRANSITION_LOCK` during this migration.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

const SERVICE_CALL_TIMEOUT: Duration = Duration::from_secs(110);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ServicePhase {
    Probing,
    NotInstalled,
    DaemonStopped,
    Installing,
    StartingDaemon,
    Ready,
    Incompatible,
    Restarting,
    Exhausted,
    Uninstalling,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct ServiceHostStatus {
    pub(crate) name: std::borrow::Cow<'static, str>,
    pub(crate) version: std::borrow::Cow<'static, str>,
    pub(crate) status: chimera_ipc::types::ServiceStatus,
    pub(crate) server: Option<chimera_ipc::api::status::StatusResBody<'static>>,
    pub(crate) phase: ServicePhase,
    pub(crate) compat: crate::core::service::compat::ServiceCompat,
    pub(crate) runtime_owned: bool,
    pub(crate) restart_attempts: u8,
}

impl ServiceHostStatus {
    pub(crate) fn probing() -> Self {
        Self {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed(""),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
            phase: ServicePhase::Probing,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: 0,
        }
    }

    pub(crate) fn from_probe(info: &chimera_ipc::types::StatusInfo<'static>) -> Self {
        use chimera_ipc::types::ServiceStatus;

        let compat = crate::core::service::compat::ServiceCompat::classify(&info);
        let runtime_owned = crate::core::service::is_service_runtime_owned(&info);
        let phase = match info.status {
            ServiceStatus::Running if compat.allows_service_backend() && runtime_owned => {
                ServicePhase::Ready
            }
            ServiceStatus::Running => ServicePhase::Incompatible,
            ServiceStatus::Stopped => ServicePhase::DaemonStopped,
            ServiceStatus::NotInstalled => ServicePhase::NotInstalled,
        };
        Self {
            name: info.name.clone(),
            version: info.version.clone(),
            status: info.status,
            server: info.server.clone(),
            phase,
            compat,
            runtime_owned,
            restart_attempts: 0,
        }
    }

    pub(crate) fn probe_failed(previous: &Self) -> Self {
        Self {
            name: previous.name.clone(),
            version: previous.version.clone(),
            status: previous.status,
            server: previous.server.clone(),
            phase: ServicePhase::Unknown,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: previous.restart_attempts,
        }
    }
}

#[async_trait]
pub(crate) trait ServiceHostAdapter: Send + Sync {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>>;
    async fn install(&self) -> anyhow::Result<()>;
    async fn uninstall(&self) -> anyhow::Result<()>;
    async fn update(&self) -> anyhow::Result<()>;
    async fn start(&self) -> anyhow::Result<()>;
    async fn restart(&self) -> anyhow::Result<()>;
    async fn stop(&self) -> anyhow::Result<()>;
}

pub(crate) struct OsServiceHostAdapter;

#[async_trait]
impl ServiceHostAdapter for OsServiceHostAdapter {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        crate::core::service::control::status().await
    }

    async fn install(&self) -> anyhow::Result<()> {
        crate::core::service::control::install_service_daemon().await
    }

    async fn uninstall(&self) -> anyhow::Result<()> {
        crate::core::service::control::uninstall_service().await
    }

    async fn update(&self) -> anyhow::Result<()> {
        crate::core::service::control::update_service().await
    }

    async fn start(&self) -> anyhow::Result<()> {
        crate::core::service::control::start_service_daemon().await
    }

    async fn restart(&self) -> anyhow::Result<()> {
        crate::core::service::control::restart_service_daemon().await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        crate::core::service::control::stop_service().await
    }
}

enum Message {
    Probe {
        reply: RpcReplyPort<anyhow::Result<chimera_ipc::types::StatusInfo<'static>>>,
    },
    Install {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Uninstall {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Update {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Start {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Restart {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Stop {
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    Observe {
        info: chimera_ipc::types::StatusInfo<'static>,
    },
    ProbeFailed,
    EndpointDown,
}

struct ServiceActor;

struct ServiceActorArgs {
    adapter: Arc<dyn ServiceHostAdapter>,
    status_tx: tokio::sync::watch::Sender<ServiceHostStatus>,
    restart_attempts: Arc<AtomicU8>,
    restart_exhausted: Arc<AtomicBool>,
    restart_budget: u8,
}

struct ServiceActorState {
    adapter: Arc<dyn ServiceHostAdapter>,
    status_tx: tokio::sync::watch::Sender<ServiceHostStatus>,
    restart_attempts: Arc<AtomicU8>,
    restart_exhausted: Arc<AtomicBool>,
    restart_budget: u8,
}

impl ServiceActorState {
    fn rearm(&self) {
        self.restart_attempts.store(0, Ordering::Release);
        self.restart_exhausted.store(false, Ordering::Release);
    }

    fn publish_phase(&self, phase: ServicePhase) {
        let current = self.status_tx.borrow().clone();
        self.status_tx.send_replace(ServiceHostStatus {
            name: current.name,
            version: current.version,
            status: current.status,
            server: current.server,
            phase,
            compat: crate::core::service::compat::ServiceCompat::Unknown,
            runtime_owned: current.runtime_owned,
            restart_attempts: self.restart_attempts.load(Ordering::Acquire),
        });
    }

    fn publish_probe(&self, info: &chimera_ipc::types::StatusInfo<'static>) -> ServiceHostStatus {
        let mut status = ServiceHostStatus::from_probe(info);
        status.restart_attempts = self.restart_attempts.load(Ordering::Acquire);
        if self.restart_exhausted.load(Ordering::Acquire) {
            status.phase = ServicePhase::Exhausted;
        }
        self.status_tx.send_replace(status.clone());
        status
    }

    fn publish_probe_failure(&self) {
        let previous = self.status_tx.borrow().clone();
        let mut status = ServiceHostStatus::probe_failed(&previous);
        status.restart_attempts = self.restart_attempts.load(Ordering::Acquire);
        if self.restart_exhausted.load(Ordering::Acquire) {
            status.phase = ServicePhase::Exhausted;
        }
        self.status_tx.send_replace(status);
    }

    async fn probe_and_publish(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        match self.adapter.probe().await {
            Ok(info) => {
                self.publish_probe(&info);
                Ok(info)
            }
            Err(error) => {
                self.publish_probe_failure();
                Err(error)
            }
        }
    }

    async fn endpoint_down(&self) {
        use chimera_ipc::types::ServiceStatus;

        if self.restart_exhausted.load(Ordering::Acquire) {
            return;
        }
        let info = match self.probe_and_publish().await {
            Ok(info) => info,
            Err(error) => {
                tracing::warn!(%error, "service endpoint-down probe failed; not restarting blindly");
                return;
            }
        };
        if info.status != ServiceStatus::Stopped {
            return;
        }

        let attempts = self.restart_attempts.load(Ordering::Acquire);
        if attempts >= self.restart_budget {
            self.restart_exhausted.store(true, Ordering::Release);
            self.publish_phase(ServicePhase::Exhausted);
            return;
        }
        self.restart_attempts
            .store(attempts.saturating_add(1), Ordering::Release);
        self.publish_phase(ServicePhase::Restarting);
        if let Err(error) = self.adapter.start().await {
            tracing::warn!(%error, "service auto-restart failed");
        }
        let _ = self.probe_and_publish().await;
    }
}

impl Actor for ServiceActor {
    type Msg = Message;
    type State = ServiceActorState;
    type Arguments = ServiceActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        let state = ServiceActorState {
            adapter: args.adapter,
            status_tx: args.status_tx,
            restart_attempts: args.restart_attempts,
            restart_exhausted: args.restart_exhausted,
            restart_budget: args.restart_budget,
        };
        let _ = state.probe_and_publish().await;
        Ok(state)
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Probe { reply } => {
                let _ = reply.send(state.probe_and_publish().await);
            }
            Message::Install { reply } => {
                state.rearm();
                state.publish_phase(ServicePhase::Installing);
                let result = state.adapter.install().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Uninstall { reply } => {
                state.publish_phase(ServicePhase::Uninstalling);
                let result = state.adapter.uninstall().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Update { reply } => {
                state.rearm();
                let result = state.adapter.update().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Start { reply } => {
                state.rearm();
                state.publish_phase(ServicePhase::StartingDaemon);
                let result = state.adapter.start().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Restart { reply } => {
                state.rearm();
                state.publish_phase(ServicePhase::Restarting);
                let result = state.adapter.restart().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Stop { reply } => {
                let result = state.adapter.stop().await;
                let _ = state.probe_and_publish().await;
                let _ = reply.send(result);
            }
            Message::Observe { info } => {
                state.publish_probe(&info);
            }
            Message::ProbeFailed => state.publish_probe_failure(),
            Message::EndpointDown => state.endpoint_down().await,
        }
        Ok(())
    }
}

struct MutationReplyGuard {
    outcome_uncertain: Arc<AtomicBool>,
    armed: bool,
}

impl MutationReplyGuard {
    fn new(outcome_uncertain: Arc<AtomicBool>) -> Self {
        Self {
            outcome_uncertain,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for MutationReplyGuard {
    fn drop(&mut self) {
        if self.armed {
            self.outcome_uncertain.store(true, Ordering::Release);
        }
    }
}

#[derive(Clone)]
pub(crate) struct ServiceClient {
    actor: ActorRef<Message>,
    status_rx: tokio::sync::watch::Receiver<ServiceHostStatus>,
    outcome_uncertain: Arc<AtomicBool>,
}

impl ServiceClient {
    pub(crate) async fn spawn(
        adapter: Arc<dyn ServiceHostAdapter>,
        outcome_uncertain: Arc<AtomicBool>,
        restart_attempts: Arc<AtomicU8>,
        restart_exhausted: Arc<AtomicBool>,
        restart_budget: u8,
    ) -> anyhow::Result<Self> {
        let (status_tx, status_rx) = tokio::sync::watch::channel(ServiceHostStatus::probing());
        let (actor, _handle) = Actor::spawn(
            None,
            ServiceActor,
            ServiceActorArgs {
                adapter,
                status_tx,
                restart_attempts,
                restart_exhausted,
                restart_budget,
            },
        )
        .await
        .map_err(|error| anyhow::anyhow!("failed to spawn service actor: {error}"))?;
        Ok(Self {
            actor,
            status_rx,
            outcome_uncertain,
        })
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<ServiceHostStatus> {
        self.status_rx.clone()
    }

    pub(crate) fn observe(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        let _ = self.actor.cast(Message::Observe { info });
    }

    pub(crate) fn observe_probe_failure(&self) {
        let _ = self.actor.cast(Message::ProbeFailed);
    }

    pub(crate) async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        match self
            .actor
            .call(
                |reply| Message::Probe { reply },
                Some(Duration::from_secs(10)),
            )
            .await?
        {
            ractor::rpc::CallResult::Success(result) => result,
            ractor::rpc::CallResult::Timeout => anyhow::bail!("service probe actor call timed out"),
            ractor::rpc::CallResult::SenderError => {
                anyhow::bail!("service probe actor reply dropped")
            }
        }
    }

    async fn mutate(
        &self,
        message: impl FnOnce(RpcReplyPort<anyhow::Result<()>>) -> Message,
    ) -> anyhow::Result<()> {
        if self.outcome_uncertain.load(Ordering::Acquire) {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }

        let mut guard = MutationReplyGuard::new(self.outcome_uncertain.clone());
        let result = match self.actor.call(message, Some(SERVICE_CALL_TIMEOUT)).await {
            Ok(ractor::rpc::CallResult::Success(result)) => result,
            Ok(ractor::rpc::CallResult::Timeout) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                anyhow::bail!(
                    "service actor did not answer within its bound; the admitted command may still be running"
                )
            }
            Ok(ractor::rpc::CallResult::SenderError) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                anyhow::bail!("service actor reply dropped; command outcome is uncertain")
            }
            Err(error) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                Err(error.into())
            }
        };
        guard.disarm();
        result
    }

    pub(crate) async fn install(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Install { reply }).await
    }

    pub(crate) async fn uninstall(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Uninstall { reply }).await
    }

    pub(crate) async fn update(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Update { reply }).await
    }

    pub(crate) async fn start(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Start { reply }).await
    }

    pub(crate) async fn restart(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Restart { reply }).await
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        self.mutate(|reply| Message::Stop { reply }).await
    }

    pub(crate) fn report_endpoint_down(&self) {
        let _ = self.actor.cast(Message::EndpointDown);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    struct RecordingAdapter {
        active: AtomicUsize,
        max_active: AtomicUsize,
        release: tokio::sync::Notify,
    }

    impl RecordingAdapter {
        fn new() -> Self {
            Self {
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                release: tokio::sync::Notify::new(),
            }
        }
    }

    #[async_trait]
    impl ServiceHostAdapter for RecordingAdapter {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("test"),
                status: chimera_ipc::types::ServiceStatus::Stopped,
                server: None,
            })
        }

        async fn install(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn uninstall(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn start(&self) -> anyhow::Result<()> {
            let active = self.active.fetch_add(1, AtomicOrdering::AcqRel) + 1;
            self.max_active.fetch_max(active, AtomicOrdering::AcqRel);
            self.release.notified().await;
            self.active.fetch_sub(1, AtomicOrdering::AcqRel);
            Ok(())
        }

        async fn restart(&self) -> anyhow::Result<()> {
            self.start().await
        }

        async fn stop(&self) -> anyhow::Result<()> {
            self.start().await
        }
    }

    #[tokio::test]
    async fn mailbox_serializes_service_mutations_after_waiter_cancellation() {
        let adapter = Arc::new(RecordingAdapter::new());
        let client = ServiceClient::spawn(
            adapter.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU8::new(0)),
            Arc::new(AtomicBool::new(false)),
            3,
        )
        .await
        .unwrap();

        let first = {
            let actor = client.actor.clone();
            tokio::spawn(async move { actor.call(|reply| Message::Start { reply }, None).await })
        };
        tokio::task::yield_now().await;
        first.abort();
        let _ = first.await;
        assert_eq!(adapter.active.load(AtomicOrdering::Acquire), 1);

        let second = {
            let actor = client.actor.clone();
            tokio::spawn(async move { actor.call(|reply| Message::Stop { reply }, None).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(adapter.max_active.load(AtomicOrdering::Acquire), 1);

        adapter.release.notify_one();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if adapter.active.load(AtomicOrdering::Acquire) == 1 {
                    tokio::task::yield_now().await;
                    if adapter.max_active.load(AtomicOrdering::Acquire) == 1 {
                        break;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        adapter.release.notify_one();
        let _ = second.await;
        assert_eq!(adapter.max_active.load(AtomicOrdering::Acquire), 1);
    }

    #[tokio::test]
    async fn cancelling_service_client_wait_latches_uncertain_without_cancelling_actor_command() {
        let adapter = Arc::new(RecordingAdapter::new());
        let uncertain = Arc::new(AtomicBool::new(false));
        let client = ServiceClient::spawn(
            adapter.clone(),
            uncertain.clone(),
            Arc::new(AtomicU8::new(0)),
            Arc::new(AtomicBool::new(false)),
            3,
        )
        .await
        .unwrap();

        let waiter = {
            let client = client.clone();
            tokio::spawn(async move { client.start().await })
        };
        tokio::task::yield_now().await;
        waiter.abort();
        let _ = waiter.await;

        assert!(uncertain.load(Ordering::Acquire));
        assert_eq!(adapter.active.load(AtomicOrdering::Acquire), 1);
        adapter.release.notify_one();
    }

    struct BudgetAdapter {
        status: parking_lot::Mutex<chimera_ipc::types::ServiceStatus>,
        starts: AtomicUsize,
    }

    #[async_trait]
    impl ServiceHostAdapter for BudgetAdapter {
        async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
            Ok(chimera_ipc::types::StatusInfo {
                name: std::borrow::Cow::Borrowed("chimera-service"),
                version: std::borrow::Cow::Borrowed("test"),
                status: *self.status.lock(),
                server: None,
            })
        }

        async fn install(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn uninstall(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn start(&self) -> anyhow::Result<()> {
            self.starts.fetch_add(1, AtomicOrdering::AcqRel);
            Ok(())
        }

        async fn restart(&self) -> anyhow::Result<()> {
            self.start().await
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn endpoint_down_uses_bounded_restart_budget_and_explicit_commands_rearm_it() {
        use chimera_ipc::types::ServiceStatus;

        let adapter = Arc::new(BudgetAdapter {
            status: parking_lot::Mutex::new(ServiceStatus::Stopped),
            starts: AtomicUsize::new(0),
        });
        let attempts = Arc::new(AtomicU8::new(0));
        let exhausted = Arc::new(AtomicBool::new(false));
        let client = ServiceClient::spawn(
            adapter.clone(),
            Arc::new(AtomicBool::new(false)),
            attempts.clone(),
            exhausted.clone(),
            3,
        )
        .await
        .unwrap();

        for expected in 1..=3 {
            client.report_endpoint_down();
            tokio::time::timeout(Duration::from_secs(1), async {
                while adapter.starts.load(AtomicOrdering::Acquire) < expected {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
        }
        client.report_endpoint_down();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !exhausted.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        assert_eq!(adapter.starts.load(AtomicOrdering::Acquire), 3);
        assert_eq!(attempts.load(Ordering::Acquire), 3);
        assert!(exhausted.load(Ordering::Acquire));

        client.start().await.unwrap();
        assert_eq!(attempts.load(Ordering::Acquire), 0);
        assert!(!exhausted.load(Ordering::Acquire));

        *adapter.status.lock() = ServiceStatus::Running;
        client.report_endpoint_down();
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(attempts.load(Ordering::Acquire), 0);
    }
}
