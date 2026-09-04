//! Application configuration client boundary.
//!
//! Application state is actor-owned and persisted as typed `application.yaml`.
//! The legacy combined verge model remains a compatibility mirror while callers
//! are migrated to the ref-style typed facade.

use std::{sync::Arc, time::Duration};

use super::ChimeraClient;
use crate::{
    config::{chimera::IVerge, core::Config},
    core::handle,
    state::{
        ConditionalReplaceResult,
        application::{
            ApplicationActor, ApplicationActorArgs, ApplicationActorMessage, ApplicationSnapshot,
        },
        mirror::{PreparedTypedReplace, VergeLegacyBridge},
    },
};
use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
#[cfg(test)]
use struct_patch::Patch;

const APPLICATION_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct ApplicationClient {
    inner: Arc<ApplicationClientInner>,
}

enum ApplicationClientInner {
    Actor {
        actor_ref: ActorRef<ApplicationActorMessage>,
        snapshot: StateSnapshot<ChimeraAppConfig>,
    },
    #[cfg(test)]
    Static {
        state: parking_lot::RwLock<ChimeraAppConfig>,
    },
}

impl ApplicationClient {
    #[cfg(test)]
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(ApplicationClientInner::Static {
                state: parking_lot::RwLock::new(ChimeraAppConfig::default()),
            }),
        })
    }

    pub(super) async fn new(
        config_path: Utf8PathBuf,
        seed: ChimeraAppConfig,
        bridge: Arc<dyn VergeLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<ChimeraAppConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load application persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize application persistent state manager")?
        };
        let snapshot = manager.snapshot_handle();

        let (actor_ref, _handle) = Actor::spawn(
            None,
            ApplicationActor,
            ApplicationActorArgs { manager, bridge },
        )
        .await
        .context("failed to spawn application actor")?;

        Ok(Self {
            inner: Arc::new(ApplicationClientInner::Actor {
                actor_ref,
                snapshot,
            }),
        })
    }

    pub(crate) fn get_legacy(&self) -> IVerge {
        Config::verge().latest().clone()
    }

    pub(crate) fn get_typed(&self) -> ChimeraAppConfig {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(test)]
            ApplicationClientInner::Static { state } => state.read().clone(),
        }
    }

    pub(super) async fn get(&self) -> anyhow::Result<ApplicationSnapshot> {
        match self.inner.as_ref() {
            ApplicationClientInner::Actor { .. } => {
                self.call(ApplicationActorMessage::Get, Some(APPLICATION_READ_TIMEOUT))
                    .await
            }
            #[cfg(test)]
            ApplicationClientInner::Static { state } => Ok(ApplicationSnapshot {
                state: state.read().clone(),
                version: 0,
            }),
        }
    }

    async fn patch_typed(
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
            #[cfg(test)]
            ApplicationClientInner::Static { state } => {
                let mut state = state.write();
                state.apply(patch);
                Ok(ApplicationSnapshot {
                    state: state.clone(),
                    version: 0,
                })
            }
        }
    }

    pub(super) async fn prepare_replace(
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
            #[cfg(test)]
            ApplicationClientInner::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub(super) async fn replace_prepared_if_version(
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
            #[cfg(test)]
            ApplicationClientInner::Static { state } => {
                let (next, mirror) = prepared.into_parts();
                *state.write() = next.clone();
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(ApplicationSnapshot {
                    state: next,
                    version: expected_version + 1,
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
            #[cfg(test)]
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
            #[cfg(test)]
            ApplicationClientInner::Static { .. } => {}
        }
    }
}

impl ChimeraClient {
    pub(crate) fn get_app_config(&self) -> anyhow::Result<ChimeraAppConfig> {
        Ok(self.inner.application.get_typed())
    }

    pub(crate) fn application_config(&self) -> IVerge {
        self.inner.application.get_legacy()
    }

    #[allow(dead_code)]
    pub(crate) async fn patch_app_config(
        &self,
        patch: ChimeraAppConfigPatch,
    ) -> anyhow::Result<()> {
        self.inner.application.patch_typed(patch).await?;
        Config::verge().data().save_file()?;
        handle::Handle::refresh_verge();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use std::sync::{Arc, Mutex};

    struct RecordingPreparedMirror {
        value: bool,
        mirrored: Arc<Mutex<Option<bool>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            *self.mirrored.lock().unwrap() = Some(self.value);
        }
    }

    struct RecordingBridge {
        legacy: ChimeraAppConfig,
        mirrored: Arc<Mutex<Option<bool>>>,
    }

    impl VergeLegacyBridge for RecordingBridge {
        fn prepare(
            &self,
            snap: &ChimeraAppConfig,
        ) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedMirror {
                value: snap.enable_auto_check_update,
                mirrored: self.mirrored.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ChimeraAppConfig> {
            Ok(self.legacy.clone())
        }
    }

    #[tokio::test]
    async fn existing_typed_file_wins_over_legacy_seed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("application.yaml");

        let persisted = ChimeraAppConfig {
            enable_auto_check_update: false,
            ..ChimeraAppConfig::default()
        };
        std::fs::write(&path, serde_yaml::to_string(&persisted).unwrap()).unwrap();

        let legacy = ChimeraAppConfig {
            enable_auto_check_update: true,
            ..ChimeraAppConfig::default()
        };
        let mirrored = Arc::new(Mutex::new(None));
        let bridge: Arc<dyn VergeLegacyBridge> = Arc::new(RecordingBridge {
            legacy,
            mirrored: mirrored.clone(),
        });

        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let client = ApplicationClient::new(path, bridge.snapshot_legacy().unwrap(), bridge)
            .await
            .unwrap();

        assert!(!client.get_typed().enable_auto_check_update);
        assert_eq!(*mirrored.lock().unwrap(), None);
    }
}
