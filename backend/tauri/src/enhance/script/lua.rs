use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use mlua::{
    HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value as LuaValue, Variadic,
    VmState, chunk::ChunkMode,
};

use crate::enhance::{
    chain::{LogSpan, Logs},
    script::runner::{ScriptRunOutput, ScriptRunRequest, ScriptRunner},
};

const LUA_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const LUA_HOOK_INTERVAL: u32 = 10_000;
const LUA_INSTRUCTION_LIMIT: u64 = 2_000_000;
const LUA_WALL_TIME_LIMIT: Duration = Duration::from_millis(750);

#[derive(Debug, Default)]
pub(crate) struct LuaRunner;

impl LuaRunner {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ScriptRunner for LuaRunner {
    async fn run(&self, request: ScriptRunRequest) -> Result<ScriptRunOutput> {
        let uid = request.uid.clone();
        tokio::task::spawn_blocking(move || execute(request))
            .await
            .with_context(|| format!("lua transform {uid} runtime task failed"))?
    }
}

fn execute(request: ScriptRunRequest) -> Result<ScriptRunOutput> {
    let lua = create_sandbox(&request.uid)?;
    let environment = create_environment(&lua, &request.uid)?;
    let logs = install_log_functions(&lua, &environment)?;
    install_limits(&lua, &request.uid)?;

    let config = lua_result(
        lua.to_value(&request.config),
        format!("failed to encode config for lua transform {}", request.uid),
    )?;
    lua_result(
        environment.set("config", config),
        format!("failed to expose config to lua transform {}", request.uid),
    )?;

    let value: LuaValue = lua_result(
        lua.load(&request.source)
            .set_name(format!("@profile/{}", request.uid))
            .set_mode(ChunkMode::Text)
            .set_environment(environment)
            .eval(),
        format!("lua transform {} failed", request.uid),
    )?;

    let config = lua_result(
        lua.from_value(value),
        format!("lua transform {} must return a config mapping", request.uid),
    )?;
    let logs = logs
        .lock()
        .map_err(|_| anyhow::anyhow!("lua transform {} log sink is poisoned", request.uid))?
        .clone();

    Ok(ScriptRunOutput { config, logs })
}

fn create_sandbox(uid: &str) -> Result<Lua> {
    let libraries = StdLib::TABLE | StdLib::STRING | StdLib::UTF8 | StdLib::MATH;
    let lua = lua_result(
        Lua::new_with(libraries, LuaOptions::default()),
        format!("failed to create lua sandbox for transform {uid}"),
    )?;
    lua_result(
        lua.set_memory_limit(LUA_MEMORY_LIMIT_BYTES),
        format!("failed to set lua memory limit for transform {uid}"),
    )?;
    Ok(lua)
}

fn create_environment(lua: &Lua, uid: &str) -> Result<Table> {
    let globals = lua.globals();
    let environment = lua_result(
        lua.create_table(),
        format!("failed to create lua environment for transform {uid}"),
    )?;

    for name in [
        "assert", "error", "ipairs", "next", "pairs", "select", "tonumber", "tostring", "type",
    ] {
        let value: LuaValue = lua_result(
            globals.get(name),
            format!("failed to resolve lua builtin `{name}` for transform {uid}"),
        )?;
        lua_result(
            environment.set(name, value),
            format!("failed to install lua builtin `{name}` for transform {uid}"),
        )?;
    }

    for name in ["math", "string", "table", "utf8"] {
        let value: LuaValue = lua_result(
            globals.get(name),
            format!("failed to resolve lua library `{name}` for transform {uid}"),
        )?;
        lua_result(
            environment.set(name, value),
            format!("failed to install lua library `{name}` for transform {uid}"),
        )?;
    }

    lua_result(
        environment.set("_G", environment.clone()),
        format!("failed to finalize lua environment for transform {uid}"),
    )?;
    Ok(environment)
}

fn install_limits(lua: &Lua, uid: &str) -> Result<()> {
    let started = Instant::now();
    let instructions = Arc::new(AtomicU64::new(0));
    let transform_uid = uid.to_string();
    let hook_uid = transform_uid.clone();
    lua_result(
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(LUA_HOOK_INTERVAL),
            move |_lua, _debug| {
            let executed = instructions.fetch_add(
                u64::from(LUA_HOOK_INTERVAL),
                Ordering::Relaxed,
            ) + u64::from(LUA_HOOK_INTERVAL);
            if executed > LUA_INSTRUCTION_LIMIT {
                return Err(mlua::Error::RuntimeError(format!(
                    "lua transform {hook_uid} exceeded the instruction limit ({LUA_INSTRUCTION_LIMIT})"
                )));
            }
            if started.elapsed() > LUA_WALL_TIME_LIMIT {
                return Err(mlua::Error::RuntimeError(format!(
                    "lua transform {hook_uid} exceeded the execution time limit ({} ms)",
                    LUA_WALL_TIME_LIMIT.as_millis()
                )));
            }
                Ok(VmState::Continue)
            },
        ),
        format!("failed to install lua execution limits for transform {transform_uid}"),
    )?;
    Ok(())
}

