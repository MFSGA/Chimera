//! Fixed source inputs and content for one runtime target and build.
use super::impact;
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::ClashConfig,
    profile::{ManagedProfilePath, Profiles},
    runtime::executor::{PortError, ProfileContentSource},
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, Default)]
pub(in crate::client) struct FrozenProfileContent(pub BTreeMap<String, Result<String, String>>);

impl FrozenProfileContent {
    pub fn capture_content(
        profiles: &Profiles,
        source: &dyn ProfileContentSource,
        staged_content: BTreeMap<String, String>,
    ) -> Self {
        let mut content = Self::default();
        for uid in crate::state::profiles::ProfilesActor::current_closure(profiles) {
            if let Some(source_path) = profiles
                .items
                .get(&uid)
                .and_then(|item| item.definition.source())
            {
                let path = &source_path.materialized().file;
                content.0.insert(
                    path.to_string(),
                    source.read(path).map_err(|error| error.to_string()),
                );
            }
        }
        for (path, staged) in staged_content {
            if let Some(captured) = content.0.get_mut(&path) {
                *captured = Ok(staged);
            }
        }
        content
    }
}

impl ProfileContentSource for FrozenProfileContent {
    fn read(&self, path: &ManagedProfilePath) -> Result<String, PortError> {
        self.0
            .get(&path.to_string())
            .cloned()
            .map(|result| result.map_err(Into::into))
            .ok_or_else(|| -> PortError {
                format!("profile content was not captured: {path}").into()
            })?
    }
}

#[derive(Debug, Clone)]
pub(in crate::client) struct RuntimeInputs {
    pub app: ChimeraAppConfig,
    pub clash: ClashConfig,
    pub profiles: Arc<Profiles>,
    pub content: FrozenProfileContent,
}

impl RuntimeInputs {
    pub fn target_key(&self) -> anyhow::Result<String> {
        let projections = (
            impact::application_target(&self.app).ok_or_else(|| {
                anyhow::anyhow!("application runtime identity cannot be serialized")
            })?,
            impact::clash_target(&self.clash)
                .ok_or_else(|| anyhow::anyhow!("clash runtime identity cannot be serialized"))?,
            impact::profiles_target(&self.profiles)
                .ok_or_else(|| anyhow::anyhow!("profiles runtime identity cannot be serialized"))?,
            &self.content.0,
        );
        Ok(crate::state::profiles::payload_digest(&serde_json::to_vec(
            &projections,
        )?))
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
            .insert("profiles/active.yaml".into(), Ok("mode: rule".into()));
        let content_changed = inputs.target_key().unwrap();
        assert_ne!(content_changed, clash_changed);

        inputs
            .content
            .0
            .insert("profiles/active.yaml".into(), Ok("mode: global".into()));
        assert_ne!(inputs.target_key().unwrap(), content_changed);
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
