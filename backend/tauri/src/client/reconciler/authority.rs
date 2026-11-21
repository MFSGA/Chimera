//! The ref Reconciler's last authoritative source read before Core Apply.
//!
//! All three committed source documents and the managed file dependency
//! closure must identify the *same* target as the checked runtime. Unlike a
//! source-actor RPC, these handles cannot deadlock when a source writer is
//! already waiting for the CoreLifecycle mailbox to finish its apply.
use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use chimera_config::runtime::executor::ProfileContentSource;

use super::{
    committed::SourceDomain,
    inputs::{FrozenProfileContent, RuntimeInputs},
};
use crate::{
    client::{application::ApplicationClient, clash_config::ClashConfigClient},
    state::mutation::ProfileContentReader,
};

/// Required on the executor side of the sole Runtime mutating Actor, after
/// the source commit and immediately before any Core command is submitted.
#[async_trait]
pub(in crate::client) trait RuntimeSourceAuthority:
    Send + Sync + 'static
{
    async fn verify(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<()>;
}

pub(in crate::client) struct LiveSourceAuthority {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: ProfileContentReader,
    content: Arc<dyn ProfileContentSource + Send + Sync>,
}

impl LiveSourceAuthority {
    pub(in crate::client) fn new(
        application: ApplicationClient,
        clash: ClashConfigClient,
        profiles: ProfileContentReader,
        content: Arc<dyn ProfileContentSource + Send + Sync>,
    ) -> Self {
        Self {
            application,
            clash,
            profiles,
            content,
        }
    }

    /// A bounded optimistic read. The managed file closure is captured while
    /// Profiles' writer lock is held, and both other source versions are
    /// sampled on either side. The last version check catches a Profile
    /// publication that arrived while comparing the independent domains.
    /// A moving snapshot is a refusal, not a candidate for blind retries.
    async fn read_committed_target(&self) -> anyhow::Result<SourceTarget> {
        let (app, app_version) = self.application.committed_runtime_source();
        let (clash, clash_version) = self.clash.committed_runtime_source();
        let content = self.content.clone();
        let (profiles_version, profiles, frozen) = self
            .profiles
            .committed(|snapshot| async move {
                let frozen = FrozenProfileContent::capture_content(
                    &snapshot.state,
                    content.as_ref(),
                    BTreeMap::new(),
                );
                (*snapshot.version, Arc::new(snapshot.state.clone()), frozen)
            })
            .await;
        // Treat inaccessible managed content as a missing prerequisite, never
        // as a valid target with a reproducible error string for its digest.
        if let Some((path, Err(error))) = frozen.0.iter().find(|(_, text)| text.is_err()) {
            anyhow::bail!("authoritative managed Profile file {path} is unavailable: {error}");
        }
        let (after_app, after_app_version) = self.application.committed_runtime_source();
        let (after_clash, after_clash_version) = self.clash.committed_runtime_source();
        let after_profiles_version = self
            .profiles
            .committed(|snapshot| async move { *snapshot.version })
            .await;
        anyhow::ensure!(
            app_version == after_app_version
                && clash_version == after_clash_version
                && profiles_version == after_profiles_version,
            "authoritative runtime sources changed during the final target read"
        );
        // Version invariance proves the repeated values refer to one snapshot.
        // Keep the final sampled value to detect incorrect read-only clients
        // that accidentally return different data without changing version.
        anyhow::ensure!(
            super::inputs::RuntimeInputs {
                app: app.clone(),
                clash: clash.clone(),
                profiles: profiles.clone(),
                content: frozen.clone(),
            }
            .target_key()
                == super::inputs::RuntimeInputs {
                    app: after_app,
                    clash: after_clash,
                    profiles: profiles.clone(),
                    content: frozen.clone(),
                }
                .target_key(),
            "authoritative application or Clash content changed without a version update"
        );
        let target = RuntimeInputs {
            app,
            clash,
            profiles,
            content: frozen,
        }
        .checked_target_key()?;
        Ok(SourceTarget {
            versions: [app_version, clash_version, profiles_version],
            target,
        })
    }
}

struct SourceTarget {
    versions: [u64; 3],
    target: String,
}

impl SourceTarget {
    fn version(&self, domain: SourceDomain) -> u64 {
        match domain {
            SourceDomain::Application => self.versions[0],
            SourceDomain::Clash => self.versions[1],
            SourceDomain::Profiles => self.versions[2],
        }
    }
}

#[async_trait]
impl RuntimeSourceAuthority for LiveSourceAuthority {
    async fn verify(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<()> {
        let current = self.read_committed_target().await?;
        // A moved Source or target is not a Core rejection. Preserve a
        // typed identity-advanced cause so the sole lifecycle owner schedules
        // a fresh latest-target reconcile rather than recording a refusal.
        use super::attempt::CommittedTargetAdvanced;
        if current.version(domain) != version {
            return Err(anyhow::Error::new(CommittedTargetAdvanced).context(format!(
                "committed {domain:?} version changed: expected {version}, observed {}",
                current.version(domain)
            )));
        }
        if current.target != checked_target {
            return Err(anyhow::Error::new(CommittedTargetAdvanced).context(
                "checked runtime target differs from the authoritative committed sources",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mutation::ProfileContentLock;
    use camino::Utf8PathBuf;
    use chimera_config::{
        application::ChimeraAppConfig,
        clash::config::ClashConfig,
        profile::{
            ConfigDefinition, FileConfig, LocalBinding, ManagedProfilePath, MaterializedFile,
            ProfileDefinition, ProfileId, ProfileItem, ProfileMetadata, ProfileSource, Profiles,
        },
        runtime::executor::PortError,
    };
    use chimera_core::state::PersistentStateManagerSetup;
    use std::sync::Mutex;

    struct Content(Mutex<BTreeMap<String, String>>);
    impl ProfileContentSource for Content {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            self.0
                .lock()
                .unwrap()
                .get(&path.to_string())
                .cloned()
                .ok_or_else(|| format!("missing managed profile file: {path}").into())
        }
    }

    fn selected_profile() -> Profiles {
        let uid = ProfileId("selected".into());
        let mut profiles = Profiles::default();
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
    async fn authoritative_target_covers_committed_source_versions_and_managed_content() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = PersistentStateManagerSetup::<Profiles>::builder()
            .config_path(Utf8PathBuf::from_path_buf(dir.path().join("profiles.yaml")).unwrap())
            .assemble()
            .from_state(selected_profile())
            .await
            .unwrap();
        let app = ApplicationClient::static_state(ChimeraAppConfig::default());
        let clash = ClashConfigClient::legacy_with(ClashConfig::default()).unwrap();
        let content = Arc::new(Content(Mutex::new(BTreeMap::from([(
            "selected.yaml".to_owned(),
            "mode: rule\n".to_owned(),
        )]))));
        let authority = LiveSourceAuthority::new(
            app,
            clash,
            ProfileContentReader::new(manager.snapshot_handle(), ProfileContentLock::default()),
            content.clone(),
        );
        let initial = authority.read_committed_target().await.unwrap();
        assert_eq!(initial.version(SourceDomain::Profiles), 0);
        authority
            .verify(SourceDomain::Profiles, 0, &initial.target)
            .await
            .unwrap();
        let changed_version = authority
            .verify(SourceDomain::Profiles, 1, &initial.target)
            .await
            .unwrap_err();
        assert!(
            changed_version
                .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
                .is_some(),
            "a version advancement must request the latest target rather than refuse Core"
        );
        let changed_target = authority
            .verify(SourceDomain::Profiles, 0, "different-target")
            .await
            .unwrap_err();
        assert!(
            changed_target
                .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
                .is_some(),
            "a stale target must use the single owner's latest-source recovery"
        );

        content
            .0
            .lock()
            .unwrap()
            .insert("selected.yaml".into(), "mode: direct\n".into());
        assert!(
            authority
                .verify(SourceDomain::Profiles, 0, &initial.target)
                .await
                .is_err(),
            "a changed file invalidates the target even without a source version change"
        );
        content.0.lock().unwrap().remove("selected.yaml");
        assert!(
            authority
                .verify(SourceDomain::Profiles, 0, &initial.target)
                .await
                .is_err(),
            "unreadable dependencies must fail closed"
        );
        content
            .0
            .lock()
            .unwrap()
            .insert("selected.yaml".into(), "mode: rule\n".into());
        manager.upsert(selected_profile()).await.unwrap();
        assert!(
            authority
                .verify(SourceDomain::Profiles, 0, &initial.target)
                .await
                .is_err(),
            "a committed Profile version change must not reuse the old checked receipt"
        );
    }
}
