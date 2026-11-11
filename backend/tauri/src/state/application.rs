use std::sync::Arc;

use anyhow::Context as _;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch, ReleaseChannel};
use chimera_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
};

#[derive(Debug, Clone)]
pub struct ApplicationSnapshot {
    pub state: ChimeraAppConfig,
    pub version: u64,
}

pub struct ApplicationActorArgs {
    pub manager: PersistentStateManager<ChimeraAppConfig>,
    pub bridge: Arc<dyn VergeLegacyBridge>,
    pub build_channel: ReleaseChannel,
}

pub struct ApplicationActorState {
    manager: PersistentStateManager<ChimeraAppConfig>,
    bridge: Arc<dyn VergeLegacyBridge>,
    build_channel: ReleaseChannel,
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ApplicationActorMessage {
    Get(RpcReplyPort<anyhow::Result<ApplicationSnapshot>>),
    Patch {
        patch: ChimeraAppConfigPatch,
        reply: RpcReplyPort<anyhow::Result<ApplicationSnapshot>>,
    },
    Replace {
        state: ChimeraAppConfig,
        reply: RpcReplyPort<anyhow::Result<ApplicationSnapshot>>,
    },
    PrepareReplace {
        state: ChimeraAppConfig,
        reply: RpcReplyPort<anyhow::Result<PreparedTypedReplace<ChimeraAppConfig>>>,
    },
    ReplacePreparedIfVersion {
        expected_version: u64,
        prepared: PreparedTypedReplace<ChimeraAppConfig>,
        reply: RpcReplyPort<anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>>>,
    },
}

pub struct ApplicationActor;

impl ApplicationActor {
    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        ApplicationSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
        }
    }

    fn prepare_replace(
        state: &ApplicationActorState,
        mut next: ChimeraAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ChimeraAppConfig>> {
        Self::validate_release_channel(state, &mut next)?;
        chimera_config::application::validate_update_sources(&next.update_sources)
            .map_err(anyhow::Error::msg)
            .context("invalid application update sources")?;
        let mirror = state
            .bridge
            .prepare(&next)
            .context("failed to prepare legacy application mirror")?;
        Ok(PreparedTypedReplace::new(next, mirror))
    }

    fn validate_release_channel(
        state: &ApplicationActorState,
        next: &mut ChimeraAppConfig,
    ) -> anyhow::Result<()> {
        let current = state.manager.snapshot_handle().load();
        next.release_channel = next
            .release_channel
            .or(current.state.release_channel)
            .or(Some(state.build_channel));
        if state.build_channel == ReleaseChannel::Nightly
            && next.release_channel != Some(ReleaseChannel::Nightly)
        {
            anyhow::bail!(
                "cannot switch from the nightly release channel to {:?}",
                next.release_channel
            );
        }
        Ok(())
    }

    async fn commit(
        state: &mut ApplicationActorState,
        next: ChimeraAppConfig,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let (next, mirror) = Self::prepare_replace(state, next)?.into_parts();
        state
            .manager
            .upsert(next)
            .await
            .context("failed to persist application config")?;
        mirror.apply();
        Ok(Self::snapshot(state))
    }

    async fn replace_prepared_if_version(
        state: &mut ApplicationActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<ChimeraAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist application config")?
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

impl Actor for ApplicationActor {
    type Msg = ApplicationActorMessage;
    type State = ApplicationActorState;
    type Arguments = ApplicationActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ApplicationActorState {
            manager: args.manager,
            bridge: args.bridge,
            build_channel: args.build_channel,
        })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            ApplicationActorMessage::Get(reply) => {
                let _ = reply.send(Ok(Self::snapshot(state)));
            }
            ApplicationActorMessage::Patch { patch, reply } => {
                let result = async {
                    let mut next = state.manager.snapshot_handle().load().state.clone();
                    next.apply(patch);
                    Self::commit(state, next).await
                }
                .await;
                let _ = reply.send(result);
            }
            ApplicationActorMessage::Replace { state: next, reply } => {
                let _ = reply.send(Self::commit(state, next).await);
            }
            ApplicationActorMessage::PrepareReplace { state: next, reply } => {
                let _ = reply.send(Self::prepare_replace(state, next));
            }
            ApplicationActorMessage::ReplacePreparedIfVersion {
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
    use chimera_core::state::PersistentStateManagerSetup;
    use ractor::rpc::CallResult;
    use tempfile::tempdir;

    struct NoopVergeMirror;

    impl VergeLegacyBridge for NoopVergeMirror {
        fn prepare(
            &self,
            _snap: &ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(crate::state::mirror::NoopPreparedLegacyMirror))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
            Ok(ChimeraAppConfig::default())
        }
    }

    struct FailingVergeMirror;

    impl VergeLegacyBridge for FailingVergeMirror {
        fn prepare(
            &self,
            _snap: &ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            anyhow::bail!("injected application mirror prepare failure");
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
            Ok(ChimeraAppConfig::default())
        }
    }

    async fn spawn_actor(
        bridge: Arc<dyn VergeLegacyBridge>,
        build_channel: ReleaseChannel,
    ) -> (ActorRef<ApplicationActorMessage>, tempfile::TempDir) {
        let dir = tempdir().expect("tempdir should be created");
        let config_dir = dir.path().join("config");
        std::fs::create_dir(&config_dir).expect("config directory should be created");
        let path = camino::Utf8PathBuf::from_path_buf(config_dir.join("application.yaml"))
            .expect("temp path should be UTF-8");
        let mut config = ChimeraAppConfig::default();
        config.release_channel = Some(build_channel.resolve(config.release_channel));
        let manager = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(path)
            .assemble()
            .from_state(config)
            .await
            .expect("application manager should initialize");
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                bridge,
                build_channel,
            },
        )
        .await
        .expect("application actor should spawn");
        (actor_ref, dir)
    }

    async fn get_snapshot(
        actor: &ActorRef<ApplicationActorMessage>,
    ) -> anyhow::Result<ApplicationSnapshot> {
        match actor.call(ApplicationActorMessage::Get, None).await? {
            CallResult::Success(result) => result,
            CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
            CallResult::Timeout => anyhow::bail!("application actor call timed out"),
        }
    }

    #[tokio::test]
    async fn japanese_write_failure_preserves_previous_application_state() {
        let (actor, dir) = spawn_actor(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let config_dir = dir.path().join("config");
        let config_path = config_dir.join("application.yaml");

        let mut initial_patch = ChimeraAppConfig::new_empty_patch();
        initial_patch.enable_system_proxy = Some(true);
        match actor
            .call(
                |reply| ApplicationActorMessage::Patch {
                    patch: initial_patch,
                    reply,
                },
                None,
            )
            .await
            .expect("initial actor call should complete")
        {
            CallResult::Success(result) => {
                result.expect("initial config should be persisted");
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }

        let previous_bytes = std::fs::read(&config_path).expect("saved config should exist");
        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");
        assert_eq!(before.state.language.as_str(), "en");
        assert!(before.state.enable_system_proxy);

        let unavailable_dir = dir.path().join("config-unavailable");
        std::fs::rename(&config_dir, &unavailable_dir)
            .expect("config directory should be made unavailable");

        let mut patch = ChimeraAppConfig::new_empty_patch();
        patch.language = Some(chimera_config::application::I18nLanguage::Japanese);
        let result = actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete");
        match result {
            CallResult::Success(result) => {
                let error = result.expect_err("a missing config directory must reject the write");
                assert!(error.to_string().contains("persist application config"));
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.language, before.state.language);
        assert!(after.state.enable_system_proxy);
        assert_eq!(
            std::fs::read(unavailable_dir.join("application.yaml"))
                .expect("previous config should remain available"),
            previous_bytes,
        );

        std::fs::rename(&unavailable_dir, &config_dir)
            .expect("config directory should be restored");
    }

    #[tokio::test]
    async fn mirror_prepare_failure_leaves_state_and_version_unchanged() {
        let (actor, _dir) = spawn_actor(Arc::new(FailingVergeMirror), ReleaseChannel::Stable).await;
        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = ChimeraAppConfig::new_empty_patch();
        patch.enable_system_proxy = Some(true);
        let result = actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete");
        match result {
            CallResult::Success(result) => {
                let error = result.expect_err("mirror prepare must reject the mutation");
                assert!(error.to_string().contains("application mirror"));
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(
            after.state.enable_system_proxy,
            before.state.enable_system_proxy
        );
    }

    #[tokio::test]
    async fn invalid_update_sources_do_not_change_persisted_settings() {
        let (actor, dir) = spawn_actor(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let mut initial_patch = ChimeraAppConfig::new_empty_patch();
        initial_patch.enable_auto_download_update = Some(true);
        match actor
            .call(
                |reply| ApplicationActorMessage::Patch {
                    patch: initial_patch,
                    reply,
                },
                None,
            )
            .await
            .expect("initial actor call should complete")
        {
            CallResult::Success(result) => {
                result.expect("initial config should be persisted");
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }
        let previous_bytes = std::fs::read(dir.path().join("config/application.yaml"))
            .expect("initial config should be readable");
        let before = get_snapshot(&actor)
            .await
            .expect("initial get should succeed");

        let mut patch = ChimeraAppConfig::new_empty_patch();
        patch.update_sources = Some(Vec::new());
        let result = actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await
            .expect("actor call should complete");
        match result {
            CallResult::Success(result) => {
                assert!(
                    result
                        .expect_err("empty source list must be rejected")
                        .to_string()
                        .contains("invalid application update sources")
                );
            }
            CallResult::SenderError => panic!("application actor reply dropped"),
            CallResult::Timeout => panic!("application actor call timed out"),
        }

        let after = get_snapshot(&actor)
            .await
            .expect("post-failure get should succeed");
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.update_sources, before.state.update_sources);
        assert_eq!(
            std::fs::read(dir.path().join("config/application.yaml"))
                .expect("initial config should remain readable"),
            previous_bytes,
        );
    }

    #[tokio::test]
    async fn nightly_build_rejects_channel_changes_in_patch_and_replace() {
        let (actor, _dir) = spawn_actor(Arc::new(NoopVergeMirror), ReleaseChannel::Nightly).await;

        for channel in [ReleaseChannel::Stable, ReleaseChannel::Beta] {
            let mut patch = ChimeraAppConfig::new_empty_patch();
            patch.release_channel = Some(Some(channel));
            let result = actor
                .call(
                    |reply| ApplicationActorMessage::Patch { patch, reply },
                    None,
                )
                .await
                .expect("patch actor call should complete");
            assert!(matches!(
                result,
                CallResult::Success(Err(error)) if error.to_string().contains("cannot switch from the nightly release channel")
            ));

            let replacement = ChimeraAppConfig {
                release_channel: Some(channel),
                ..ChimeraAppConfig::default()
            };
            let result = actor
                .call(
                    |reply| ApplicationActorMessage::Replace {
                        state: replacement,
                        reply,
                    },
                    None,
                )
                .await
                .expect("replace actor call should complete");
            assert!(matches!(
                result,
                CallResult::Success(Err(error)) if error.to_string().contains("cannot switch from the nightly release channel")
            ));
        }

        let mut clear_patch = ChimeraAppConfig::new_empty_patch();
        clear_patch.release_channel = Some(None);
        let result = actor
            .call(
                |reply| ApplicationActorMessage::Patch {
                    patch: clear_patch,
                    reply,
                },
                None,
            )
            .await
            .expect("clear patch actor call should complete");
        assert!(
            matches!(result, CallResult::Success(Ok(snapshot)) if snapshot.state.release_channel == Some(ReleaseChannel::Nightly))
        );
    }
}
