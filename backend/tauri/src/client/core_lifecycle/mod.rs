//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Chimera currently routes execution to the legacy CoreManager adapter while
//! lifecycle mutation admission is serialized through a client-owned mailbox.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{
    ChimeraClient, application::ApplicationClient, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};
use workflow::{Command, CoreLifecycleWorkflow};

const CALL_WAIT: Duration = Duration::from_secs(180);
const DIRTY_WINDOW: Duration = Duration::from_millis(500);
const COMPLETED_LIMIT: usize = 32;

type OperationId = u64;

#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub(crate) struct CoreLifecycleStatus {
    pub(crate) active: Option<OperationId>,
    pub(crate) queued: Vec<OperationId>,
    pub(crate) completed: Vec<CoreLifecycleOperationResult>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub(crate) struct CoreLifecycleOperationResult {
    pub(crate) id: OperationId,
    pub(crate) error: Option<String>,
}

#[allow(unused_imports)]
pub(crate) use adapters::{FsBinaryInstaller, LegacyCoreBridge, LegacyRunningConfigBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    BinaryInstallProgress, CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot,
    PreparedCoreBinary, RunningConfigPort, RuntimeTransformDiagnostics,
};

enum Message {
    Execute {
        id: OperationId,
        command: Command,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    RecoverCore,
    RuntimeDirty,
    DirtyTick,
}

struct CoreLifecycleActor;

struct CoreLifecycleActorState {
    workflow: CoreLifecycleWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    dirty: bool,
    dirty_tick_scheduled: bool,
}

struct CoreLifecycleActorArgs {
    workflow: CoreLifecycleWorkflow,
    next_id: Arc<AtomicU64>,
    status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
}

impl CoreLifecycleActorState {
    fn allocate_operation_id(&self) -> OperationId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn execute_operation(&mut self, id: OperationId, command: Command) -> anyhow::Result<()> {
        {
            let mut status = self.status.lock();
            status.queued.retain(|queued| *queued != id);
            status.active = Some(id);
        }
        let progress = match &command {
            Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
            _ => None,
        };
        let result = self.workflow.execute(command).await;
        if let Some(progress) = progress {
            let error = result.as_ref().err().map(ToString::to_string);
            progress.finished(error.as_deref());
        }
        {
            let mut status = self.status.lock();
            status.active = None;
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id,
                error: result.as_ref().err().map(ToString::to_string),
            });
        }
        result
    }
}

impl Actor for CoreLifecycleActor {
    type Msg = Message;
    type State = CoreLifecycleActorState;
    type Arguments = CoreLifecycleActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(CoreLifecycleActorState {
            workflow: args.workflow,
            next_id: args.next_id,
            status: args.status,
            dirty: false,
            dirty_tick_scheduled: false,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Execute { id, command, reply } => {
                let _ = reply.send(state.execute_operation(id, command).await);
            }
            Message::RecoverCore => {
                tracing::info!("trying to recover core through lifecycle actor");
                let id = state.allocate_operation_id();
                if let Err(error) = state.execute_operation(id, Command::RecoverCore).await {
                    tracing::error!(%error, "failed to recover core; scheduling retry");
                    let actor = myself.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let _ = actor.cast(Message::RecoverCore);
                    });
                }
            }
            Message::RuntimeDirty => {
                state.dirty = true;
                if !state.dirty_tick_scheduled {
                    state.dirty_tick_scheduled = true;
                    let actor = myself.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(DIRTY_WINDOW).await;
                        let _ = actor.cast(Message::DirtyTick);
                    });
                }
            }
            Message::DirtyTick => {
                state.dirty_tick_scheduled = false;
                if state.dirty {
                    state.dirty = false;
                    let id = state.allocate_operation_id();
                    if let Err(error) = state.execute_operation(id, Command::Reconcile).await {
                        tracing::warn!(%error, id, "coalesced background runtime rebuild failed");
                    }
                }
            }
        }
        Ok(())
    }
}

