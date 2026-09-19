use std::sync::Arc;

use crate::config::chimera::ClashCore;

use super::ports::CoreLifecyclePort;

pub(super) enum Command {
    StopCore,
    SelectCore(ClashCore),
}

/// Serialized lifecycle command workflow.
///
/// The lower-level port remains the compatibility boundary for the legacy
/// CoreManager while command admission moves toward REF's actor-owned
/// core-lifecycle model.
pub(super) struct CoreLifecycleWorkflow {
    core: Arc<dyn CoreLifecyclePort>,
}

impl CoreLifecycleWorkflow {
    pub(super) fn new(core: Arc<dyn CoreLifecyclePort>) -> Self {
        Self { core }
    }

    pub(super) async fn execute(&self, command: Command) -> anyhow::Result<()> {
        let mut lease = self.core.begin().await?;
        match command {
            Command::StopCore => lease.stop().await,
            Command::SelectCore(core) => lease.change_core(core).await,
        }
    }
}
