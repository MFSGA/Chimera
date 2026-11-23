//! Runtime candidate content port, adapted from ref's reconciler/ports.rs.
//!
//! The upstream RuntimeBuildPort also owns candidate build, core-spec
//! resolution and publication. Chimera currently routes those through the
//! existing CoreLifecyclePort and runtime builder. Migrate their ownership
//! only when the complete prepared candidate can be handed to the single
//! CoreLifecycle executor without a second apply path.
use std::collections::BTreeMap;

use chimera_config::{profile::Profiles, runtime::executor::ProfileContentSource};

use super::inputs::FrozenProfileContent;

/// Capture the active managed-file dependency closure from one supplied
/// content source. A staged candidate may replace bytes it explicitly owns.
/// File reads are fenced by RuntimePreparation, not by this port.
#[async_trait::async_trait]
pub(in crate::client) trait RuntimeContentPort: Send + Sync {
    async fn capture_content(
        &self,
        profiles: &Profiles,
        staged_content: BTreeMap<String, String>,
    ) -> FrozenProfileContent;
}

#[async_trait::async_trait]
impl<T> RuntimeContentPort for T
where
    T: ProfileContentSource + Send + Sync,
{
    async fn capture_content(
        &self,
        profiles: &Profiles,
        staged_content: BTreeMap<String, String>,
    ) -> FrozenProfileContent {
        FrozenProfileContent::capture_content(profiles, self, staged_content)
    }
}
