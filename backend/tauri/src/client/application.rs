//! Application configuration client boundary.
//!
//! REF owns application configuration through a typed actor-backed client.
//! Chimera still persists the combined legacy `IVerge` model, so this
//! transitional client keeps the same persistence and side-effect ordering
//! while moving ownership and mutation serialization behind the ref-style
//! application boundary.

use crate::config::{chimera::IVerge, core::Config};

use super::ChimeraClient;

#[derive(Default)]
pub(crate) struct ApplicationClient {
    patch_gate: tokio::sync::Mutex<()>,
}

impl ApplicationClient {
    pub(crate) fn legacy() -> Self {
        Self::default()
    }

    pub(crate) fn get(&self) -> IVerge {
        Config::verge().latest().clone()
    }

    async fn patch(&self, owner: &ChimeraClient, patch: IVerge) -> anyhow::Result<()> {
        let _guard = self.patch_gate.lock().await;
        crate::feat::patch_verge_uncoordinated(owner, patch).await
    }
}

impl ChimeraClient {
    pub(crate) fn application_config(&self) -> IVerge {
        self.inner.application.get()
    }

    pub(crate) async fn patch_verge(&self, patch: IVerge) -> anyhow::Result<()> {
        self.inner.application.patch(self, patch).await
    }
}
