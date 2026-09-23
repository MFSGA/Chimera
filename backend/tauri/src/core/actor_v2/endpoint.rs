//! Canonical lower-host status projection.

use chimera_ipc::api::status::CoreState;

use crate::core::clash::core::RunType;

#[derive(Debug, Clone)]
pub(crate) struct CoreStatusSnapshot {
    pub(crate) state: CoreState,
    pub(crate) state_changed_at: i64,
    pub(crate) run_type: RunType,
}
