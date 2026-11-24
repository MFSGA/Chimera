use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_yaml::Mapping;

use crate::extensions::{SandboxedExtensions, ScriptExtensionFactory};
use chimera_config::{
    profile::ScriptRuntime,
    runtime::executor::{ScriptExecutionError, ScriptOperation},
};
use chimera_profile_transform::Logs;

#[derive(Debug, Clone)]
pub struct ScriptRunRequest {
    pub uid: String,
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
    extensions: Arc<dyn ScriptExtensionFactory>,
    javascript: std::sync::OnceLock<Arc<dyn ScriptRunner>>,
    lua: std::sync::OnceLock<Arc<dyn ScriptRunner>>,
}

impl Default for RunnerManager {
    fn default() -> Self {
        Self::with_extensions(Arc::new(SandboxedExtensions))
    }
}

impl RunnerManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_extensions(extensions: Arc<dyn ScriptExtensionFactory>) -> Self {
        Self {
            extensions,
            javascript: Default::default(),
            lua: Default::default(),
        }
    }

    // Like ref, construct an engine adapter only after admission.
    fn get_or_init_runner(&self, runtime: ScriptRuntime) -> Result<&dyn ScriptRunner> {
        if !self.extensions.allows(runtime) {
            return Err(ScriptExecutionError::StartEngine {
                runtime,
                operation: ScriptOperation::Transform,
            }
            .into());
        }
        match runtime {
            ScriptRuntime::JavaScript => {
                if let Some(runner) = self.javascript.get() {
                    return Ok(runner.as_ref());
                }
                #[cfg(feature = "javascript")]
                {
                    Ok(self
                        .javascript
                        .get_or_init(|| Arc::new(crate::js::JSRunner::new()))
                        .as_ref())
                }
                #[cfg(not(feature = "javascript"))]
                {
                    Err(ScriptExecutionError::StartEngine {
                        runtime,
                        operation: ScriptOperation::Transform,
                    }
                    .into())
                }
            }
            ScriptRuntime::Lua => {
                if let Some(runner) = self.lua.get() {
                    return Ok(runner.as_ref());
                }
                #[cfg(feature = "lua")]
                {
                    Ok(self
                        .lua
                        .get_or_init(|| Arc::new(crate::lua::LuaRunner::new()))
                        .as_ref())
                }
                #[cfg(not(feature = "lua"))]
                {
                    Err(ScriptExecutionError::StartEngine {
                        runtime,
                        operation: ScriptOperation::Transform,
                    }
                    .into())
                }
            }
        }
    }

    pub async fn run(
        &self,
        runtime: ScriptRuntime,
        request: ScriptRunRequest,
    ) -> Result<ScriptRunOutput> {
        let runner = self.get_or_init_runner(runtime)?;
        runner.run(request).await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_runner(self, runtime: ScriptRuntime, runner: Arc<dyn ScriptRunner>) -> Self {
        let target = match runtime {
            ScriptRuntime::JavaScript => &self.javascript,
            ScriptRuntime::Lua => &self.lua,
        };
        assert!(
            target.set(runner).is_ok(),
            "test runner already initialized"
        );
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Denied;
    impl ScriptExtensionFactory for Denied {
        fn allows(&self, _: ScriptRuntime) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn denied_runtime_fails_with_typed_error() {
        let manager = RunnerManager::with_extensions(Arc::new(Denied));
        let error = manager
            .run(
                ScriptRuntime::JavaScript,
                ScriptRunRequest {
                    uid: "sj-missing".into(),
                    source: "export default (config) => config".into(),
                    config: Mapping::new(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ScriptExecutionError>(),
            Some(ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::JavaScript,
                operation: ScriptOperation::Transform,
            })
        ));
    }
}
