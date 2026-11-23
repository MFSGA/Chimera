//! Compatibility source-capture bridge for Chimera's existing CoreLifecycle.
//!
//! The copied upstream `reconciler::preparation` owns the complete typed
//! runtime build. This old module remains only for existing call sites that
//! capture candidate/committed managed files, including staged file bytes.
use std::{collections::BTreeMap, sync::Arc};

use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
    runtime::executor::ProfileContentSource,
};
use chimera_core::state::StateSnapshot;

use super::inputs::{FrozenProfileContent, RuntimeInputs};
use crate::state::mutation::ProfileContentReader;

/// The read-only runtime candidate preparation boundary. All managed file
/// reads are fenced against source publication and file promotion.
pub(in crate::client) struct RuntimePreparation<'a> {
    profile_content: Option<&'a ProfileContentReader>,
    committed_profiles: Option<&'a StateSnapshot<Profiles>>,
    content_source: &'a (dyn ProfileContentSource + Send + Sync),
}

impl<'a> RuntimePreparation<'a> {
    pub fn new(
        profile_content: Option<&'a ProfileContentReader>,
        committed_profiles: Option<&'a StateSnapshot<Profiles>>,
        content_source: &'a (dyn ProfileContentSource + Send + Sync),
    ) -> Self {
        Self {
            profile_content,
            committed_profiles,
            content_source,
        }
    }

    /// Build a candidate from the exact Profiles document supplied by the
    /// source owner. Hold its managed-file fence only during content capture,
    /// never through candidate validation, Core submission or source commit.
    pub async fn capture_inputs(
        &self,
        app: ChimeraAppConfig,
        clash: ClashConfig,
        profiles: Arc<Profiles>,
        staged_content: BTreeMap<String, String>,
    ) -> anyhow::Result<RuntimeInputs> {
        let content = match self.profile_content {
            Some(reader) => {
                reader
                    .candidate(async {
                        FrozenProfileContent::capture_content(
                            &profiles,
                            self.content_source,
                            staged_content,
                        )
                    })
                    .await
            }
            None => FrozenProfileContent::capture_content(
                &profiles,
                self.content_source,
                staged_content,
            ),
        };
        Ok(RuntimeInputs {
            app,
            clash,
            profiles,
            content,
        })
    }

    /// Capture a committed Profiles document *and* its managed file bytes
    /// within one fence. When the snapshot reader has not yet been installed
    /// (legacy tests), use the explicitly supplied fallback snapshot.
    pub async fn capture_committed_inputs(
        &self,
        app: ChimeraAppConfig,
        clash: ClashConfig,
    ) -> anyhow::Result<RuntimeInputs> {
        if let Some(reader) = self.profile_content {
            return reader
                .committed(|versioned| async move {
                    let profiles = Arc::new(versioned.state.clone());
                    let content = FrozenProfileContent::capture_content(
                        &profiles,
                        self.content_source,
                        BTreeMap::new(),
                    );
                    Ok(RuntimeInputs {
                        app,
                        clash,
                        profiles,
                        content,
                    })
                })
                .await;
        }
        let profiles = self.committed_profiles.ok_or_else(|| {
            anyhow::anyhow!("Profile state is unavailable for committed runtime capture")
        })?;
        self.capture_inputs(
            app,
            clash,
            Arc::new(profiles.load().state.clone()),
            BTreeMap::new(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chimera_config::{
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
        },
        runtime::executor::{PortError, ProfileContentSource},
    };
    use chimera_core::state::PersistentStateManagerSetup;

    use crate::state::mutation::ProfileContentLock;

    struct MemoryContent(BTreeMap<String, String>);

    impl ProfileContentSource for MemoryContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            self.0
                .get(&path.to_string())
                .cloned()
                .ok_or_else(|| format!("unavailable managed file: {path}").into())
        }
    }

    fn profiles() -> Profiles {
        let mut profiles = Profiles::default();
        let uid = ProfileId("selected".to_owned());
        profiles.current = Some(uid.clone());
        profiles.items.insert(
            uid.clone(),
            ProfileItem {
                uid,
                metadata: ProfileMetadata {
                    name: "selected".into(),
                    desc: None,
                    custom_name: true,
                },
                definition: ProfileDefinition::Config {
                    config: ConfigDefinition::File(FileConfig {
                        source: ProfileSource::Local {
                            binding: LocalBinding::Managed {
                                materialized: MaterializedFile {
                                    file: ManagedProfilePath::new("selected.yaml").unwrap(),
                                    updated_at: None,
                                },
                            },
                        },
                        transforms: vec![],
                    }),
                },
            },
        );
        profiles
    }

    #[tokio::test]
    async fn candidate_staged_bytes_replace_captured_content_without_touching_commit() {
        let committed = Arc::new(profiles());
        let content = MemoryContent(BTreeMap::from([(
            "selected.yaml".into(),
            "committed".into(),
        )]));
        let prep = RuntimePreparation::new(None, None, &content);
        let inputs = prep
            .capture_inputs(
                ChimeraAppConfig::default(),
                ClashConfig::default(),
                committed.clone(),
                BTreeMap::from([("selected.yaml".into(), "staged".into())]),
            )
            .await
            .unwrap();
        assert_eq!(inputs.profiles.current, committed.current);
        assert_eq!(
            inputs
                .content
                .0
                .get("selected.yaml")
                .unwrap()
                .as_ref()
                .unwrap(),
            "staged"
        );
        assert_eq!(content.0.get("selected.yaml").unwrap(), "committed");
    }

    #[tokio::test]
    async fn committed_capture_is_serialized_with_file_promotion() {
        let temp = tempfile::tempdir().unwrap();
        let path = camino::Utf8PathBuf::from_path_buf(temp.path().join("profiles.yaml")).unwrap();
        let manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(path)
            .assemble()
            .from_state(profiles())
            .await
            .unwrap();
        let lock = ProfileContentLock::default();
        let reader = ProfileContentReader::new(manager.snapshot_handle(), lock.clone());
        let content = MemoryContent(BTreeMap::from([(
            "selected.yaml".into(),
            "committed".into(),
        )]));
        let prep = RuntimePreparation::new(Some(&reader), None, &content);
        let promotion_guard = lock.write().await;
        let mut capture = Box::pin(
            prep.capture_committed_inputs(ChimeraAppConfig::default(), ClashConfig::default()),
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut capture)
                .await
                .is_err(),
            "committed capture must wait for managed-file promotion"
        );
        drop(promotion_guard);
        let inputs = capture.await.unwrap();
        assert_eq!(
            inputs
                .content
                .0
                .get("selected.yaml")
                .unwrap()
                .as_ref()
                .unwrap(),
            "committed"
        );
    }

    #[tokio::test]
    async fn missing_committed_snapshot_remains_an_error() {
        let content = MemoryContent(BTreeMap::new());
        let prep = RuntimePreparation::new(None, None, &content);
        let err = prep
            .capture_committed_inputs(ChimeraAppConfig::default(), ClashConfig::default())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Profile state is unavailable"));
    }
}
