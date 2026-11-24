//! Shared application configuration source owner.
use crate::state::{
    ConditionalReplaceResult,
    application::{
        ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
    },
    mirror::{PreparedTypedReplace, VergeLegacyBridge},
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch, ReleaseChannel};
use chimera_state_engine::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use std::{sync::Arc, time::Duration};
use struct_patch::Patch;
#[derive(Clone)]
pub struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
    settings_changes: tokio::sync::watch::Receiver<ChimeraAppConfig>,
}

enum ApplicationClientInner {
    Actor {
        actor_ref: ActorRef<ApplicationActorMessage>,
        snapshot: StateSnapshot<ChimeraAppConfig>,
    },
    #[cfg(any(test, feature = "test-support"))]
    Static {
        // Keep the test document and its version under the same lock so the
        // compatibility backend exercises the real source actor's CAS fence.
        state: parking_lot::RwLock<(ChimeraAppConfig, u64)>,
        settings_changes: tokio::sync::watch::Sender<ChimeraAppConfig>,
    },
}

impl ApplicationClient {
    #[cfg(any(test, feature = "test-support"))]
    pub fn legacy() -> anyhow::Result<Self> {
        Ok(Self::static_state(ChimeraAppConfig::default()))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn static_state(state: ChimeraAppConfig) -> Self {
        let (settings_changes, receiver) = tokio::sync::watch::channel(state.clone());
        Self {
            inner: Arc::new(ApplicationClientInner::Static {
                state: parking_lot::RwLock::new((state, 0)),
                settings_changes,
            }),
            settings_changes: receiver,
        }
    }

    pub async fn new(
        config_path: Utf8PathBuf,
        seed: ChimeraAppConfig,
        bridge: Arc<dyn VergeLegacyBridge>,
        build_channel: ReleaseChannel,
    ) -> anyhow::Result<Self> {
        // An existing but unreadable node, including a dangling symlink, must
        // fail rather than silently substituting the legacy/default seed.
        let should_load = chimera_migration::fs::config_node_exists(config_path.as_std_path())
            .with_context(|| format!("failed to inspect application config {config_path}"))?;
        let config_path = if should_load {
            let metadata = std::fs::symlink_metadata(config_path.as_std_path())
                .with_context(|| format!("failed to inspect application config {config_path}"))?;
            if metadata.file_type().is_symlink() && config_path.exists() {
                let resolved =
                    std::fs::canonicalize(config_path.as_std_path()).with_context(|| {
                        format!("failed to resolve application config {config_path}")
                    })?;
                Utf8PathBuf::from_path_buf(resolved).map_err(|path| {
                    anyhow::anyhow!(
                        "resolved application config path is not UTF-8: {}",
                        path.display()
                    )
                })?
            } else {
                config_path
            }
        } else {
            config_path
        };
        let setup = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(config_path)
            .assemble();
        let mut seed = seed;
        seed.release_channel = Some(build_channel.resolve(seed.release_channel));
        let mut manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load application persistent state manager")?
        } else {
            setup.from_state(seed)
        };
        let mut initial = manager.snapshot_handle().load().state.clone();
        let channel = match initial.release_channel {
            Some(ReleaseChannel::Nightly) if build_channel != ReleaseChannel::Nightly => {
                build_channel
            }
            preference => build_channel.resolve(preference),
        };
        if initial.release_channel != Some(channel) {
            initial.release_channel = Some(channel);
            manager
                .upsert(initial)
                .await
                .context("failed to normalize installed release channel")?;
        }
        let snapshot = manager.snapshot_handle();
        crate::state::application::validate_network_automation_policy(&snapshot.load().state)
            .context("invalid persisted network automation policy")?;
        let (settings_changes, receiver) =
            tokio::sync::watch::channel(snapshot.load().state.clone());

        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs {
                manager,
                bridge: bridge.clone(),
                build_channel,
                settings_changes,
            },
        )
        .await
        .context("failed to spawn application actor")?;

        let client = Self {
            inner: Arc::new(ApplicationClientInner::Actor {
                actor_ref,
                snapshot,
            }),
            settings_changes: receiver,
        };

        let loaded = client.get().await?.state;
        chimera_config::application::validate_update_sources(&loaded.update_sources)
            .map_err(anyhow::Error::msg)
            .context("invalid persisted application update sources")?;
        bridge
            .prepare(&loaded)
            .context("failed to prepare loaded application legacy mirror")?
            .apply();

