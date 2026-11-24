//! Legacy ScriptType adapter to the shared sandboxed engines.
use crate::config::profile::item_type::ScriptType;
use chimera_config::profile::ScriptRuntime;
pub use chimera_scripts::runner::ScriptRunRequest;
#[cfg(test)]
pub use chimera_scripts::runner::{ScriptRunOutput, ScriptRunner};

pub struct RunnerManager(chimera_scripts::runner::RunnerManager);

fn runtime(kind: ScriptType) -> ScriptRuntime {
    match kind {
        ScriptType::JavaScript => ScriptRuntime::JavaScript,
        ScriptType::Lua => ScriptRuntime::Lua,
    }
}

impl RunnerManager {
    pub fn new() -> Self {
        Self(chimera_scripts::runner::RunnerManager::new())
    }

    pub async fn run(
        &self,
        kind: ScriptType,
        request: ScriptRunRequest,
    ) -> anyhow::Result<chimera_scripts::runner::ScriptRunOutput> {
        self.0.run(runtime(kind), request).await
    }

    #[cfg(test)]
    pub(crate) fn with_runner(
        self,
        kind: ScriptType,
        runner: std::sync::Arc<dyn ScriptRunner>,
    ) -> Self {
        Self(self.0.with_runner(runtime(kind), runner))
    }
}
