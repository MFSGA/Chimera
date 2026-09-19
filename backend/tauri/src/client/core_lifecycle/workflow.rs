use std::sync::Arc;

use crate::config::chimera::ClashCore;

use super::ports::CoreLifecyclePort;

pub(super) enum Command {
    StopCore,
    SelectCore(ClashCore),
    RecoverCore,
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
        match command {
            Command::RecoverCore => self.core.recover().await,
            Command::StopCore => {
                let mut lease = self.core.begin().await?;
                lease.stop().await
            }
            Command::SelectCore(core) => {
                let mut lease = self.core.begin().await?;
                lease.change_core(core).await
            }
        }
    }
}