        Ok(client)
    }

    /// Read-only ref RuntimeGate source handle. No RPC into a source actor
    /// waiting for a runtime apply, and no independent persisted-state copy.
    pub fn committed_snapshot_handle(&self) -> anyhow::Result<StateSnapshot<ChimeraAppConfig>> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => Ok(snapshot.clone()),
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { .. } => anyhow::bail!(
                "test-only unversioned application fixture has no persistent state handle"
            ),
        }
    }

    /// Read the source document and its monotonic version from one committed
    /// snapshot without messaging the writer Actor. A Core apply that calls
    /// back into a writer waiting for the lifecycle mailbox can deadlock.
    pub fn committed_runtime_source(&self) -> (ChimeraAppConfig, u64) {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => {
                let observed = snapshot.load();
                (observed.state.clone(), *observed.version)
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { state, .. } => state.read().clone(),
        }
    }

    pub fn get_typed(&self) -> ChimeraAppConfig {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { state, .. } => state.read().0.clone(),
        }
    }

    /// Every successful typed application source commit publishes a complete
    /// snapshot. Rejected, conflicting or failed persistence never emits.
    #[allow(dead_code)]
    pub fn subscribe_settings_changes(&self) -> tokio::sync::watch::Receiver<ChimeraAppConfig> {
        self.settings_changes.clone()
    }

    pub fn snapshot(&self) -> ApplicationSnapshot {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => {
                ApplicationSnapshot::from_versioned(&snapshot.load())
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { state, .. } => {
                let source = state.read();
                ApplicationSnapshot {
                    state: source.0.clone(),
                    version: source.1,
                    reaches_runtime: false,
                    operation_id: None,
                }
            }
        }
    }

    pub async fn get(&self) -> anyhow::Result<ApplicationSnapshot> {
        Ok(self.snapshot())
    }

    pub async fn patch_typed(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<ApplicationSnapshot> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { .. } => {
                self.call(
                    |reply| ApplicationActorMessage::Patch { patch, reply },
                    None,
                )
                .await
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static {
                state,
                settings_changes,
            } => {
                let mut source = state.write();
                let next_version = source
                    .1
                    .checked_add(1)
                    .context("test application version overflow")?;
                let before = source.0.clone();
                let mut candidate = before.clone();
                candidate.apply(patch);
                crate::state::application::validate_network_automation_policy(&candidate)?;
                let reaches_runtime =
                    crate::state::impact::reaches_runtime(&before, &candidate, &[]);
                source.0 = candidate.clone();
                source.1 = next_version;
                let snapshot = ApplicationSnapshot {
                    state: candidate,
                    version: source.1,
                    reaches_runtime,
                    operation_id: reaches_runtime.then(chimera_core_manager::OperationId::generate),
                };
                drop(source);
                settings_changes.send_replace(snapshot.state.clone());
                Ok(snapshot)
            }
        }
    }

    pub async fn prepare_replace(
        &self,
        state: ChimeraAppConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ChimeraAppConfig>> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ApplicationActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { .. } => {
                crate::state::application::validate_network_automation_policy(&state)?;
                Ok(PreparedTypedReplace::new(
                    state,
                    Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
                ))
            }
        }
    }

    pub async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ChimeraAppConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ApplicationSnapshot>> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ApplicationActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static {
                state,
                settings_changes,
            } => {
                let mut source = state.write();
                if source.1 != expected_version {
                    return Ok(ConditionalReplaceResult::Conflict {
                        actual_version: source.1,
                    });
                }
                let next_version = source
                    .1
                    .checked_add(1)
                    .context("test application version overflow")?;
                let (next, mirror) = prepared.into_parts();
                crate::state::application::validate_network_automation_policy(&next)?;
                let reaches_runtime = crate::state::impact::reaches_runtime(&source.0, &next, &[]);
                source.0 = next.clone();
                source.1 = next_version;
                let version = source.1;
                drop(source);
                mirror.apply();
                settings_changes.send_replace(next.clone());
                Ok(ConditionalReplaceResult::Replaced(ApplicationSnapshot {
                    state: next,
                    version,
                    reaches_runtime,
                    operation_id: reaches_runtime.then(chimera_core_manager::OperationId::generate),
                }))
            }
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ApplicationSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ApplicationSnapshot>>) -> ApplicationActorMessage,
    {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { actor_ref, .. } => {
                match actor_ref.call(make, timeout).await? {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("application actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("application actor call timed out"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { .. } => {
                anyhow::bail!("application actor is unavailable in the static test backend")
            }
        }
    }
}

impl Drop for ApplicationClientInner {
    fn drop(&mut self) {
        match self {
            ApplicationClientInner::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(any(test, feature = "test-support"))]
            ApplicationClientInner::Static { .. } => {}
        }
    }
}
