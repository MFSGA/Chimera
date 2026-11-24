//! Synchronous ScriptRunner adapter. Construct, use and drop it on the
//! blocking build worker, not a Tokio async worker.

use std::sync::Arc;

use chimera_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{
            PortError, ScriptExecutionError, ScriptOperation, ScriptRunner, StepLogEntry,
            StepLogLevel,
        },
        value::ConfigValue,
    },
};
#[cfg(feature = "lua")]
use mlua::{Lua, LuaOptions, LuaSerdeExt, StdLib};

use super::runner::{RunnerManager, ScriptRunRequest};
use crate::{
    extensions::{SandboxedExtensions, ScriptExtensionFactory},
    runner::ScriptRunOutput,
};
use chimera_profile_transform::LogSpan;

pub struct EnhanceScriptRunner {
    runtime: tokio::runtime::Runtime,
    extensions: Arc<dyn ScriptExtensionFactory>,
}

impl EnhanceScriptRunner {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_extensions(Arc::new(SandboxedExtensions))
    }

    pub fn with_extensions(extensions: Arc<dyn ScriptExtensionFactory>) -> anyhow::Result<Self> {
        Ok(Self {
            extensions,
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }
}

fn to_step_logs(logs: Vec<(LogSpan, String)>) -> Vec<StepLogEntry> {
    logs.into_iter()
        .map(|(span, message)| {
            let level = match span {
                LogSpan::Log => StepLogLevel::Log,
                LogSpan::Info => StepLogLevel::Info,
                LogSpan::Warn => StepLogLevel::Warn,
                LogSpan::Error => StepLogLevel::Error,
            };
            StepLogEntry::new(level, message)
        })
        .collect()
}

fn config_to_mapping(config: &ConfigValue) -> Result<serde_yaml::Mapping, PortError> {
    serde_yaml::to_value(config.to_json())
        .map_err(|error| format!("config to yaml: {error}").into())
        .and_then(|value| {
            value
                .as_mapping()
                .cloned()
                .ok_or_else(|| "config is not a mapping".into())
        })
}

fn mapping_to_config(mapping: serde_yaml::Mapping) -> Result<ConfigValue, PortError> {
    let text =
        serde_yaml::to_string(&mapping).map_err(|error| format!("mapping to yaml: {error}"))?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(text.as_str())
        .map_err(|error| format!("yaml to runtime value: {error}"))?;
    ConfigValue::try_from(value).map_err(|error| format!("yaml to config: {error:?}").into())
}

fn run_legacy_script(
    runtime: &tokio::runtime::Runtime,
    script_runtime: ScriptRuntime,
    source: &str,
    config: &ConfigValue,
    extensions: Arc<dyn ScriptExtensionFactory>,
) -> Result<ScriptRunOutput, PortError> {
    let mapping = config_to_mapping(config)?;
    runtime
        .block_on(async {
            RunnerManager::with_extensions(extensions)
                .run(
                    script_runtime,
                    ScriptRunRequest {
                        uid: "runtime".into(),
                        source: source.into(),
                        config: mapping,
                    },
                )
                .await
        })
        .map_err(|error| match error.downcast::<ScriptExecutionError>() {
            Ok(error) => Box::new(error) as PortError,
            Err(error) => error.to_string().into(),
        })
}

impl ScriptRunner for EnhanceScriptRunner {
    fn run(
        &self,
        runtime: ScriptRuntime,
        source: &str,
        config: &ConfigValue,
        logs: &mut Vec<StepLogEntry>,
    ) -> Result<ConfigValue, PortError> {
        let enabled = match runtime {
            ScriptRuntime::JavaScript => cfg!(feature = "javascript"),
            ScriptRuntime::Lua => cfg!(feature = "lua"),
        };
        if !enabled || !self.extensions.allows(runtime) {
            return Err(ScriptExecutionError::StartEngine {
                runtime,
                operation: ScriptOperation::Transform,
            }
            .into());
        }
        let output = run_legacy_script(
            &self.runtime,
            runtime,
            source,
            config,
            self.extensions.clone(),
        )?;
        logs.extend(to_step_logs(output.logs));
        mapping_to_config(output.config)
    }

    #[cfg(feature = "lua")]
    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        if !self.extensions.allows(ScriptRuntime::Lua) {
            return Err(ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::Lua,
                operation: ScriptOperation::ItemPredicate,
            }
            .into());
        }
        let lua = create_eval_lua()?;
        let value = lua
            .to_value(&item.to_json())
            .map_err(|error| format!("item to lua: {error}"))?;
        lua.globals()
            .set("item", value)
            .map_err(|error| format!("set item: {error}"))?;
        lua.load(expr)
            .set_name("=predicate")
            .eval::<bool>()
            .map_err(|error| format!("predicate eval: {error}").into())
    }

    #[cfg(feature = "lua")]
    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        if !self.extensions.allows(ScriptRuntime::Lua) {
            return Err(ScriptExecutionError::StartEngine {
                runtime: ScriptRuntime::Lua,
                operation: ScriptOperation::ItemExpression,
            }
            .into());
        }
        let lua = create_eval_lua()?;
        let value = lua
            .to_value(&item.to_json())
            .map_err(|error| format!("item to lua: {error}"))?;
        lua.globals()
            .set("item", value)
            .map_err(|error| format!("set item: {error}"))?;
        let value = lua
            .load(expr)
            .set_name("=expression")
            .eval::<mlua::Value>()
            .map_err(|error| format!("expr eval: {error}"))?;
        let json = lua
            .from_value::<serde_json::Value>(value)
            .map_err(|error| format!("lua to json: {error}"))?;
        ConfigValue::try_from(json).map_err(|error| format!("json to config: {error:?}").into())
    }
    #[cfg(not(feature = "lua"))]
    fn eval_item_predicate(&self, _: &str, _: &ConfigValue) -> Result<bool, PortError> {
        Err(ScriptExecutionError::StartEngine {
            runtime: ScriptRuntime::Lua,
            operation: ScriptOperation::ItemPredicate,
        }
        .into())
    }

    #[cfg(not(feature = "lua"))]
    fn eval_item_expr(&self, _: &str, _: &ConfigValue) -> Result<ConfigValue, PortError> {
        Err(ScriptExecutionError::StartEngine {
            runtime: ScriptRuntime::Lua,
            operation: ScriptOperation::ItemExpression,
        }
        .into())
    }
}

