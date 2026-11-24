use std::sync::Arc;

use anyhow::Context as _;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch, ReleaseChannel};
use chimera_core_manager::OperationId;
use chimera_state_engine::{PersistentStateManager, ReplaceIfVersionResult, Version};
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};
use struct_patch::Patch;

use super::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
    mutation::impact,
};

#[derive(Debug, Clone)]
pub struct ApplicationSnapshot {
    pub state: ChimeraAppConfig,
    pub version: u64,
    /// Whether the committed document changes inputs consumed by the runtime.
    pub reaches_runtime: bool,
    pub(crate) operation_id: Option<OperationId>,
}

pub struct ApplicationActorArgs {
    pub manager: PersistentStateManager<ChimeraAppConfig>,
    pub bridge: Arc<dyn VergeLegacyBridge>,
    pub build_channel: ReleaseChannel,
    pub settings_changes: tokio::sync::watch::Sender<ChimeraAppConfig>,
}

pub struct ApplicationActorState {
    manager: PersistentStateManager<ChimeraAppConfig>,
    bridge: Arc<dyn VergeLegacyBridge>,
    build_channel: ReleaseChannel,
    settings_changes: tokio::sync::watch::Sender<ChimeraAppConfig>,
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

/// Validate the entire policy at the source owner for Patch, Replace and CAS.
/// Persisting disabled rules is safe. Until the observer and authorized RPC
/// are wired, accepting an enabled policy would falsely promise automation.
pub(crate) fn validate_network_automation_policy(
    candidate: &ChimeraAppConfig,
) -> anyhow::Result<()> {
    candidate
        .core_logs
        .validate()
        .map_err(anyhow::Error::msg)
        .context("invalid Core log storage settings")?;
    candidate
        .network_automation
        .validate()
        .map_err(|error| anyhow::anyhow!("invalid network automation policy: {error}"))?;
    anyhow::ensure!(
        !candidate.network_automation.enabled,
        "network automation cannot be enabled without a production observer and authorized control RPC"
    );
    Ok(())
}

impl ApplicationActor {
    fn snapshot(state: &ApplicationActorState) -> ApplicationSnapshot {
        let snapshot = state.manager.snapshot_handle().load();
        ApplicationSnapshot {
            state: snapshot.state.clone(),
            version: *snapshot.version.as_ref(),
            reaches_runtime: false,
            operation_id: None,
        }
    }

    fn prepare_replace(
        state: &ApplicationActorState,
        mut next: ChimeraAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ChimeraAppConfig>> {
        Self::validate_release_channel(state, &mut next)?;
        validate_network_automation_policy(&next)?;
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
        let current = state.manager.snapshot_handle().load();
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        let version = current.version;
        drop(current);
        match state
            .manager
            .replace_if_version(version, next)
            .await
            .context("failed to persist application config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = reaches_runtime.then(OperationId::generate);
                state.settings_changes.send_replace(snapshot.state.clone());
                Ok(snapshot)
            }
            ReplaceIfVersionResult::Conflict { actual_version } => anyhow::bail!(
                "application config version conflict: expected {}, actual {}",
                *version.as_ref(),
                *actual_version.as_ref()
            ),
        }
    }

