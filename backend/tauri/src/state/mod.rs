#[derive(Debug, Default)]
pub(crate) struct TypedConfigPatchPlan {
    pub application: Option<chimera_config::application::ChimeraAppConfigPatch>,
    pub session_state: Option<chimera_config::state::PersistentStatePatch>,
    pub clash_config: Option<chimera_config::clash::config::ClashConfigPatch>,
}

#[derive(Debug)]
pub(crate) enum ConditionalReplaceResult<T> {
    Replaced(T),
    Conflict { actual_version: u64 },
}

pub mod application;
pub mod clash_config;
pub mod mirror;
pub mod session_state;
