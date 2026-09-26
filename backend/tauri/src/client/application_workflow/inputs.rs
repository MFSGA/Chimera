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
}
