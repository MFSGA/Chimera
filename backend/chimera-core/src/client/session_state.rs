use std::{sync::Arc, time::Duration};

use anyhow::Context as _;
use camino::Utf8PathBuf;
use chimera_config::state::{
    PersistentState, PersistentStatePatch,
    window::{WindowLabel, WindowState},
};
use chimera_state_engine::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use struct_patch::Patch;

use crate::state::{
    ConditionalReplaceResult,
    mirror::{PreparedTypedReplace, WindowLegacyBridge},
    session_state::{
        SessionStateActor, SessionStateActorArgs, SessionStateActorMessage, SessionStateSnapshot,
    },
};

#[cfg(test)]
const SESSION_STATE_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct SessionStateClient {
    inner: Arc<SessionStateClientInner>,
}

enum SessionStateClientInner {
    Actor {
        actor_ref: ActorRef<SessionStateActorMessage>,
        snapshot: StateSnapshot<PersistentState>,
    },
    #[cfg(any(test, feature = "test-support"))]
    Static {
        state: parking_lot::RwLock<PersistentState>,
    },
}

impl SessionStateClient {
    #[cfg(any(test, feature = "test-support"))]
    pub fn legacy() -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(SessionStateClientInner::Static {
                state: parking_lot::RwLock::new(PersistentState::default()),
            }),
        })
    }

    pub async fn new(
        config_path: Utf8PathBuf,
        seed: PersistentState,
        bridge: Arc<dyn WindowLegacyBridge>,
    ) -> anyhow::Result<Self> {
        // Share the startup recovery reader with migrations. A valid typed file
        // wins over the legacy seed; an invalid one is preserved before reset.
        let read_path = config_path.clone();
        let persisted = chimera_owner_support::blocking::join(
            tokio::task::spawn_blocking(move || {
                chimera_migration::session_state::read(read_path.as_std_path())
            })
            .await,
        )
        .context("failed to read or recover session state")?;
        let setup = PersistentStateManagerSetup::<PersistentState>::builder()
            .config_path(config_path)
            .assemble();
        let manager = setup.from_state(persisted.unwrap_or(seed));
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

    pub fn get_typed(&self) -> PersistentState {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { state } => state.read().clone(),
        }
    }

    /// Read the actor-owned window geometry without consulting the legacy
    /// Verge mirror. A valid typed state (including an empty one) is final.
    pub fn window_geometry(&self, label: &str) -> Option<WindowState> {
        self.get_typed()
            .window_state
            .get(&WindowLabel(label.into()))
            .cloned()
    }

    /// Synchronous acknowledgement from the same actor that accepts queued
    /// saves; merging the one entry inside the actor avoids replacing another
    /// queued update with a stale whole-map snapshot.
    pub async fn save_window(&self, label: &str, geometry: WindowState) -> anyhow::Result<()> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { .. } => {
                self.call(
                    |reply| SessionStateActorMessage::SaveWindow {
                        label: WindowLabel(label.into()),
                        geometry,
                        reply: Some(reply),
                    },
                    None,
                )
                .await?;
            }
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { state } => {
                state
                    .write()
                    .window_state
                    .insert(WindowLabel(label.into()), geometry);
            }
        }
        Ok(())
    }

    pub fn snapshot(&self) -> SessionStateSnapshot {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { snapshot, .. } => {
                let snapshot = snapshot.load();
                SessionStateSnapshot {
                    state: snapshot.state.clone(),
                    version: *snapshot.version.as_ref(),
                }
            }
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { state } => SessionStateSnapshot {
                state: state.read().clone(),
                version: 0,
            },
        }
    }
    pub async fn get(&self) -> anyhow::Result<SessionStateSnapshot> {
        Ok(self.snapshot())
    }

    /// Wait for all actor messages already queued by this client to finish.
    /// Shutdown must not use the ordinary read timeout: exiting on timeout
    /// could discard a geometry write that is still being persisted.
    pub async fn wait_for_pending_writes(&self) -> anyhow::Result<()> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { .. } => {
                self.call(SessionStateActorMessage::Get, None).await?;
            }
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { .. } => {}
        }
        Ok(())
    }

    /// Queue the main window geometry on the session-state actor without
    /// waiting on Tauri's window-event thread. The actor serializes saves
    /// with other session-state mutations and persists the typed file.
    pub fn queue_window_save(&self, label: &str, geometry: WindowState) -> anyhow::Result<()> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { actor_ref, .. } => {
                actor_ref
                    .cast(SessionStateActorMessage::SaveWindow {
                        label: WindowLabel(label.into()),
                        geometry,
                        reply: None,
                    })
                    .map_err(|_| anyhow::anyhow!("session state actor stopped"))?;
            }
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { state } => {
                state
                    .write()
                    .window_state
                    .insert(WindowLabel(label.into()), geometry);
            }
        }
        Ok(())
    }

    pub async fn patch(&self, patch: PersistentStatePatch) -> anyhow::Result<SessionStateSnapshot> {
        match self.inner.as_ref() {
            SessionStateClientInner::Actor { .. } => {
                self.call(
                    |reply| SessionStateActorMessage::Patch { patch, reply },
                    None,
                )
                .await
            }
            #[cfg(any(test, feature = "test-support"))]
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

    pub async fn prepare_replace(
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
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub async fn replace_prepared_if_version(
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
            #[cfg(any(test, feature = "test-support"))]
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
            #[cfg(any(test, feature = "test-support"))]
            SessionStateClientInner::Static { .. } => {
                anyhow::bail!("session state actor is unavailable in the static test backend")
            }
        }
    }
}

impl Drop for SessionStateClientInner {
    fn drop(&mut self) {
        match self {
            SessionStateClientInner::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(any(test, feature = "test-support"))]
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
    async fn maximized_and_fullscreen_saves_preserve_normal_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml")).unwrap();
        let bridge: Arc<dyn WindowLegacyBridge> = Arc::new(RecordingBridge {
            legacy: PersistentState::default(),
            mirrored: Arc::new(Mutex::new(None)),
        });
        let client = SessionStateClient::new(path, PersistentState::default(), bridge)
            .await
            .unwrap();
        let normal = WindowState {
            width: 900,
            height: 600,
            x: 10,
            y: 20,
            maximized: false,
            fullscreen: false,
        };
        client.save_window("editor", normal.clone()).await.unwrap();
        let mut maximized = normal.clone();
        maximized.width = 2000;
        maximized.height = 1200;
        maximized.maximized = true;
        client.save_window("editor", maximized).await.unwrap();
        let saved = client.window_geometry("editor").unwrap();
        assert!(saved.maximized);
        assert_eq!(
            (saved.width, saved.height, saved.x, saved.y),
            (normal.width, normal.height, normal.x, normal.y)
        );
        assert!(client.window_geometry("main").is_none());
    }

    #[tokio::test]
    async fn queued_and_acknowledged_geometry_writes_preserve_other_windows() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml")).unwrap();
        let bridge: Arc<dyn WindowLegacyBridge> = Arc::new(RecordingBridge {
            legacy: PersistentState::default(),
            mirrored: Arc::new(Mutex::new(None)),
        });
        let client = SessionStateClient::new(path, PersistentState::default(), bridge)
            .await
            .unwrap();
        let geometry = |width| WindowState {
            width,
            height: 680,
            x: 10,
            y: 20,
            maximized: false,
            fullscreen: false,
        };

        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("editor".into()),
            geometry(500),
        )]));
        client.patch(patch).await.unwrap();

        client.queue_window_save("main", geometry(900)).unwrap();
        client.save_window("main", geometry(1024)).await.unwrap();

        assert_eq!(client.window_geometry("main").map(|s| s.width), Some(1024));
        let state = client.get().await.unwrap().state;
        assert_eq!(state.window_state[&WindowLabel("main".into())].width, 1024);
        assert_eq!(state.window_state[&WindowLabel("editor".into())].width, 500);
        let saved: PersistentState =
            serde_yaml::from_slice(&std::fs::read(dir.path().join("session-state.yaml")).unwrap())
                .unwrap();
        assert_eq!(saved.window_state[&WindowLabel("main".into())].width, 1024);
        assert_eq!(saved.window_state[&WindowLabel("editor".into())].width, 500);
    }

    #[tokio::test]
    async fn shutdown_barrier_waits_until_queued_geometry_is_durable() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("session-state.yaml")).unwrap();
        let bridge: Arc<dyn WindowLegacyBridge> = Arc::new(RecordingBridge {
            legacy: PersistentState::default(),
            mirrored: Arc::new(Mutex::new(None)),
        });
        let client = SessionStateClient::new(path, PersistentState::default(), bridge)
            .await
            .unwrap();
        let SessionStateClientInner::Actor { actor_ref, .. } = client.inner.as_ref() else {
            panic!("session-state actor is unavailable");
        };
        // Keep the actor busy longer than its ordinary read timeout. This
        // makes the test fail if shutdown regresses to `get()`.
        actor_ref
            .cast(SessionStateActorMessage::TestDelay {
                millis: (SESSION_STATE_READ_TIMEOUT + Duration::from_secs(1)).as_millis() as u64,
            })
            .unwrap();
        let geometry = WindowState {
            width: 1120,
            height: 740,
            x: 15,
            y: 25,
            maximized: false,
            fullscreen: false,
        };

        tokio::time::timeout(
            SESSION_STATE_READ_TIMEOUT + Duration::from_secs(10),
            async {
                client.queue_window_save("main", geometry.clone()).unwrap();
                client.wait_for_pending_writes().await.unwrap();
            },
        )
        .await
        .expect("shutdown barrier timed out while a queued write was pending");

        assert_eq!(client.window_geometry("main"), Some(geometry.clone()));
        let saved: PersistentState =
            serde_yaml::from_slice(&std::fs::read(dir.path().join("session-state.yaml")).unwrap())
                .unwrap();
        assert_eq!(saved.window_state[&WindowLabel("main".into())], geometry);
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

    #[tokio::test]
    async fn corrupted_session_is_preserved_and_default_is_mirrored_instead_of_legacy_seed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-state.yaml");
        let invalid = b"window_state: [invalid";
        std::fs::write(&path, invalid).unwrap();
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

        let client = SessionStateClient::new(
            Utf8PathBuf::from_path_buf(path.clone()).unwrap(),
            bridge.snapshot_legacy().unwrap(),
            bridge,
        )
        .await
        .unwrap();
        assert!(client.get_typed().window_state.is_empty());
        assert_eq!(*mirrored.lock().unwrap(), Some(None));
        let saved: PersistentState =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.window_state.is_empty());
        let evidence = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("session-state.yaml.corrupt-")
            })
            .collect::<Vec<_>>();
        assert_eq!(evidence.len(), 1);
        assert_eq!(std::fs::read(&evidence[0]).unwrap(), invalid);

        // A normal post-recovery mutation must still flow through the same
        // actor, persistence coordinator and legacy window mirror.
        let mut patch = PersistentState::new_empty_patch();
        patch.window_state = Some(BTreeMap::from([(
            WindowLabel("main".into()),
            WindowState {
                width: 875,
                height: 600,
                x: 3,
                y: 4,
                maximized: false,
                fullscreen: false,
            },
        )]));
        client.patch(patch).await.unwrap();
        let written: PersistentState =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            written
                .window_state
                .get(&WindowLabel("main".into()))
                .map(|state| state.width),
            Some(875)
        );
        assert_eq!(*mirrored.lock().unwrap(), Some(Some(875)));
        assert_eq!(std::fs::read(&evidence[0]).unwrap(), invalid);
    }

    #[tokio::test]
    async fn missing_session_keeps_legacy_seed_without_creating_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session-state.yaml");
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
        let client = SessionStateClient::new(
            Utf8PathBuf::from_path_buf(path).unwrap(),
            bridge.snapshot_legacy().unwrap(),
            bridge,
        )
        .await
        .unwrap();
        assert_eq!(
            client
                .get_typed()
                .window_state
                .get(&WindowLabel("main".into()))
                .map(|state| state.width),
            Some(640)
        );
        assert_eq!(*mirrored.lock().unwrap(), Some(Some(640)));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
