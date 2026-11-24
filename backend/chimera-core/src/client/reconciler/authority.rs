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
    committed::{Selection, SelectionChange, SourceDomain},
    inputs::{FrozenProfileContent, RuntimeInputs},
};
use crate::{
    client::{application::ApplicationClient, clash_config::ClashConfigClient},
    state::mutation::ProfileContentReader,
};

/// Required on the executor side of the sole Runtime mutating Actor, after
/// the source commit and immediately before any Core command is submitted.
#[async_trait]
pub trait RuntimeSourceAuthority: Send + Sync + 'static {
    async fn verify(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<()>;

    /// Return a selection only when the *same bounded authoritative read*
    /// proved the checked target. Legacy test authorities can prove an apply
    /// without exposing selection; such an unknown value is never inferred.
    async fn verify_selection(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<Option<Selection>> {
        self.verify(domain, version, checked_target).await?;
        Ok(None)
    }

    /// Prepare only an interruption justified by the same committed target
    /// that was checked. A test adapter with no selection evidence may not
    /// manufacture an interruption from source/document differences.
    async fn verify_interruption(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
        _previous: Option<&Selection>,
    ) -> anyhow::Result<Option<SelectionChange>> {
        self.verify(domain, version, checked_target).await?;
        Ok(None)
    }

    /// Re-adopt the selection of a legacy startup/reconcile *only* when its
    /// instance-bound Core receipt identifies the complete latest committed
    /// target. Unlike Checked Apply, no individual source domain version is
    /// carried by the old Core port. Unknown test ports must fail closed.
    async fn selection_for_applied_target(
        &self,
        _applied_target: &str,
    ) -> anyhow::Result<Option<Selection>> {
        Ok(None)
    }

    /// The legacy SourceCommitted/Retry path has no domain CAS version. A
    /// prepared submission can only be admitted when one bounded read proves
    /// its exact complete target and its connection-interruption policy.
    /// Test/legacy authorities have no such evidence and fail closed.
    async fn verify_committed_prepared_target(
        &self,
        _target: &str,
        _previous: Option<&Selection>,
    ) -> anyhow::Result<Option<(Selection, Option<SelectionChange>)>> {
        Ok(None)
    }
}

pub struct LiveSourceAuthority {
    application: ApplicationClient,
    clash: ClashConfigClient,
    profiles: ProfileContentReader,
    content: Arc<dyn ProfileContentSource + Send + Sync>,
}

impl LiveSourceAuthority {
    pub fn new(
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
        let inputs = RuntimeInputs {
            app,
            clash,
            profiles,
            content: frozen,
        };
        let selection = Selection::of(&inputs);
        let interrupt_on_mode = inputs.clash.break_connection.on_mode_change;
        let interrupt_on_profile = inputs.clash.break_connection.on_profile_change;
        let target = inputs.checked_target_key()?;
        Ok(SourceTarget {
            versions: [app_version, clash_version, profiles_version],
            target,
            selection,
            interrupt_on_mode,
            interrupt_on_profile,
        })
    }

    async fn verify_target(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<SourceTarget> {
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
        Ok(current)
    }
}

struct SourceTarget {
    versions: [u64; 3],
    target: String,
    selection: Selection,
    interrupt_on_mode: bool,
    interrupt_on_profile: bool,
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
        self.verify_selection(domain, version, checked_target)
            .await
            .map(|_| ())
    }

    async fn verify_selection(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
    ) -> anyhow::Result<Option<Selection>> {
        Ok(Some(
            self.verify_target(domain, version, checked_target)
                .await?
                .selection,
        ))
    }

    async fn verify_interruption(
        &self,
        domain: SourceDomain,
        version: u64,
        checked_target: &str,
        previous: Option<&Selection>,
    ) -> anyhow::Result<Option<SelectionChange>> {
        let current = self.verify_target(domain, version, checked_target).await?;
        Ok(current.selection.interruption_with_policy(
            previous,
            current.interrupt_on_mode,
            current.interrupt_on_profile,
        ))
    }

    async fn selection_for_applied_target(
        &self,
        applied_target: &str,
    ) -> anyhow::Result<Option<Selection>> {
        let current = self.read_committed_target().await?;
        Ok((current.target == applied_target).then_some(current.selection))
    }

    async fn verify_committed_prepared_target(
        &self,
        target: &str,
        previous: Option<&Selection>,
    ) -> anyhow::Result<Option<(Selection, Option<SelectionChange>)>> {
        let current = self.read_committed_target().await?;
        if current.target != target {
            return Err(anyhow::Error::new(super::attempt::CommittedTargetAdvanced)
                .context("source changed before the prepared committed runtime could be applied"));
        }
        let interruption = current.selection.interruption_with_policy(
            previous,
            current.interrupt_on_mode,
            current.interrupt_on_profile,
        );
        Ok(Some((current.selection, interruption)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::PersistentStateManagerSetup;
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
        let mut clash_state = ClashConfig::default();
        clash_state.break_connection.on_mode_change = true;
        let clash = ClashConfigClient::legacy_with(clash_state).unwrap();
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
        assert_eq!(
            authority
                .selection_for_applied_target(&initial.target)
                .await
                .unwrap(),
            Some(initial.selection.clone()),
            "legacy applied selection requires the exact committed target"
        );
        assert!(
            authority
                .selection_for_applied_target("previously-applied-core-target")
                .await
                .unwrap()
                .is_none(),
            "a running Core receipt for obsolete bytes cannot become the selection baseline"
        );
        authority
            .verify(SourceDomain::Profiles, 0, &initial.target)
            .await
            .unwrap();
        assert_eq!(
            authority
                .verify_selection(SourceDomain::Profiles, 0, &initial.target)
                .await
                .unwrap(),
            Some(initial.selection.clone()),
            "the applied selection must come from the same authoritative committed target"
        );
        use chimera_config::clash::config::overrides::{ClashGuardOverridesPatch, Mode};
        use struct_patch::Patch as _;
        let mut prior_clash = ClashConfig::default();
        prior_clash.overrides.apply(ClashGuardOverridesPatch {
            mode: Some(Mode::Global),
            ..Default::default()
        });
        let previous = Selection::of(&RuntimeInputs {
            app: ChimeraAppConfig::default(),
            clash: prior_clash,
            profiles: Arc::new(selected_profile()),
            content: Default::default(),
        });
        assert_eq!(
            authority
                .verify_interruption(SourceDomain::Profiles, 0, &initial.target, Some(&previous),)
                .await
                .unwrap(),
            Some(SelectionChange::Mode),
            "mode interruption policy is frozen with the verified source"
        );
        assert_eq!(
            authority
                .verify_committed_prepared_target(&initial.target, Some(&previous))
                .await
                .unwrap(),
            Some((initial.selection.clone(), Some(SelectionChange::Mode))),
            "legacy prepared Core admission must use the same source policy as Checked Apply"
        );
        assert_eq!(
            authority
                .verify_interruption(SourceDomain::Profiles, 0, &initial.target, None)
                .await
                .unwrap(),
            None,
            "an unknown applied selection never authorizes interruption"
        );
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

        let mismatch = authority
            .verify_committed_prepared_target("stale-legacy-runtime", Some(&previous))
            .await
            .unwrap_err();
        assert!(
            mismatch
                .downcast_ref::<crate::client::reconciler::attempt::CommittedTargetAdvanced>()
                .is_some()
        );

        content
            .0
            .lock()
            .unwrap()
            .insert("selected.yaml".into(), "mode: direct\n".into());
        assert!(
            authority
                .verify_committed_prepared_target(&initial.target, Some(&previous))
                .await
                .is_err(),
            "managed Profile content changes must invalidate a prepared Local Core admission"
        );
        assert!(
            authority
                .selection_for_applied_target(&initial.target)
                .await
                .unwrap()
                .is_none(),
            "a changed managed file invalidates the old Core receipt's selection"
        );
        assert!(
            authority
                .verify_selection(SourceDomain::Profiles, 0, &initial.target)
                .await
                .is_err(),
            "a changed file cannot authorize a new applied selection"
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
