//! Temporary fail-closed seam for the reference domain actors.
//!
//! The reference `MutationCoordinator` is connected by the application
//! workflow at composition-root startup. Chimera's workflow actor and setup
//! wiring have not been migrated yet, so profile mutations must remain
//! rejected until that complete call path is available.

use chimera_core::state::{DecisionHandle, StateParticipant};

use crate::client::application_workflow::{impact::MutationHints, policy::CommandClass};

#[derive(Clone, Default)]
pub(crate) struct MutationCoordinator;

impl MutationCoordinator {
    pub fn ensure_ready(&self) -> anyhow::Result<()> {
        anyhow::bail!("profile writes are disabled until the application workflow is connected")
    }

    pub fn participant<T>(
        &self,
        _operation_id: String,
        _hints: MutationHints,
        _class: CommandClass,
    ) -> anyhow::Result<impl FnOnce(DecisionHandle) -> StateParticipant<T>>
    where
        T: Clone + Send + Sync + 'static,
    {
        // This result always fails before PersistentStateManager begins a
        // transaction. The closure only supplies the opaque return type until
        // the real workflow participant is migrated.
        let unreachable_participant = |_decision: DecisionHandle| -> StateParticipant<T> {
            unreachable!("a fail-closed profile participant cannot be invoked")
        };
        anyhow::bail!("profile writes are disabled until the application workflow is connected");
        #[allow(unreachable_code)]
        Ok(unreachable_participant)
    }

    pub async fn finish(
        &self,
        _operation_id: String,
        _domain: &str,
        _source_version: u64,
    ) -> (
        crate::client::runtime::CommitReceipt,
        Vec<crate::client::runtime::Degradation>,
    ) {
        todo!("migrate and connect the reference application workflow before enabling writes")
    }
}