fn install_log_functions(lua: &Lua, environment: &Table) -> Result<Arc<Mutex<Logs>>> {
    let logs = Arc::new(Mutex::new(Vec::new()));
    for (name, span) in [
        ("print", LogSpan::Log),
        ("log", LogSpan::Log),
        ("info", LogSpan::Info),
        ("warn", LogSpan::Warn),
        ("error_log", LogSpan::Error),
    ] {
        let sink = logs.clone();
        let callback = lua_result(
            lua.create_function(move |_lua, values: Variadic<LuaValue>| {
                let message = values
                    .iter()
                    .map(LuaValue::to_string)
                    .collect::<mlua::Result<Vec<_>>>()?
                    .join("\t");
                sink.lock()
                    .map_err(|_| mlua::Error::RuntimeError("lua log sink is poisoned".into()))?
                    .push((span, message));
                Ok(())
            }),
            format!("failed to create lua {name} logger"),
        )?;
        lua_result(
            environment.set(name, callback),
            format!("failed to install lua {name} logger"),
        )?;
    }

    let console = lua_result(
        lua.create_table(),
        "failed to create lua console logger".into(),
    )?;
    for (name, span) in [
        ("log", LogSpan::Log),
        ("info", LogSpan::Info),
        ("warn", LogSpan::Warn),
        ("error", LogSpan::Error),
    ] {
        let sink = logs.clone();
        let callback = lua_result(
            lua.create_function(move |_lua, values: Variadic<LuaValue>| {
                let message = values
                    .iter()
                    .map(LuaValue::to_string)
                    .collect::<mlua::Result<Vec<_>>>()?
                    .join("\t");
                sink.lock()
                    .map_err(|_| mlua::Error::RuntimeError("lua log sink is poisoned".into()))?
                    .push((span, message));
                Ok(())
            }),
            format!("failed to create lua console.{name} logger"),
        )?;
        lua_result(
            console.set(name, callback),
            format!("failed to install lua console.{name} logger"),
        )?;
    }
    lua_result(
        environment.set("console", console),
        "failed to install lua console logger".into(),
    )?;
    Ok(logs)
}

fn lua_result<T>(result: mlua::Result<T>, context: String) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use serde_yaml::Mapping;

    use super::*;

    fn mapping(source: &str) -> Mapping {
        serde_yaml::from_str(source).unwrap()
    }

    fn request(source: &str, config: Mapping) -> ScriptRunRequest {
        ScriptRunRequest {
            uid: "sl-test".into(),
            source: source.into(),
            config,
        }
    }

    #[tokio::test]
    async fn lua_runner_returns_and_mutates_config() {
        let output = LuaRunner::new()
            .run(request(
                r#"
config["unified-delay"] = true
config.dns = config.dns or {}
config.dns.enable = true
return config
"#,
                mapping("unified-delay: false\n"),
            ))
            .await
            .unwrap();

        assert_eq!(
            output
                .config
                .get("unified-delay")
                .and_then(serde_yaml::Value::as_bool),
            Some(true)
        );
        let dns = output.config.get("dns").unwrap().as_mapping().unwrap();
        assert_eq!(
            dns.get("enable").and_then(serde_yaml::Value::as_bool),
            Some(true)
        );
    }

    #[tokio::test]
    async fn lua_runner_round_trips_nested_sequences() {
        let config = mapping(
            r#"
proxies:
  - name: node-a
    type: socks5
    server: 127.0.0.1
    port: 1080
rules:
  - MATCH,DIRECT
"#,
        );
        let output = LuaRunner::new()
            .run(request("return config", config.clone()))
            .await
            .unwrap();

        assert_eq!(output.config, config);
    }

    #[tokio::test]
    async fn lua_runner_captures_logs() {
        let output = LuaRunner::new()
            .run(request(
                r#"
print("hello", 7)
warn("careful")
return config
"#,
                Mapping::new(),
            ))
            .await
            .unwrap();

        assert_eq!(
            output.logs,
            vec![
                (LogSpan::Log, "hello\t7".into()),
                (LogSpan::Warn, "careful".into()),
            ]
        );
    }

    #[tokio::test]
    async fn lua_runner_exposes_no_io_os_or_package_library() {
        LuaRunner::new()
            .run(request(
                r#"
assert(io == nil)
assert(os == nil)
assert(package == nil)
assert(debug == nil)
assert(load == nil)
assert(loadfile == nil)
assert(dofile == nil)
assert(pcall == nil)
assert(xpcall == nil)
assert(coroutine == nil)
return config
"#,
                Mapping::new(),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn lua_runner_rejects_non_mapping_results() {
        let error = LuaRunner::new()
            .run(request("return 42", Mapping::new()))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("must return a config mapping"));
    }

    #[tokio::test]
    async fn lua_runner_interrupts_infinite_loops() {
        let error = LuaRunner::new()
            .run(request("while true do end", Mapping::new()))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("instruction limit") || error.contains("execution time limit"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn lua_runner_enforces_memory_limit() {
        let error = LuaRunner::new()
            .run(request(
                r#"
local chunks = {}
for i = 1, 1000000 do
  chunks[i] = string.rep("x", 1024)
end
return config
"#,
                Mapping::new(),
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.to_ascii_lowercase().contains("memory"),
            "unexpected error: {error}"
        );
    }
}
