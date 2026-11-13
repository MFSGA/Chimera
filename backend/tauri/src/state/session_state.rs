use std::sync::Arc;

use anyhow::Context as _;
use chimera_config::state::{
    PersistentState, PersistentStatePatch,
    window::{WindowLabel, WindowState},
};
use chimera_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, WindowLegacyBridge},
};

#[derive(Debug, Clone)]
pub struct SessionStateSnapshot {
    pub state: PersistentState,
    pub version: u64,
}

pub struct SessionStateActorArgs {
    pub manager: PersistentStateManager<PersistentState>,
    pub bridge: Arc<dyn WindowLegacyBridge>,
}

pub struct SessionStateActorState {
    manager: PersistentStateManager<PersistentState>,
    bridge: Arc<dyn WindowLegacyBridge>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum SessionStateActorMessage {
    Get(RpcReplyPort<anyhow::Result<SessionStateSnapshot>>),
    Patch {
        patch: PersistentStatePatch,
        reply: RpcReplyPort<anyhow::Result<SessionStateSnapshot>>,
    },
    /// Ref-style, ordered main-window geometry save. A missing reply makes
    /// dismissal fire-and-forget on the native event thread.
    SaveMainWindow {
        geometry: WindowState,
        reply: Option<RpcReplyPort<anyhow::Result<SessionStateSnapshot>>>,
    },
    #[cfg(test)]
    TestDelay {
        millis: u64,
    },
    Replace {
        state: PersistentState,
        reply: RpcReplyPort<anyhow::Result<SessionStateSnapshot>>,
    },
    PrepareReplace {
        state: PersistentState,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<PersistentState>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>>>,
    },
}

pub struct SessionStateActor;

impl SessionStateActor {
    fn snapshot(state: &SessionStateActorState) -> SessionStateSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        SessionStateSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    fn prepare_replace(
        state: &SessionStateActorState,
        next: PersistentState,
    ) -> anyhow::Result<PreparedTypedReplace<PersistentState>> {
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy session mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn commit(
        state: &mut SessionStateActorState,
        next: PersistentState,
    ) -> anyhow::Result<SessionStateSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        state
            .manager
            .upsert(next)
            .await
            .context("failed to persist session state")?;
        mirror.apply();
        Ok(Self::snapshot(state))
    }

    async fn replace_prepared_if_version(
        state: &mut SessionStateActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
    ) -> anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist session state")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(Self::snapshot(state)))
            }
            ReplaceIfVersionResult::Conflict { actual_version } => {
                Ok(ConditionalReplaceResult::Conflict {
                    actual_version: *actual_version.as_ref(),
                })
            }
        }
    }
}

impl Actor for SessionStateActor {
    type Msg = SessionStateActorMessage;
    type State = SessionStateActorState;
    type Arguments = SessionStateActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(SessionStateActorState {
            manager: args.manager,
            bridge: args.bridge,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SessionStateActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            SessionStateActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            SessionStateActorMessage::SaveMainWindow { geometry, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.window_state.insert(
                        WindowLabel(crate::consts::MAIN_WINDOW_LABEL.into()),
                        geometry,
                    );
                    Self::commit(state, next).await
                }
                .await;
                if let Some(reply) = reply {
                    let _ = reply.send(result);
                } else if let Err(error) = result {
                    tracing::error!(%error, "queued main window geometry save failed");
                }
            }
            #[cfg(test)]
            SessionStateActorMessage::TestDelay { millis } => {
                tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
            }
            SessionStateActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
            SessionStateActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            SessionStateActorMessage::ReplacePreparedIfVersion {
                expected_version,
                prepared,
                reply,
            } => {
                let _ = reply.send(
                    Self::replace_prepared_if_version(state, expected_version, prepared).await,
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use chimera_config::state::window::{WindowLabel, WindowState};
    use chimera_core::state::PersistentStateManagerSetup;
    use ractor::rpc::CallResult;
    use std::collections::BTreeMap;
    use struct_patch::Patch;
    use tempfile::tempdir;

    struct FailingWindowMirror;

    struct AcceptingWindowMirror;

    impl WindowLegacyBridge for AcceptingWindowMirror {
        fn prepare(
            &self,
            _snap: &PersistentState,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    impl WindowLegacyBridge for FailingWindowMirror {
        fn prepare(
            &self,
            _snap: &PersistentState,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected session mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(PersistentState::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn WindowLegacyBridge>,
    ) -> (ActorRef<SessionStateActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(path)
            .assemble()
            .from_state(PersistentState::default())
            .await
            .expect("session manager should initialize");
        let (actor_ref, _handle) = Actor::spawn(
            None,
            SessionStateActor,
            SessionStateActorArgs { manager, bridge },
        )
        .await
        .expect("session state actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<SessionStateActorMessage>,
    ) -> anyhow::Result<SessionStateSnapshot> {
        match actor.call(SessionStateActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
        }
    }

    #[tokio::test]
    async fn queued_window_geometry_is_ordered_and_durable_before_get() {
        let (actor, dir) = spawn_actor(Arc::new(AcceptingWindowMirror)).await;
        let geometry = |width| WindowState {
            width,
            height: 768,
            x: 10,
            y: 20,
            maximized: false,
            fullscreen: false,
        };

        // A different window entry must survive the main-window patch.
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("other".into()),
            geometry(500),
        )]));
        match actor
            .call(
                |reply| SessionStateActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(result) => {
                result.unwrap();
            }
            _ => panic!("preparation patch failed"),
        }

        // A main-thread dismissal only casts. A subsequent Get is a fence:
        // both queued writes must be applied before it returns.
        for width in [900, 1024] {
            actor
                .cast(SessionStateActorMessage::SaveMainWindow {
                    geometry: geometry(width),
                    reply: None,
                })
                .unwrap();
        }
        let snapshot = get_snapshot(&actor).await.unwrap();
        assert_eq!(
            snapshot
                .state
                .window_state
                .get(&WindowLabel(crate::consts::MAIN_WINDOW_LABEL.into()))
                .unwrap()
                .width,
            1024
        );
        assert_eq!(
            snapshot
                .state
                .window_state
                .get(&WindowLabel("other".into()))
                .unwrap()
                .width,
            500
        );

        let saved = std::fs::read_to_string(dir.path().join("session-state.yaml")).unwrap();
        assert!(saved.contains("1024"), "queued geometry was not persisted");
    }

    #[tokio::test]
    async fn mirror_prepare_failure_returns_error_without_commit() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingWindowMirror)).await;
        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        let before_version = before.version;
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("main".into()),
            WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            },
        )]));

        let err = match actor
            .call(
                |reply| SessionStateActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete")
        {
            CallResult::Success(result) => result.expect_err("mirror failure must surface"),
            CallResult::SenderError => panic!("session state actor reply dropped"),
            CallResult::Timeout => panic!("session state actor call timed out"),
        };
        assert!(err.to_string().contains("session mirror"));

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.state.window_state, before.state.window_state);
        assert_eq!(after.version, before_version);
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingWindowMirror)).await;
        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("main".into()),
            WindowState {
                width: 1024,
                height: 768,
                x: 10,
                y: 20,
                maximized: false,
                fullscreen: false,
            },
        )]));
        let _ = actor
            .call(
                |reply| SessionStateActorMessage::Patch { patch, reply },
                None,
            )
            .await;
        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.window_state, before.state.window_state);
    }
}