    async fn replace_prepared_if_version(
        state: &mut ApplicationActorState,
        expected_version: u64,
        prepared: PreparedTypedReplace<ChimeraAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        let (next, mirror) = prepared.into_parts();
        validate_network_automation_policy(&next)?;
        let current = state.manager.snapshot_handle().load();
        let reaches_runtime = impact::reaches_runtime(&current.state, &next, &[]);
        drop(current);
        match state
            .manager
            .replace_if_version(Version::new(expected_version), next)
            .await
            .context("failed to conditionally persist application config")?
        {
            ReplaceIfVersionResult::Replaced => {
                mirror.apply();
                let mut snapshot = Self::snapshot(state);
                snapshot.reaches_runtime = reaches_runtime;
                snapshot.operation_id = reaches_runtime.then(OperationId::generate);
                state.settings_changes.send_replace(snapshot.state.clone());
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
            settings_changes: args.settings_changes,
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
    use chimera_state_engine::PersistentStateManagerSetup;
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
        let (actor, dir, _settings_changes) =
            spawn_actor_with_receiver(bridge, build_channel).await;
        (actor, dir)
    }

    async fn spawn_actor_with_receiver(
        bridge: Arc<dyn VergeLegacyBridge>,
        build_channel: ReleaseChannel,
    ) -> (
        ActorRef<ApplicationActorMessage>,
        tempfile::TempDir,
        tokio::sync::watch::Receiver<ChimeraAppConfig>,
    ) {
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
            .from_state(config.clone());
        let (settings_changes, rx) = tokio::sync::watch::channel(config);
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                bridge,
                build_channel,
                settings_changes,
            },
        )
        .await
        .expect("application actor should spawn");
        (actor_ref, dir, rx)
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

    fn disabled_home_policy() -> chimera_config::application::NetworkAutomation {
        serde_json::from_value(serde_json::json!({
            "enabled": false,
            "default_actions": { "system_proxy": "enable" },
            "rules": [{
                "id": "home",
                "name": "Home",
                "conditions": { "wifi_ssid": "Home" },
                "actions": { "system_proxy": "disable" }
            }]
        }))
        .unwrap()
    }

    async fn patch_network_policy(
        actor: &ActorRef<ApplicationActorMessage>,
        policy: chimera_config::application::NetworkAutomation,
    ) -> anyhow::Result<ApplicationSnapshot> {
        let mut patch = ChimeraAppConfig::new_empty_patch();
        patch.network_automation = Some(policy);
        match actor
            .call(
                |reply| ApplicationActorMessage::Patch { patch, reply },
                None,
            )
            .await?
        {
            CallResult::Success(result) => result,
            other => anyhow::bail!("network policy actor reply failed: {other:?}"),
        }
    }

    #[tokio::test]
    async fn disabled_network_policy_persists_and_notifies_only_after_commit() {
        let (actor, dir, mut changes) =
            spawn_actor_with_receiver(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let before = get_snapshot(&actor).await.unwrap();
        assert!(!changes.has_changed().unwrap());

        let policy = disabled_home_policy();
        let committed = patch_network_policy(&actor, policy.clone()).await.unwrap();
        assert!(committed.version > before.version);
        assert!(!committed.reaches_runtime);
        assert!(!committed.state.network_automation.enabled);
        changes.changed().await.unwrap();
        assert_eq!(changes.borrow_and_update().network_automation, policy);
        assert!(!changes.has_changed().unwrap());

        let persisted: ChimeraAppConfig = serde_yaml_ng::from_slice(
            &std::fs::read(dir.path().join("config/application.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(persisted.network_automation, policy);

        // Replacing rules clears previous conditions instead of merging them.
        let empty = chimera_config::application::NetworkAutomation::default();
        patch_network_policy(&actor, empty.clone()).await.unwrap();
        changes.changed().await.unwrap();
        assert_eq!(changes.borrow_and_update().network_automation, empty);
    }

    #[tokio::test]
    async fn invalid_or_enabled_network_policy_rejects_without_persisting_or_notifying() {
        let (actor, dir, mut changes) =
            spawn_actor_with_receiver(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let valid = disabled_home_policy();
        let before = patch_network_policy(&actor, valid.clone()).await.unwrap();
        changes.changed().await.unwrap();
        changes.borrow_and_update();
        let previous_bytes = std::fs::read(dir.path().join("config/application.yaml")).unwrap();

        let mut enabled = valid.clone();
        enabled.enabled = true;
        let err = patch_network_policy(&actor, enabled)
            .await
            .expect_err("no production owner can activate network automation");
        assert!(err.to_string().contains("cannot be enabled"));

        let mut unnamed = valid.clone();
        unnamed.rules[0].name.clear();
        let err = patch_network_policy(&actor, unnamed)
            .await
            .expect_err("invalid rule must fail even while disabled");
        assert!(err.to_string().contains("rule 0 has no name"));

        let mut conflicting = valid.clone();
        conflicting.default_actions.tun = chimera_config::application::MechanismAction::Enable;
        conflicting.default_actions.transparent_proxy =
            chimera_config::application::MechanismAction::Enable;
        let err = patch_network_policy(&actor, conflicting)
            .await
            .expect_err("conflicting capture cannot commit");
        assert!(err.to_string().contains("exclude each other"));

        assert_eq!(get_snapshot(&actor).await.unwrap().version, before.version);
        assert_eq!(
            get_snapshot(&actor).await.unwrap().state.network_automation,
            valid
        );
        assert!(!changes.has_changed().unwrap());
        assert_eq!(
            std::fs::read(dir.path().join("config/application.yaml")).unwrap(),
            previous_bytes
        );
    }

    #[tokio::test]
    async fn rejected_cas_policy_replace_does_not_notify_but_success_does() {
        let (actor, _dir, mut changes) =
            spawn_actor_with_receiver(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let original = get_snapshot(&actor).await.unwrap();
        let mut next = original.state.clone();
        next.network_automation = disabled_home_policy();

        let prepared = match actor
            .call(
                |reply| ApplicationActorMessage::PrepareReplace {
                    state: next.clone(),
                    reply,
                },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(prepared)) => prepared,
            other => panic!("valid disabled policy must prepare: {other:?}"),
        };
        let conflict = actor
            .call(
                |reply| ApplicationActorMessage::ReplacePreparedIfVersion {
                    expected_version: original.version + 1,
                    prepared,
                    reply,
                },
                None,
            )
            .await
            .unwrap();
        assert!(matches!(
            conflict,
            CallResult::Success(Ok(ConditionalReplaceResult::Conflict { .. }))
        ));
        assert!(!changes.has_changed().unwrap());
        assert_eq!(
            get_snapshot(&actor).await.unwrap().version,
            original.version
        );

        let prepared = match actor
            .call(
                |reply| ApplicationActorMessage::PrepareReplace { state: next, reply },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(prepared)) => prepared,
            other => panic!("valid policy must prepare: {other:?}"),
        };
        let committed = actor
            .call(
                |reply| ApplicationActorMessage::ReplacePreparedIfVersion {
                    expected_version: original.version,
                    prepared,
                    reply,
                },
                None,
            )
            .await
            .unwrap();
        assert!(matches!(
            committed,
            CallResult::Success(Ok(ConditionalReplaceResult::Replaced(_)))
        ));
        changes.changed().await.unwrap();
        assert_eq!(
            changes.borrow_and_update().network_automation,
            disabled_home_policy()
        );
    }

    #[tokio::test]
    async fn actor_commit_reports_runtime_impact_only_after_durable_source_publish() {
        let (actor, dir) = spawn_actor(Arc::new(NoopVergeMirror), ReleaseChannel::Stable).await;
        let before = get_snapshot(&actor).await.unwrap();
        assert!(!before.reaches_runtime);
        assert!(before.operation_id.is_none());

        let mut ui_patch = ChimeraAppConfig::new_empty_patch();
        ui_patch.language = Some(chimera_config::application::I18nLanguage::Japanese);
        let ui = match actor
            .call(
                |reply| ApplicationActorMessage::Patch {
                    patch: ui_patch,
                    reply,
                },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(snapshot)) => snapshot,
            other => panic!("UI config mutation must commit: {other:?}"),
        };
        assert!(!ui.reaches_runtime);
        assert!(ui.operation_id.is_none());
        assert!(ui.version > before.version);

        let mut runtime_patch = ChimeraAppConfig::new_empty_patch();
        runtime_patch.enable_builtin_enhanced = Some(!ui.state.enable_builtin_enhanced);
        let runtime = match actor
            .call(
                |reply| ApplicationActorMessage::Patch {
                    patch: runtime_patch,
                    reply,
                },
                None,
            )
            .await
            .unwrap()
        {
            CallResult::Success(Ok(snapshot)) => snapshot,
            other => panic!("runtime input source mutation must commit: {other:?}"),
        };
        assert!(runtime.reaches_runtime);
        assert!(runtime.operation_id.is_some());
        assert!(runtime.version > ui.version);
        let persisted: ChimeraAppConfig = serde_yaml_ng::from_slice(
            &std::fs::read(dir.path().join("config/application.yaml")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            persisted.enable_builtin_enhanced,
            runtime.state.enable_builtin_enhanced
        );
        let reread = get_snapshot(&actor).await.unwrap();
        assert_eq!(reread.version, runtime.version);
        assert!(
            !reread.reaches_runtime,
            "read-only snapshots do not claim a new commit"
        );
        assert!(reread.operation_id.is_none());
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
