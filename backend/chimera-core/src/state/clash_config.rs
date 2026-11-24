use std::sync::Arc;

use anyhow::Context as _;
use chimera_config::clash::config::{ClashConfig, ClashConfigPatch};
use chimera_core_manager::OperationId;
use chimera_state_engine::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult, impact,
    mirror::{ClashLegacyBridge, PreparedTypedReplace},
};

/// Snapshot of the saved Clash configuration domain, not live Clash runtime API state.
#[derive(Debug, Clone)]
pub struct ClashConfigSnapshot {
    pub state: ClashConfig,
    pub version: u64,
    /// Whether this durable source commit changes the runtime target.
    pub reaches_runtime: bool,
    pub operation_id: Option<OperationId>,
}

impl ClashConfigSnapshot {
    pub fn from_versioned(versioned: &chimera_state_engine::VersionedState<ClashConfig>) -> Self {
        Self {
            state: versioned.state.clone(),
            version: *versioned.version.as_ref(),
            reaches_runtime: false,
            operation_id: None,
        }
    }
}

pub struct ClashConfigActorArgs {
    pub manager: PersistentStateManager<ClashConfig>,
    pub bridge: Arc<dyn ClashLegacyBridge>,
}

pub struct ClashConfigActorState {
    manager: PersistentStateManager<ClashConfig>,
    bridge: Arc<dyn ClashLegacyBridge>,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ClashConfigActorMessage {
    Get(RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>),
    Patch {
        patch: ClashConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    Replace {
        state: ClashConfig,
        reply: RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>,
    },
    PrepareReplace {
        state: ClashConfig,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<ClashConfig>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>>>,
    },
}

/// Actor-owned persistent Clash configuration. Runtime Clash API state stays in the core/API path.
pub struct ClashConfigActor;

impl ClashConfigActor {
    fn snapshot(state: &ClashConfigActorState) -> ClashConfigSnapshot {
        ClashConfigSnapshot::from_versioned(&state.manager.snapshot_handle().load())
    }

    fn prepare_replace(
        state: &ClashConfigActorState,
        next: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy clash mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    async fn commit(
        state: &mut ClashConfigActorState,
        next: ClashConfig,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        let current = state.manager.snapshot_handle().load();
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        let version = current.version;
        drop(current);
        match state
            .manager
            .replace_if_version(version, next)
            .await
            .context("failed to persist clash config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = reaches_runtime.then(OperationId::generate);
                Ok(snapshot)
            }
            ReplaceIfVersionResult::Conflict { actual_version } => anyhow::bail!(
                "clash config version conflict: expected {}, actual {}",
                *version.as_ref(),
                *actual_version.as_ref()
            ),
        }
    }

    async fn replace_prepared_if_version(
        state: &mut ClashConfigActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        let current = state.manager.snapshot_handle().load();
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        drop(current);
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist clash config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = reaches_runtime.then(OperationId::generate);
                Ok(ConditionalReplaceResult::Replaced(snapshot))
            }
            ReplaceIfVersionResult::Conflict { actual_version } => {
                Ok(ConditionalReplaceResult::Conflict {
                    actual_version: *actual_version.as_ref(),
                })
            }
        }
    }
}

impl Actor for ClashConfigActor {
    type Msg = ClashConfigActorMessage;
    type State = ClashConfigActorState;
    type Arguments = ClashConfigActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ClashConfigActorState {
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
            ClashConfigActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            ClashConfigActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            ClashConfigActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
            ClashConfigActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            ClashConfigActorMessage::ReplacePreparedIfVersion {
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
    use chimera_state_engine::PersistentStateManagerSetup;
    use ractor::rpc::CallResult;
    use struct_patch::Patch;
    use tempfile::tempdir;

    struct NoopClashMirror;

    impl ClashLegacyBridge for NoopClashMirror {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    /// Test-only double that fails every Clash legacy mirror preparation.
    struct FailingClashMirror;

    impl ClashLegacyBridge for FailingClashMirror {
        fn prepare(&self, _snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected clash mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> (ActorRef<ClashConfigActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml"))
            .expect("temp path should be UTF-8");
        let manager = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(ClashConfig::default());
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs { manager, bridge },
        )
        .await
        .expect("clash config actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<ClashConfigActorMessage>,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        match actor.call(ClashConfigActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
        }
    }

    #[tokio::test]
    async fn actor_commit_reports_runtime_impact_only_for_changed_clash_inputs() {
        let (actor, dir) = spawn_actor(Arc::new(NoopClashMirror)).await;
        let before = get_snapshot(&actor).await.unwrap();
        assert!(!before.reaches_runtime);
        assert!(before.operation_id.is_none());

        let mut ui_patch = ClashConfig::new_empty_patch();
        ui_patch.web_ui_list = Some(vec!["https://dashboard.example/".into()]);
        let ui = match actor
            .call(
                |reply| ClashConfigActorMessage::Patch {
                    patch: ui_patch,
                    reply,
                },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(snapshot)) => snapshot,
            other => panic!("empty typed Clash patch must commit: {other:?}"),
        };
        assert!(!ui.reaches_runtime);
        assert!(ui.operation_id.is_none());
        assert!(ui.version > before.version);

        let mut runtime_patch = ClashConfig::new_empty_patch();
        runtime_patch.enable_tun_mode = Some(!ui.state.enable_tun_mode);
        let runtime = match actor
            .call(
                |reply| ClashConfigActorMessage::Patch {
                    patch: runtime_patch,
                    reply,
                },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(snapshot)) => snapshot,
            other => panic!("TUN source mutation must commit: {other:?}"),
        };
        assert!(runtime.reaches_runtime);
        assert!(runtime.operation_id.is_some());
        assert!(runtime.version > ui.version);
        let persisted: ClashConfig = serde_yaml_ng::from_slice(
            &std::fs::read(dir.path().join("clash-config.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.enable_tun_mode, runtime.state.enable_tun_mode);
        let reread = get_snapshot(&actor).await.unwrap();
        assert!(!reread.reaches_runtime);
        assert!(reread.operation_id.is_none());
    }

    #[tokio::test]
    async fn mirror_prepare_failure_returns_error_without_commit() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        assert!(!before.state.enable_tun_mode);
        let before_version = before.version;

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let err = match actor
            .call(
                |reply| ClashConfigActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete")
        {
            CallResult::Success(result) => result
                .expect_err("mirror failure after upsert must surface as Err under current defect"),
            CallResult::SenderError => panic!("clash config actor reply dropped"),
            CallResult::Timeout => panic!("clash config actor call timed out"),
        };
        assert!(
            err.to_string().contains("legacy clash mirror")
                || err.to_string().contains("injected clash mirror failure"),
            "unexpected error: {err:#}"
        );

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.state.enable_tun_mode, before.state.enable_tun_mode);
        assert_eq!(after.version, before_version);
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingClashMirror)).await;

        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let _ = actor
            .call(
                |reply| ClashConfigActorMessage::Patch { patch, reply },
                None,
            )
            .await;

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.enable_tun_mode, before.state.enable_tun_mode);
    }
}