#[cfg(feature = "lua")]
fn create_eval_lua() -> Result<Lua, PortError> {
    Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|error| format!("lua context: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(yaml: &str) -> ConfigValue {
        let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(yaml).unwrap();
        ConfigValue::try_from(value).unwrap()
    }

    #[cfg(any(feature = "javascript", feature = "lua"))]
    fn to_yaml(config: &ConfigValue) -> serde_yaml_ng::Value {
        serde_yaml_ng::to_value(config.to_json()).unwrap()
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn runs_javascript_transform_and_captures_logs() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let script = r#"
function main(config) {
  console.log("hello from js");
  config["mode"] = "rule";
  return config;
}
"#;
        let mut logs = Vec::new();
        let result = runner
            .run(
                ScriptRuntime::JavaScript,
                script,
                &value("mixed-port: 7890\n"),
                &mut logs,
            )
            .expect("script should succeed");
        assert_eq!(to_yaml(&result)["mode"], serde_yaml_ng::Value::from("rule"));
        assert!(!logs.is_empty(), "console.log must surface as step log");
    }

    #[cfg(feature = "javascript")]
    #[test]
    fn failing_script_returns_error() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let result = runner.run(
            ScriptRuntime::JavaScript,
            "not valid js ][",
            &value("a: 1\n"),
            &mut Vec::new(),
        );
        assert!(result.is_err());
    }

    #[cfg(feature = "lua")]
    #[test]
    fn eval_item_errors_name_the_expression() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let item = value("name: test-node\n");
        let error = runner
            .eval_item_predicate("item.missing.field", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("predicate:1:"), "{error}");
        assert!(!error.contains(".rs:"), "{error}");
        let error = runner
            .eval_item_expr("item.missing.field", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expression:1:"), "{error}");
        assert!(!error.contains(".rs:"), "{error}");
    }

    #[cfg(feature = "lua")]
    #[test]
    fn eval_item_predicate_and_expr_use_lua_item_global() {
        let runner = EnhanceScriptRunner::new().unwrap();
        let item = value("name: test-node\ntype: ss\n");
        assert!(
            runner
                .eval_item_predicate(r#"item.name == "test-node""#, &item)
                .unwrap()
        );
        assert!(
            !runner
                .eval_item_predicate(r#"item.name == "other""#, &item)
                .unwrap()
        );
        let replaced = runner
            .eval_item_expr(
                r#"(function() item.name = "renamed"; return item end)()"#,
                &item,
            )
            .unwrap();
        assert_eq!(
            to_yaml(&replaced)["name"],
            serde_yaml_ng::Value::from("renamed")
        );
    }

    fn runner() -> EnhanceScriptRunner {
        EnhanceScriptRunner::new().unwrap()
    }

    fn assert_start_engine(error: PortError, runtime: ScriptRuntime, operation: ScriptOperation) {
        assert!(
            matches!(
                error.downcast_ref::<ScriptExecutionError>(),
                Some(ScriptExecutionError::StartEngine {
                    runtime: actual_runtime,
                    operation: actual_operation,
                }) if *actual_runtime == runtime && *actual_operation == operation
            ),
            "{error:?}"
        );
    }

    struct DecliningExtensions;

    impl ScriptExtensionFactory for DecliningExtensions {
        fn allows(&self, _: ScriptRuntime) -> bool {
            false
        }
    }

    #[test]
    fn declined_engine_returns_start_engine_error() {
        let runner = EnhanceScriptRunner::with_extensions(Arc::new(DecliningExtensions)).unwrap();
        for runtime in [ScriptRuntime::JavaScript, ScriptRuntime::Lua] {
            let mut logs = Vec::new();
            assert_start_engine(
                runner
                    .run(runtime, "invalid ][", &value("{}"), &mut logs)
                    .unwrap_err(),
                runtime,
                ScriptOperation::Transform,
            );
            assert!(logs.is_empty(), "a declined engine must not run a script");
        }
    }

    #[test]
    fn declined_lua_predicate_returns_start_engine_error() {
        let runner = EnhanceScriptRunner::with_extensions(Arc::new(DecliningExtensions)).unwrap();
        assert_start_engine(
            runner
                .eval_item_predicate("invalid ][", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemPredicate,
        );
    }

    #[test]
    fn declined_lua_expression_returns_start_engine_error() {
        let runner = EnhanceScriptRunner::with_extensions(Arc::new(DecliningExtensions)).unwrap();
        assert_start_engine(
            runner
                .eval_item_expr("invalid ][", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemExpression,
        );
    }

    #[cfg(not(feature = "javascript"))]
    #[test]
    fn javascript_disabled_returns_start_engine_error() {
        // A non-mapping input would fail conversion if capability admission ran too late.
        let runner = runner();
        let error = runner
            .run(
                ScriptRuntime::JavaScript,
                "invalid",
                &value("[]"),
                &mut vec![],
            )
            .unwrap_err();
        assert_start_engine(error, ScriptRuntime::JavaScript, ScriptOperation::Transform);
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_returns_start_engine_error() {
        let runner = runner();
        let error = runner
            .run(ScriptRuntime::Lua, "invalid", &value("[]"), &mut vec![])
            .unwrap_err();
        assert_start_engine(error, ScriptRuntime::Lua, ScriptOperation::Transform);
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_predicate_returns_start_engine_error() {
        let runner = runner();
        assert_start_engine(
            runner
                .eval_item_predicate("invalid", &value("[]"))
                .unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemPredicate,
        );
    }

    #[cfg(not(feature = "lua"))]
    #[test]
    fn lua_disabled_expression_returns_start_engine_error() {
        let runner = runner();
        assert_start_engine(
            runner.eval_item_expr("invalid", &value("[]")).unwrap_err(),
            ScriptRuntime::Lua,
            ScriptOperation::ItemExpression,
        );
    }

    #[cfg(any(feature = "javascript", feature = "lua"))]
    #[test]
    fn enabled_engine_errors_are_not_misclassified_as_unavailable() {
        let runner = runner();
        for runtime in [ScriptRuntime::JavaScript, ScriptRuntime::Lua] {
            let enabled = match runtime {
                ScriptRuntime::JavaScript => cfg!(feature = "javascript"),
                ScriptRuntime::Lua => cfg!(feature = "lua"),
            };
            if enabled {
                let error = runner
                    .run(runtime, "invalid ][", &value("{}"), &mut vec![])
                    .unwrap_err();
                assert!(
                    error.downcast_ref::<ScriptExecutionError>().is_none(),
                    "{error:?}"
                );
            }
        }
        #[cfg(feature = "lua")]
        {
            assert!(
                runner
                    .eval_item_predicate("invalid ][", &value("{}"))
                    .unwrap_err()
                    .downcast_ref::<ScriptExecutionError>()
                    .is_none()
            );
            assert!(
                runner
                    .eval_item_expr("invalid ][", &value("{}"))
                    .unwrap_err()
                    .downcast_ref::<ScriptExecutionError>()
                    .is_none()
            );
        }
    }
}
