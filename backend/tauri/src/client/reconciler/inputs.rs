//! Fixed source inputs and content for one runtime target and build.
use crate::state::mutation::impact;
use chimera_config::{
    application::ChimeraAppConfig, clash::config::ClashConfig, profile::Profiles,
};
#[cfg(test)]
use chimera_config::{
    profile::ManagedProfilePath,
    runtime::executor::{PortError, ProfileContentSource},
};
use std::{collections::BTreeMap, sync::Arc};

// The frozen dependency closure belongs to the shared runtime builder, not a
// second Tauri facade. Both ref-style Reconciler and legacy checked gates must
// consume the same captured bytes and read-error semantics.
pub use chimera_core::client::runtime_build_port::FrozenProfileContent;

// Chimera's existing fallible preparation tests use unwrap_err() and require
// Debug for the success payload; the upstream fields are already Debug.
#[derive(Debug, Clone)]
pub struct RuntimeInputs {
    pub app: ChimeraAppConfig,
    pub clash: ClashConfig,
    pub profiles: Arc<Profiles>,
    pub content: FrozenProfileContent,
}

impl RuntimeInputs {
    /// `None` when a projection cannot be serialized: a target with no
    /// identity is never treated as equal to another one.
    pub fn target_key(&self) -> Option<String> {
        let projections = (
            impact::application_target(&self.app)?,
            impact::clash_target(&self.clash)?,
            impact::profiles_target(&self.profiles)?,
            &self.content.0,
        );
        Some(chimera_core_manager::payload_digest(
            &serde_json::to_vec(&projections).ok()?,
        ))
    }

    /// Chimera compatibility adapter: older lifecycle callers expect an error,
    /// while ref keeps the target identity optional when serialization fails.
    pub fn checked_target_key(&self) -> anyhow::Result<String> {
        self.target_key()
            .ok_or_else(|| anyhow::anyhow!("runtime input target identity could not be serialized"))
    }

    pub fn into_parts(
        self,
    ) -> anyhow::Result<(
        ChimeraAppConfig,
        ClashConfig,
        Arc<Profiles>,
        BTreeMap<String, String>,
    )> {
        let staged_content = self
            .content
            .0
            .into_iter()
            .map(|(path, content)| {
                content
                    .map(|content| (path, content))
                    .map_err(anyhow::Error::msg)
            })
            .collect::<anyhow::Result<_>>()?;
        Ok((self.app, self.clash, self.profiles, staged_content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> RuntimeInputs {
        RuntimeInputs {
            app: ChimeraAppConfig::default(),
            clash: ClashConfig::default(),
            profiles: Arc::new(Profiles::default()),
            content: FrozenProfileContent::default(),
        }
    }

    #[test]
    fn target_identity_covers_all_runtime_inputs_but_not_gui_settings() {
        let mut inputs = inputs();
        let first = inputs.target_key().unwrap();
        inputs.app.language = chimera_config::application::I18nLanguage::English;
        assert_eq!(inputs.target_key().unwrap(), first);
        inputs.app.enable_builtin_enhanced = !inputs.app.enable_builtin_enhanced;
        let application_changed = inputs.target_key().unwrap();
        assert_ne!(application_changed, first);
        inputs.clash.enable_tun_mode = !inputs.clash.enable_tun_mode;
        let clash_changed = inputs.target_key().unwrap();
        assert_ne!(clash_changed, application_changed);
        inputs
            .content
            .0
            .insert("profile.yaml".into(), Ok("mode: rule".into()));
        let content_changed = inputs.target_key().unwrap();
        assert_ne!(content_changed, clash_changed);
        inputs
            .content
            .0
            .insert("profile.yaml".into(), Ok("mode: direct".into()));
        assert_ne!(inputs.target_key().unwrap(), content_changed);
    }

    // Chimera-specific managed-file regressions not present in pinned ref.
    struct MapContent(BTreeMap<String, String>);

    impl ProfileContentSource for MapContent {
        fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
            self.0
                .get(&path.to_string())
                .cloned()
                .ok_or_else(|| format!("missing fixture content: {path}").into())
        }
    }

    fn profiles_with_active_file() -> Profiles {
        use chimera_config::profile::{
            ConfigDefinition, FileConfig, LocalBinding, MaterializedFile, ProfileDefinition,
            ProfileId, ProfileItem, ProfileMetadata, ProfileSource,
        };

        let uid = ProfileId("active".into());
        let mut profiles = Profiles::default();
        profiles.current = Some(uid.clone());
        profiles.items.insert(
            uid.clone(),
            ProfileItem {
                uid,
                metadata: ProfileMetadata {
                    name: "active".into(),
                    desc: None,
                    custom_name: true,
                },
                definition: ProfileDefinition::Config {
                    config: ConfigDefinition::File(FileConfig {
                        source: ProfileSource::Local {
                            binding: LocalBinding::Managed {
                                materialized: MaterializedFile {
                                    file: ManagedProfilePath::new("active.yaml").unwrap(),
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

    #[test]
    fn capture_content_freezes_active_files_and_overlays_staged_bytes() {
        let profiles = profiles_with_active_file();
        let source = MapContent(BTreeMap::from([(
            "active.yaml".into(),
            "proxies: []\nmode: rule\n".into(),
        )]));
        let content = FrozenProfileContent::capture_content(
            &profiles,
            &source,
            BTreeMap::from([("active.yaml".into(), "proxies: []\nmode: global\n".into())]),
        );

        assert_eq!(
            content
                .read(&ManagedProfilePath::new("active.yaml").unwrap())
                .unwrap(),
            "proxies: []\nmode: global\n"
        );
    }

    #[test]
    fn capture_content_keeps_source_read_errors_for_candidate_classification() {
        let profiles = profiles_with_active_file();
        let content = FrozenProfileContent::capture_content(
            &profiles,
            &MapContent(BTreeMap::new()),
            BTreeMap::new(),
        );

        assert!(
            content
                .read(&ManagedProfilePath::new("active.yaml").unwrap())
                .is_err()
        );
    }
}
