//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Core mutations are admitted through a client-owned mailbox while execution
//! still uses the legacy CoreManager adapter.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::{sync::Arc, time::Duration};

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::{ChimeraClient, application::ApplicationClient, runtime::RuntimePaths};
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
        runtime_paths: RuntimePaths,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_installer(
            core,
            application,
            runtime_paths,
            Arc::new(FsBinaryInstaller),
        )
        .await
    }

    async fn spawn_with_installer(
        core: Arc<dyn CoreLifecyclePort>,
        application: ApplicationClient,
        runtime_paths: RuntimePaths,
        installer: Arc<dyn ports::BinaryInstaller>,
    ) -> anyhow::Result<Self> {
        let recovery_notify = core.recovery_notify();
        let workflow = CoreLifecycleWorkflow::new(application, core, installer, runtime_paths);
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
        runtime_paths: RuntimePaths,
    ) -> Self {
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(CoreLifecycleWorkflow::new(
                application,
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

    pub(super) async fn replace_core_binary(
        &self,
        artifact: PreparedCoreBinary,
    ) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }
}

impl ChimeraClient {
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
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_trait::async_trait;
    use chimera_config::clash::config::ClashConfig;
    use chimera_ipc::api::status::CoreState;
    use tokio::sync::Notify;

    use super::*;
    use crate::{config::chimera::ClashCore, core::RunType};

    struct RecordingCore {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<Notify>,
        release_stop: Arc<Notify>,
        recovery_notify: Arc<Notify>,
        run_type: RunType,
        restart_args: Arc<Mutex<Vec<(std::path::PathBuf, ClashCore, RunType)>>>,
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
        stop_started: Arc<Notify>,
        release_stop: Arc<Notify>,
        restart_args: Arc<Mutex<Vec<(std::path::PathBuf, ClashCore, RunType)>>>,
    }

    struct RecordingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct FailingInstaller {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    struct RecordingProgress {
        events: Arc<Mutex<Vec<&'static str>>>,
    }

    #[async_trait]
    impl ports::BinaryInstaller for RecordingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            Ok(())
        }
    }

    #[async_trait]
    impl ports::BinaryInstaller for FailingInstaller {
        async fn install(&self, _artifact: &PreparedCoreBinary) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("install");
            anyhow::bail!("test install failure")
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
    impl CoreLifecycleLease for RecordingLease {
        async fn rebuild_running_config(
            &mut self,
            _clash: ClashConfig,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn run_core_from(
            &mut self,
            config_path: &std::path::Path,
            target_core: ClashCore,
            run_type: RunType,
        ) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("run");
            self.restart_args.lock().unwrap().push((
                config_path.to_path_buf(),
                target_core,
                run_type,
            ));
            Ok(())
        }

        async fn stop(&mut self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("stop-start");
            self.stop_started.notify_one();
            self.release_stop.notified().await;
            self.events.lock().unwrap().push("stop-end");
            Ok(())
        }

        async fn change_core(&mut self, _core: ClashCore) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("select");
            Ok(())
        }
    }

    #[async_trait]
    impl CoreLifecyclePort for RecordingCore {
        async fn begin(&self) -> anyhow::Result<Box<dyn CoreLifecycleLease>> {
            Ok(Box::new(RecordingLease {
                events: self.events.clone(),
                stop_started: self.stop_started.clone(),
                release_stop: self.release_stop.clone(),
                restart_args: self.restart_args.clone(),
            }))
        }

        async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
            Ok(CoreStatusSnapshot {
                state: CoreState::Stopped(None),
                state_changed_at: 0,
                run_type: self.run_type,
            })
        }

        async fn recover(&self) -> anyhow::Result<()> {
            self.events.lock().unwrap().push("recover");
            Ok(())
        }

        fn recovery_notify(&self) -> Option<Arc<Notify>> {
            Some(self.recovery_notify.clone())
        }

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[tokio::test]
    async fn mailbox_serializes_stop_and_core_selection() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let stop_started = Arc::new(Notify::new());
        let release_stop = Arc::new(Notify::new());
        let client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: stop_started.clone(),
                release_stop: release_stop.clone(),
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
                restart_args: Arc::new(Mutex::new(Vec::new())),
            }),
            ApplicationClient::legacy().unwrap(),
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
        let CoreLifecycleClientInner::Actor { actor_ref } = client.0.as_ref() else {
            panic!("production lifecycle client should own an actor");
        };
        let (reply, result) = tokio::sync::oneshot::channel();
        if actor_ref
            .send_message(Message::Execute {
                command: Command::SelectCore(ClashCore::Mihomo),
                reply: reply.into(),
            })
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
    async fn crash_recovery_signal_enters_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let recovery_notify = Arc::new(Notify::new());
        let _client = CoreLifecycleClient::spawn(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop: Arc::new(Notify::new()),
                recovery_notify: recovery_notify.clone(),
                run_type: RunType::Normal,
                restart_args: Arc::new(Mutex::new(Vec::new())),
            }),
            ApplicationClient::legacy().unwrap(),
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
    async fn binary_replacement_runs_stop_install_restart_in_lifecycle_mailbox() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = crate::bridge::verge::legacy_core_from_typed(application.get_typed().core);
        let expected_core = target.clone();
        let restart_args = Arc::new(Mutex::new(Vec::new()));
        let release_stop = Arc::new(Notify::new());
        release_stop.notify_one();
        let runtime_paths =
            RuntimePaths::from_config_root(std::path::PathBuf::from("test-runtime-root"));
        let expected_config_path = runtime_paths.product().to_path_buf();
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop,
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Service,
                restart_args: restart_args.clone(),
            }),
            application,
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
            [
                "stop-start",
                "stop-end",
                "install",
                "restarting",
                "run",
                "finished"
            ]
        );
        assert_eq!(
            restart_args.lock().unwrap().as_slice(),
            &[(expected_config_path, expected_core, RunType::Service)]
        );
    }

    #[tokio::test]
    async fn binary_install_failure_finishes_update_without_restarting_core() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let application = ApplicationClient::legacy().unwrap();
        let target = crate::bridge::verge::legacy_core_from_typed(application.get_typed().core);
        let release_stop = Arc::new(Notify::new());
        release_stop.notify_one();
        let client = CoreLifecycleClient::spawn_with_installer(
            Arc::new(RecordingCore {
                events: events.clone(),
                stop_started: Arc::new(Notify::new()),
                release_stop,
                recovery_notify: Arc::new(Notify::new()),
                run_type: RunType::Normal,
                restart_args: Arc::new(Mutex::new(Vec::new())),
            }),
            application,
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
            ["stop-start", "stop-end", "install", "finished-error"]
        );
    }
}
