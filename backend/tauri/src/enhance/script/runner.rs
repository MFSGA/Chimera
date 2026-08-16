use std::sync::Arc;

use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_yaml::Mapping;

use crate::{
    config::profile::item_type::{ProfileUid, ScriptType},
    enhance::{chain::Logs, script::lua::LuaRunner},
};

#[derive(Debug, Clone)]
pub struct ScriptRunRequest {
    pub uid: ProfileUid,
    pub source: String,
    pub config: Mapping,
}

#[derive(Debug, Clone, Default)]
pub struct ScriptRunOutput {
    pub config: Mapping,
    pub logs: Logs,
}

#[async_trait]
pub trait ScriptRunner: Send + Sync {
    async fn run(&self, request: ScriptRunRequest) -> Result<ScriptRunOutput>;
}

pub struct RunnerManager {
    javascript: Option<Arc<dyn ScriptRunner>>,
    lua: Option<Arc<dyn ScriptRunner>>,
}

impl Default for RunnerManager {
    fn default() -> Self {
        Self {
            javascript: None,
            lua: Some(Arc::new(LuaRunner::new())),
        }
    }
}

impl RunnerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn run(
        &self,
        script_type: ScriptType,
        request: ScriptRunRequest,
    ) -> Result<ScriptRunOutput> {
        let runner = match script_type {
            ScriptType::JavaScript => self.javascript.as_ref(),
            ScriptType::Lua => self.lua.as_ref(),
        };
        let Some(runner) = runner else {
            bail!(
                "script runtime for {script_type:?} is unavailable for transform {}",
                request.uid
            );
        };
        runner.run(request).await
    }

    #[cfg(test)]
    pub(crate) fn with_runner(
        mut self,
        script_type: ScriptType,
        runner: Arc<dyn ScriptRunner>,
    ) -> Self {
        match script_type {
            ScriptType::JavaScript => self.javascript = Some(runner),
            ScriptType::Lua => self.lua = Some(runner),
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_runtime_fails_closed_with_transform_identity() {
        let error = RunnerManager::new()
            .run(
                ScriptType::JavaScript,
                ScriptRunRequest {
                    uid: "sj-missing".into(),
                    source: "export default (config) => config".into(),
                    config: Mapping::new(),
                },
            )
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("sj-missing"));
        assert!(error.contains("JavaScript"));
        assert!(error.contains("unavailable"));
    }
}
