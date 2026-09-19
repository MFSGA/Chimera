//! Client-owned core lifecycle boundary.
//!
//! The directory mirrors ref's `client/core_lifecycle` ownership boundary.
//! Chimera currently routes execution to the legacy CoreManager adapter while
//! lifecycle mutation admission is serialized through a client-owned mailbox.

pub(crate) mod adapters;
pub(crate) mod ports;
mod workflow;

use std::sync::Arc;

use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort, rpc::CallResult};

use super::ChimeraClient;
use workflow::{Command, CoreLifecycleWorkflow};

#[allow(unused_imports)]
pub(crate) use adapters::{CoreUpdateLease, LegacyCoreBridge, LegacyRunningConfigBridge};
#[allow(unused_imports)]
pub(crate) use ports::{
    CoreLifecycleLease, CoreLifecyclePort, CoreStatusSnapshot, RunningConfigPort,
    RuntimeTransformDiagnostics,
};

enum Message {
    Execute {
        command: Command,
        reply: RpcReplyPort<anyhow::Result<()>>,
    },
}

struct CoreLifecycleActor;

impl Actor for CoreLifecycleActor {
    type Msg = Message;
    type State = CoreLifecycleWorkflow;
    type Arguments = Arc<dyn CoreLifecyclePort>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        core: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(CoreLifecycleWorkflow::new(core))
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            Message::Execute { command, reply } => {
                let _ = reply.send(state.execute(command).await);
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
    pub(super) async fn spawn(core: Arc<dyn CoreLifecyclePort>) -> anyhow::Result<Self> {
        let (actor_ref, _handle) = Actor::spawn(None, CoreLifecycleActor, core).await?;
        Ok(Self(Arc::new(CoreLifecycleClientInner::Actor {
            actor_ref,
        })))
    }

    #[cfg(test)]
    pub(super) fn direct(core: Arc<dyn CoreLifecyclePort>) -> Self {
        Self(Arc::new(CoreLifecycleClientInner::Direct {
            workflow: tokio::sync::Mutex::new(CoreLifecycleWorkflow::new(core)),
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

    pub(crate) async fn begin_core_update(&self) -> anyhow::Result<CoreUpdateLease<'_>> {
        Ok(CoreUpdateLease {
            lease: self.inner.core.begin().await?,
        })
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
    }

    struct RecordingLease {
        events: Arc<Mutex<Vec<&'static str>>>,
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
            _config_path: &std::path::Path,
            _target_core: ClashCore,
            _run_type: RunType,
        ) -> anyhow::Result<()> {
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

        async fn on_profile_change(&self, _break_when: bool) {}
    }

    #[tokio::test]
    async fn mailbox_serializes_lifecycle_mutations() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let client = CoreLifecycleClient::spawn(Arc::new(RecordingCore {
            events: events.clone(),
        }))
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
}
