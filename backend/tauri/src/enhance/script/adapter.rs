//! Synchronous adapter from the legacy script runners to the ref runtime
//! executor's ScriptRunner port.

use chimera_config::{
    profile::ScriptRuntime,
    runtime::{
        executor::{PortError, ScriptRunOutcome, ScriptRunner, StepLogEntry, StepLogLevel},
        value::ConfigValue,
    },
};
use mlua::{Lua, LuaOptions, LuaSerdeExt, StdLib};

use super::runner::{RunnerManager, ScriptRunRequest};
use crate::enhance::chain::LogSpan;
use crate::enhance::script::runner::ScriptRunOutput;

pub struct EnhanceScriptRunner {
    runtime: tokio::runtime::Runtime,
}

impl EnhanceScriptRunner {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
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
) -> Result<ScriptRunOutput, PortError> {
    let script_type = match script_runtime {
        ScriptRuntime::JavaScript => crate::config::profile::item_type::ScriptType::JavaScript,
        ScriptRuntime::Lua => crate::config::profile::item_type::ScriptType::Lua,
    };
    let mapping = config_to_mapping(config)?;
    runtime
        .block_on(async {
            RunnerManager::new()
                .run(
                    script_type,
                    ScriptRunRequest {
                        uid: "runtime".into(),
                        source: source.into(),
                        config: mapping,
                    },
                )
                .await
        })
        .map_err(|error| error.to_string().into())
}

impl ScriptRunner for EnhanceScriptRunner {
    fn run(&self, runtime: ScriptRuntime, source: &str, config: &ConfigValue) -> ScriptRunOutcome {
        match run_legacy_script(&self.runtime, runtime, source, config) {
            Ok(output) => ScriptRunOutcome {
                result: mapping_to_config(output.config),
                logs: to_step_logs(output.logs),
            },
            Err(error) => ScriptRunOutcome {
                result: Err(error),
                logs: Vec::new(),
            },
        }
    }

    fn eval_item_predicate(&self, expr: &str, item: &ConfigValue) -> Result<bool, PortError> {
        let lua = create_eval_lua()?;
        let value = lua
            .to_value(&item.to_json())
            .map_err(|error| format!("item to lua: {error}"))?;
        lua.globals()
            .set("item", value)
            .map_err(|error| format!("set item: {error}"))?;
        lua.load(expr)
            .eval::<bool>()
            .map_err(|error| format!("predicate eval: {error}").into())
    }

    fn eval_item_expr(&self, expr: &str, item: &ConfigValue) -> Result<ConfigValue, PortError> {
        let lua = create_eval_lua()?;
        let value = lua
            .to_value(&item.to_json())
            .map_err(|error| format!("item to lua: {error}"))?;
        lua.globals()
            .set("item", value)
            .map_err(|error| format!("set item: {error}"))?;
        let value = lua
            .load(expr)
            .eval::<mlua::Value>()
            .map_err(|error| format!("expr eval: {error}"))?;
        let json = lua
            .from_value::<serde_json::Value>(value)
            .map_err(|error| format!("lua to json: {error}"))?;
        ConfigValue::try_from(json).map_err(|error| format!("json to config: {error:?}").into())
    }
}

fn create_eval_lua() -> Result<Lua, PortError> {
    Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH,
        LuaOptions::default(),
    )
    .map_err(|error| format!("lua context: {error}").into())
}