enum CoreLifecycleClientInner {
    Actor {
        actor_ref: ActorRef<Message>,
        next_id: Arc<AtomicU64>,
        status: Arc<parking_lot::Mutex<CoreLifecycleStatus>>,
    },
    #[cfg(test)]
    Direct {
        workflow: tokio::sync::Mutex<CoreLifecycleWorkflow>,
    },
}

#[derive(Clone)]
pub(super) struct CoreLifecycleClient(Arc<CoreLifecycleClientInner>);

impl CoreLifecycleClient {
    pub(super) async fn spawn(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_installer(
            core,
            application,
            clash,
            runtime_paths,
            Arc::new(FsBinaryInstaller),
        )
        .await
    }

    async fn spawn_with_installer(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
    ) -> anyhow::Result<Self> {
        let recovery_notify = core.recovery_notify();
        let workflow =
            CoreLifecycleWorkflow::new(application, clash, core, installer, runtime_paths);
        let status = Arc::new(parking_lot::Mutex::new(CoreLifecycleStatus::default()));
        let next_id = Arc::new(AtomicU64::new(1));
        let (actor_ref, _handle) = Actor::spawn(
            None,
            CoreLifecycleActor,
            CoreLifecycleActorArgs {
                workflow,
                next_id: next_id.clone(),
                status: status.clone(),
            },
        )
        .await?;
        if let Some(recovery_notify) = recovery_notify {
            let recovery_actor = actor_ref.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    recovery_notify.notified().await;
                    if recovery_actor.cast(Message::RecoverCore).is_err() {
                        break;
                    }
                }
            });
        }
        Ok(Self(Arc::new(CoreLifecycleClientInner::Actor {
            actor_ref,
            next_id,
            status,
        })))
    }

    #[cfg(test)]
    pub(super) fn direct(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        clash: ClashConfigClient,
        runtime_paths: RuntimePaths,
    ) -> Self {
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(CoreLifecycleWorkflow::new(
                application,
                clash,
                core,
                Arc::new(FsBinaryInstaller),
                runtime_paths,
            )),
        }))
    }

    async fn execute(&self, command: Command) -> anyhow::Result<()> {
        self.execute_with_timeout(command, CALL_WAIT).await
    }

    async fn execute_with_timeout(
        &self,
        command: Command,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor {
                actor_ref,
                next_id,
                status,
            } => {
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                status.lock().queued.push(id);
                match actor_ref
                    .call(
                        |reply| Message::Execute { id, command, reply },
                        Some(timeout),
                    )
                    .await
                {
                    Ok(CallResult::Success(result)) => result,
                    Ok(CallResult::Timeout) => anyhow::bail!(
                        "core lifecycle operation {id} timed out; it may still be queued or running"
                    ),
                    Ok(CallResult::SenderError) => {
                        status.lock().queued.retain(|queued| *queued != id);
                        anyhow::bail!(
                            "core lifecycle actor reply dropped for operation {id}; outcome is unknown"
                        )
                    }
                    Err(error) => {
                        status.lock().queued.retain(|queued| *queued != id);
                        Err(error.into())
                    }
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow } => {
                workflow.lock().await.execute(command).await
            }
        }
    }

    pub(super) fn status(&self) -> CoreLifecycleStatus {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { status, .. } => status.lock().clone(),
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => CoreLifecycleStatus::default(),
        }
    }

    pub(super) fn request_runtime_rebuild(&self) {
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref, .. } => {
                if actor_ref.cast(Message::RuntimeDirty).is_err() {
                    tracing::warn!("failed to enqueue background runtime rebuild");
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { .. } => {}
        }
    }

    pub(super) async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    pub(super) async fn select_core(
        &self,
        core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.execute(Command::SelectCore(core)).await
    }

    pub(super) async fn reconcile(&self) -> anyhow::Result<()> {
        self.execute(Command::Reconcile).await
    }

    pub(super) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }
}

