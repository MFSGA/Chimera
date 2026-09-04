use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::state::{
    PersistentState, PersistentStatePatch,
    window::{WindowLabel, WindowState},
};
use chimera_core::state::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use struct_patch::Patch;

use crate::state::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, WindowLegacyBridge},
    session_state::{
        SessionStateActor, SessionStateActorArgs, SessionStateActorMessage, SessionStateSnapshot,
    },
};

const SESSION_STATE_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct SessionStateClient {
    inner: Arc<SessionStateClientInner>,
}

enum SessionStateClientInner {
    Actor {
        actor_ref: ActorRef<SessionStateActorMessage>,
        snapshot: StateSnapshot<PersistentState>,
    },
    #[cfg(test)]
    Static {
        state: parking_lot::RwLock<PersistentState>,
    },
}

impl SessionStateClient {
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        #[cfg(test)]
        {
            Ok(Self {
                inner: Arc::new(SessionStateClientInner::Static {
                    state: parking_lot::RwLock::new(PersistentState::default()),
                }),
            })
        }

        #[cfg(not(test))]
        {
            let config_path = Utf8PathBuf::from_path_buf(
                crate::utils::dirs::app_config_dir()?.join("session-state.yaml"),
            )
            .map_err(|path| {
                anyhow::anyhow!("session state path is not UTF-8: {}", path.display())
            })?;
            let bridge: Arc<dyn WindowLegacyBridge> =
                Arc::new(crate::bridge::window::LegacyWindowBridge::default());
            let seed = bridge.snapshot_legacy()?;
            tauri::async_runtime::block_on(Self::new(config_path, seed, bridge))
        }
    }

    async fn new(
        config_path: Utf8PathBuf,
        seed: PersistentState,
        bridge: Arc<dyn WindowLegacyBridge>,
    ) -> anyhow::Result<Self> {
        let should_load = config_path.exists();
        let setup = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load session persistent state manager")?
        } else {
            setup
                .from_state(seed)
                .await
                .context("failed to initialize session persistent state manager")?
        };
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            SessionStateActor,
            SessionStateActorArgs {
                manager,
                bridge: bridge.clone(),
            },
        )
        .await
        .context("failed to spawn session state actor")?;

        let client = Self {
            inner: Arc::new(SessionStateClientInner::Actor {
                actor_ref,
                snapshot,
            }),
        };
        bridge
            .prepare(&client.get_typed())
            .context("failed to prepare loaded session legacy mirror")?
            .apply();
        Ok(client)
    }

    pub(crate) fn get_typed(&self) -> PersistentState {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(test)]
            SessionStateClientInner::Static { state } => state.read().clone(),
        }
    }

    pub(crate) async fn get(&self) -> anyhow::Result<SessionStateSnapshot> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { .. } => {
                self.call(
                    SessionStateActorMessage::Get,
                    Some(SESSION_STATE_READ_TIMEOUT),
                )
                .await
            }
            #[cfg(test)]
            SessionStateClientInner::Static { state } => Ok(SessionStateSnapshot {
                state: state.read().clone(),
                version: 0,
            }),
        }
    }

    pub(crate) async fn patch(
        &self,
        patch: PersistentStatePatch,
    ) -> anyhow::Result<SessionStateSnapshot> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { .. } => {
                self.call(
                    |reply| SessionStateActorMessage::Patch { patch, reply },
                    None,
                )
                .await
            }
            #[cfg(test)]
            SessionStateClientInner::Static { state } => {
                let mut state = state.write();
                state.apply(patch);
                Ok(SessionStateSnapshot {
                    state: state.clone(),
                    version: 0,
                })
            }
        }
    }

    pub(crate) async fn prepare_replace(
        &self,
        state: PersistentState,
    ) -> anyhow::Result<PreparedTypedReplace<PersistentState>> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| SessionStateActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
                }
            }
            #[cfg(test)]
            SessionStateClientInner::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub(crate) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<PersistentState>,
    ) -> anyhow::Result<ConditionalReplaceResult<SessionStateSnapshot>> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| SessionStateActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
                }
            }
            #[cfg(test)]
            SessionStateClientInner::Static { state } => {
                let (next, mirror) = prepared.into_parts();
                *state.write() = next.clone();
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(SessionStateSnapshot {
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
    ) -> anyhow::Result<SessionStateSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<SessionStateSnapshot>>) -> SessionStateActorMessage,
    {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { actor_ref, .. } => {
                match actor_ref.call(make, timeout).await? {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("session state actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("session state actor call timed out"),
                }
            }
            #[cfg(test)]
            SessionStateClientInner::Static { .. } => {
                anyhow::bail!("session state actor is unavailable in the static test backend")
            }
        }
    }
}

impl super::ChimeraClient {
    pub(crate) async fn save_main_window_state(
        &self,
        state: Option<WindowState>,
    ) -> anyhow::Result<()> {
        let mut next = self.inner.session_state.get_typed();
        let label = WindowLabel(crate::consts::MAIN_WINDOW_LABEL.into());
        if let Some(state) = state {
            next.window_state.insert(label, state);
        } else {
            next.window_state.remove(&label);
        }

        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(next.window_state);
        self.inner.session_state.patch(patch).await?;
        crate::config::core::Config::verge().data().save_file()?;
        Ok(())
    }
}

impl Drop for SessionStateClientInner {
    fn drop(&mut self) {
        match self {
            SessionStateClientInner::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(test)]
            SessionStateClientInner::Static { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use chimera_config::state::window::{WindowLabel, WindowState};
    use std::{collections::BTreeMap, sync::Mutex};

    struct RecordingPreparedMirror {
        width: Option<u32>,
        mirrored: Arc<Mutex<Option<Option<u32>>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            *self.mirrored.lock().unwrap() = Some(self.width);
        }
    }

    struct RecordingBridge {
        legacy: PersistentState,
        mirrored: Arc<Mutex<Option<Option<u32>>>>,
    }

    impl WindowLegacyBridge for RecordingBridge {
        fn prepare(&self, snap: &PersistentState) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedMirror {
                width: snap
                    .window_state
                    .get(&WindowLabel("main".into()))
                    .map(|state| state.width),
                mirrored: self.mirrored.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<PersistentState> {
            Ok(self.legacy.clone())
        }
    }

    #[tokio::test]
    async fn existing_typed_file_wins_over_legacy_seed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-state.yaml");
        let persisted = PersistentState {
            window_state: BTreeMap::from([(
                WindowLabel("main".into()),
                WindowState {
                    width: 980,
                    height: 720,
                    x: 11,
                    y: 12,
                    maximized: false,
                    fullscreen: false,
                },
            )]),
        };
        std::fs::write(&path, serde_yaml::to_string(&persisted).unwrap()).unwrap();

        let legacy = PersistentState {
            window_state: BTreeMap::from([(
                WindowLabel("main".into()),
                WindowState {
                    width: 640,
                    height: 480,
                    x: 1,
                    y: 2,
                    maximized: false,
                    fullscreen: false,
                },
            )]),
        };
        let mirrored = Arc::new(Mutex::new(None));
        let bridge: Arc<dyn WindowLegacyBridge> = Arc::new(RecordingBridge {
            legacy,
            mirrored: mirrored.clone(),
        });
        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let client = SessionStateClient::new(path, bridge.snapshot_legacy().unwrap(), bridge)
            .await
            .unwrap();

        assert_eq!(
            client
                .get_typed()
                .window_state
                .get(&WindowLabel("main".into()))
                .map(|state| state.width),
            Some(980)
        );
        assert_eq!(*mirrored.lock().unwrap(), Some(Some(980)));
    }
}
