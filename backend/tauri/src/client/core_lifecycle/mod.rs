//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Chimera currently routes execution to the legacy CoreManager adapter while
//! lifecycle mutation admission is serialized through a client-owned mailbox.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{
    ChimeraClient, application::ApplicationClient, clash_config::ClashConfigClient,
    runtime::RuntimePaths,
};
use workflow::{Command, CoreLifecycleWorkflow};

#[allow(unused_imports)]
pub(crate) use adapters::{FsBinaryInstaller, LegacyCoreBridge, LegacyRunningConfigBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    BinaryInstallProgress, CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot,
    PreparedCoreBinary, RunningConfigPort, RuntimeTransformDiagnostics,
};

enum Message {
    Execute {
        command: Command,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
    RecoverCore,
}

struct CoreLifecycleActor;

impl Actor for CoreLifecycleActor {
    type Msg = Message;
    type State = CoreLifecycleWorkflow;
    type Arguments = CoreLifecycleWorkflow;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        workflow: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(workflow)
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Execute { command, reply } => {
                let progress = match &command {
                    Command::ReplaceCoreBinary(artifact) => Some(artifact.progress.clone()),
                    _ => None,
                };
                let result = state.execute(command).await;
                if let Some(progress) = progress {
                    let error = result.as_ref().err().map(ToString::to_string);
                    progress.finished(error.as_deref());
                }
                let _ = reply.send(result);
            }
            Message::RecoverCore => {
                tracing::info!("trying to recover core through lifecycle actor");
                if let Err(error) = state.execute(Command::RecoverCore).await {
                    tracing::error!(%error, "failed to recover core; scheduling retry");
                    let actor = myself.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        let _ = actor.cast(Message::RecoverCore);
                    });
                }
            }
        }
        Ok(())
    }
}

enum CoreLifecycleClientInner {
    Actor {
        actor_ref: ActorRef<Message>,
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
        let (actor_ref, _handle) = Actor::spawn(None, CoreLifecycleActor, workflow).await?;
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
        match self.0.as_ref() {
            CoreLifecycleClientInner::Actor { actor_ref } => {
                match actor_ref
                    .call(|reply| Message::Execute { command, reply }, None)
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("core lifecycle actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("core lifecycle actor call timed out"),
                }
            }
            #[cfg(test)]
            CoreLifecycleClientInner::Direct { workflow } => {
                workflow.lock().await.execute(command).await
            }
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