impl ChimeraClient {
    pub(crate) fn init_core(&self) -> anyhow::Result<()> {
        self.inner.core.init()
    }

    pub(crate) async fn core_status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        self.inner.core.status().await
    }

    pub(crate) fn core_lifecycle_status(&self) -> CoreLifecycleStatus {
        self.inner.core_lifecycle.status()
    }

    pub(crate) fn request_runtime_rebuild(&self) {
        self.inner.core_lifecycle.request_runtime_rebuild();
    }

    pub(crate) fn runtime_transform_diagnostics(
        &self,
    ) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        self.inner.core.runtime_transform_diagnostics()
    }

    pub(crate) fn promoted_runtime_snapshot(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeSnapshot>> {
        self.inner.core.promoted_runtime_snapshot()
    }

    pub(crate) async fn change_core(
        &self,
        clash_core: crate::config::chimera::ClashCore,
    ) -> anyhow::Result<()> {
        self.inner.core_lifecycle.select_core(clash_core).await
    }

    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        self.inner.core_lifecycle.stop_core().await
    }

    pub(crate) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.inner
            .core_lifecycle
            .replace_core_binary(artifact)
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use chimera_config::clash::config::ClashConfig;
    use chimera_ipc::api::status::CoreState;

    use super::*;
    use crate::{config::chimera::ClashCore, core::clash::core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        recovery_notify: Arc<tokio::sync::Notify>,
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl ports::BinaryInstaller for RecordingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            Ok(())
        }
    }

    struct RecordingProgress {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    impl BinaryInstallProgress for RecordingProgress {
        fn restarting(&self) {
            self.events.lock().unwrap().push("restarting");
        }

        fn finished(&self, error: Option<&str>) {
            assert!(error.is_none());
            self.events.lock().unwrap().push("finished");
        }
    }

    #[async_trait]
    impl CoreLifecycleLease for RecordingLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("rebuild");
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("run");
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop-start");
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            self.events.lock().unwrap().push("stop-end");
            Ok(())
        }

        async fn change_core(&mut self, _clash_core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        fn init(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease + '_>> {
            Ok(Box::new(RecordingLease {
                events: self.events.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: RunType::Normal,
            })
        }

        async fn recover(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("recover");
            Ok(())
        }

        fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
            Some(self.recovery_notify.clone())
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[tokio::test]
    async fn mailbox_serializes_lifecycle_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            ApplicationClient::legacy().unwrap(),
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
        )
        .await
        .unwrap();

        let stop_client = client.clone();
        let stop = tokio::spawn(async move { stop_client.stop_core().await.unwrap() });
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        client.select_core(ClashCore::Mihomo).await.unwrap();
        stop.await.unwrap();

        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end", "select"]
        );
    }

    #[tokio::test]
    async fn burst_runtime_dirty_requests_coalesce_into_one_reconcile() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
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
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
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
    async fn timed_out_wait_does_not_cancel_admitted_operation() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
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

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if client.status().completed.iter().any(|entry| entry.id == 1) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
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
        assert_eq!(
            events.lock().unwrap().as_slice(),
            ["stop-start", "stop-end"]
        );
    }

    #[tokio::test]
    async fn crash_recovery_signal_enters_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovery_notify = Arc::new(tokio::sync::Notify::new());
        let _client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: recovery_notify.clone(),
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
                if events.lock().unwrap().contains(&"recover") {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("recovery signal should be admitted by the lifecycle actor");
    }

    #[tokio::test]
    async fn reconcile_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
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

    #[tokio::test]
    async fn replace_binary_runs_inside_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = crate::bridge::verge::legacy_core_from_typed(application.get_typed().core);
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                recovery_notify: Arc::new(tokio::sync::Notify::new()),
            }),
            application,
            ClashConfigClient::legacy().unwrap(),
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root")),
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
            [
                "stop-start",
                "stop-end",
                "install",
                "restarting",
                "run",
                "finished"
            ]
        );
    }
}
