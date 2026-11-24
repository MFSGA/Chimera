use chimera_config::state::window::WindowState;
pub(crate) use chimera_core::client::session_state::SessionStateClient;
impl super::ChimeraClient {
    /// Called by the window-kind dismissal hook; follows ref's queued save.
    pub(crate) fn queue_main_window_geometry_save(
        &self,
        geometry: WindowState,
    ) -> anyhow::Result<()> {
        self.inner
            .session_state
            .queue_window_save(crate::consts::MAIN_WINDOW_LABEL, geometry)
    }

    /// Fence all previously queued geometry saves during graceful shutdown,
    /// then persist the legacy mirror for older consumers of verge.yaml.
    pub(crate) async fn flush_main_window_geometry_saves(&self) -> anyhow::Result<()> {
        self.inner.session_state.wait_for_pending_writes().await?;
        crate::config::core::Config::verge().data().save_file()?;
        Ok(())
    }

    pub(crate) fn main_window_geometry(&self) -> Option<WindowState> {
        self.inner
            .session_state
            .window_geometry(crate::consts::MAIN_WINDOW_LABEL)
    }

    pub(crate) async fn save_main_window_state(&self, state: WindowState) -> anyhow::Result<()> {
        self.inner
            .session_state
            .save_window(crate::consts::MAIN_WINDOW_LABEL, state)
            .await?;
        crate::config::core::Config::verge().data().save_file()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::PreparedLegacyMirror;
    use crate::state::mirror::WindowLegacyBridge;
    use camino::Utf8PathBuf;
    use chimera_config::state::window::{WindowLabel, WindowState};
    use chimera_config::state::{PersistentState, PersistentStatePatch};
    use std::sync::Arc;
    use std::{collections::BTreeMap, sync::Mutex};
    use struct_patch::Patch;

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
    async fn session_corruption_recovers_across_migration_and_client_startup() {
        use crate::{core::migration::Runner, utils::path::PathResolver};
        for corrupt_at in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let paths =
                PathResolver::with_base_dirs(dir.path().join("config"), dir.path().join("data"));
            std::fs::create_dir_all(paths.app_config_dir()).unwrap();
            std::fs::create_dir_all(paths.app_data_dir()).unwrap();
            std::fs::write(
                paths.application_config_path(),
                serde_yaml::to_string(&chimera_config::application::ChimeraAppConfig::default())
                    .unwrap(),
            )
            .unwrap();
            std::fs::write(
                paths.clash_config_path(),
                serde_yaml::to_string(&chimera_config::clash::config::ClashConfig::default())
                    .unwrap(),
            )
            .unwrap();
            std::fs::write(
                paths.session_state_path(),
                serde_yaml::to_string(&PersistentState::default()).unwrap(),
            )
            .unwrap();
            Runner::with_paths(paths.clone(), false)
                .unwrap()
                .run_pending()
                .unwrap();
            let path = paths.session_state_path();
            let bad = b"window_state: [invalid";
            for boundary in 0..3 {
                if boundary == corrupt_at {
                    std::fs::write(&path, bad).unwrap();
                }
                if boundary < 2 {
                    Runner::with_paths(paths.clone(), false)
                        .unwrap()
                        .run_pending()
                        .unwrap();
                }
            }
            let mirrored = Arc::new(Mutex::new(None));
            let bridge: Arc<dyn WindowLegacyBridge> = Arc::new(RecordingBridge {
                legacy: PersistentState::default(),
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
            let evidence = std::fs::read_dir(path.parent().unwrap())
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with("session-state.yaml.corrupt-")
                })
                .collect::<Vec<_>>();
            assert_eq!(evidence.len(), 1, "stage {corrupt_at}");
            assert_eq!(std::fs::read(&evidence[0]).unwrap(), bad);
        }
    }
}
