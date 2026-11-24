//! Shared Clash desired configuration source owner.
use crate::state::{
    ConditionalReplaceResult,
    clash_config::{
        ClashConfigActor, ClashConfigActorArgs, ClashConfigActorMessage, ClashConfigSnapshot,
    },
    mirror::{ClashLegacyBridge, PreparedTypedReplace},
};
use anyhow::{Context as _, Result};
use camino::Utf8PathBuf;
use chimera_config::clash::config::ClashConfig;
use chimera_state_engine::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, rpc::CallResult};
use std::sync::Arc;
#[derive(Clone)]
pub struct ClashConfigClient {
    mutations: crate::state::mutation::MutationCoordinator,
    state: Arc<ClashConfigStateBackend>,
}

enum ClashConfigStateBackend {
    Actor {
        actor_ref: ActorRef<ClashConfigActorMessage>,
        snapshot: StateSnapshot<ClashConfig>,
    },
    #[cfg(any(test, feature = "test-support"))]
    Static {
        // The compatibility fixture must expose real monotonic versions or
        // the committed-version fence mistakes a successful write for a race.
        state: parking_lot::RwLock<(ClashConfig, u64)>,
    },
}

impl ClashConfigClient {
    #[cfg(any(test, feature = "test-support"))]
    pub fn legacy() -> anyhow::Result<Self> {
        Self::legacy_with(ClashConfig::default())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn legacy_with(config: ClashConfig) -> anyhow::Result<Self> {
        Ok(Self::with_state(ClashConfigStateBackend::Static {
            state: parking_lot::RwLock::new((config, 0)),
        }))
    }

    pub async fn new(
        config_path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> anyhow::Result<Self> {
        Self::new_with_mutations(
            config_path,
            seed,
            bridge,
            crate::state::mutation::MutationCoordinator::pending(),
            tokio_util::sync::CancellationToken::new(),
        )
        .await
    }

    pub async fn new_with_mutations(
        config_path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
        mutations: crate::state::mutation::MutationCoordinator,
        shutdown: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<Self> {
        // Ref's typed client fails on malformed config; also keep existing
        // dangling links and inaccessible nodes from becoming default seeds.
        let should_load = chimera_migration::fs::config_node_exists(config_path.as_std_path())
            .with_context(|| format!("failed to inspect clash config {config_path}"))?;
        let setup = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load clash persistent state manager")?
        } else {
            setup.from_state(seed)
        };
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs {
                mutations: mutations.clone(),
                shutdown,
                manager,
                bridge: bridge.clone(),
            },
        )
        .await
        .context("failed to spawn clash config actor")?;

        let client = Self {
            mutations,
            state: Arc::new(ClashConfigStateBackend::Actor {
                actor_ref,
                snapshot,
            }),
        };
        bridge
            .prepare(&client.get_typed())
            .context("failed to prepare loaded clash legacy mirror")?
            .apply();
        Ok(client)
    }

    #[cfg(any(test, feature = "test-support"))]
    fn with_state(state: ClashConfigStateBackend) -> Self {
        Self {
            mutations: crate::state::mutation::MutationCoordinator::pending(),
            state: Arc::new(state),
        }
    }

    /// Read-only ref RuntimeGate source handle, shared with the exact
    /// committing actor. An isolated fixture has no backing persisted manager.
    pub fn committed_snapshot_handle(&self) -> anyhow::Result<StateSnapshot<ClashConfig>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => Ok(snapshot.clone()),
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { .. } => {
                anyhow::bail!("test-only Clash fixture has no persistent state handle")
            }
        }
    }

    /// Read the immutable committed config+version pair without a writer
    /// Actor RPC; a queued source mutation may be awaiting CoreLifecycle.
    pub fn committed_runtime_source(&self) -> (ClashConfig, u64) {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => {
                let observed = snapshot.load();
                (observed.state.clone(), *observed.version)
            }
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { state } => state.read().clone(),
        }
    }

    pub fn get_typed(&self) -> ClashConfig {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { state } => state.read().0.clone(),
        }
    }

    pub fn get(&self) -> Result<ClashConfig> {
        Ok(self.get_typed())
    }

    pub fn snapshot(&self) -> ClashConfigSnapshot {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => {
                ClashConfigSnapshot::from_versioned(&snapshot.load())
            }
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { state } => {
                let source = state.read();
                ClashConfigSnapshot {
                    state: source.0.clone(),
                    version: source.1,
                    reaches_runtime: false,
                    operation_id: None,
                }
            }
        }
    }
    pub async fn get_snapshot(&self) -> anyhow::Result<ClashConfigSnapshot> {
        Ok(self.snapshot())
    }

    pub async fn commit_native(
        &self,
        expected_version: u64,
        candidate: ClashConfig,
        force_check: bool,
        interrupt_mode: bool,
        requested: Vec<crate::client::effects::plan::EffectKind>,
    ) -> anyhow::Result<ClashConfigSnapshot> {
        use crate::state::mutation::{DomainChange, RuntimeCandidate, ports::CheckedReceipt};
        self.mutations.ensure_ready()?;
        let before = self.snapshot();
        anyhow::ensure!(
            before.version == expected_version,
            "source version changed before candidate admission"
        );
        let prepared = self.prepare_replace(candidate).await?;
        let reaches_runtime =
            crate::state::impact::reaches_runtime(&before.state, prepared.state(), &[]);
        let receipt = if reaches_runtime || force_check {
            Some(
                CheckedReceipt::check(
                    self.mutations.runtime_gate().map_err(anyhow::Error::new)?,
                    chimera_contracts::mutation::ConfigDomain::Clash,
                    RuntimeCandidate {
                        change: DomainChange::Clash {
                            candidate: Arc::new(prepared.state().clone()),
                        },
                        staged: Default::default(),
                    },
                    interrupt_mode,
                )
                .await?,
            )
        } else {
            None
        };
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::CommitNative {
                            expected_version,
                            prepared,
                            receipt,
                            requested,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    _ => anyhow::bail!("checked clash_config source owner did not reply"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { .. } => {
                anyhow::bail!("native commits require the persistent source owner")
            }
        }
    }

    pub async fn prepare_replace(
        &self,
        state: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { state } => {
                let mut source = state.write();
                if source.1 != expected_version {
                    return Ok(ConditionalReplaceResult::Conflict {
                        actual_version: source.1,
                    });
                }
                let next_version = source
                    .1
                    .checked_add(1)
                    .context("test Clash version overflow")?;
                let (next, mirror) = prepared.into_parts();
                let reaches_runtime = crate::state::impact::reaches_runtime(&source.0, &next, &[]);
                source.0 = next.clone();
                source.1 = next_version;
                let version = source.1;
                drop(source);
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(ClashConfigSnapshot {
                    state: next,
                    version,
                    reaches_runtime,
                    operation_id: reaches_runtime.then(chimera_core_manager::OperationId::generate),
                }))
            }
        }
    }
}

impl Drop for ClashConfigStateBackend {
    fn drop(&mut self) {
        match self {
            ClashConfigStateBackend::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(any(test, feature = "test-support"))]
            ClashConfigStateBackend::Static { .. } => {}
        }
    }
}
