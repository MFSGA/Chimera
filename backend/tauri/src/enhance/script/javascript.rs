use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use parking_lot::Mutex;
use rquickjs::{Context, Ctx, Function, Module, Runtime, Value};
use serde_yaml::Mapping;

use crate::enhance::{
    chain::{LogSpan, Logs, push_script_log},
    script::runner::{ScriptRunOutput, ScriptRunRequest, ScriptRunner},
};

const JS_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const JS_MAX_STACK_BYTES: usize = 512 * 1024;
const JS_INTERRUPT_CALLBACK_LIMIT: u64 = 20_000;
const JS_WALL_TIME_LIMIT: Duration = Duration::from_millis(750);

const LIMIT_NONE: u8 = 0;
const LIMIT_CALLBACKS: u8 = 1;
const LIMIT_WALL_TIME: u8 = 2;

const LOGGER_BOOTSTRAP: &str = r#"
const __chimeraFormatLog = (args) => args.map((value) => String(value)).join("\t");
globalThis.console = Object.freeze({
  log: (...args) => __chimeraLog("log", __chimeraFormatLog(args)),
  info: (...args) => __chimeraLog("info", __chimeraFormatLog(args)),
  warn: (...args) => __chimeraLog("warn", __chimeraFormatLog(args)),
  error: (...args) => __chimeraLog("error", __chimeraFormatLog(args)),
});
globalThis.print = console.log;
globalThis.log = console.log;
globalThis.info = console.info;
globalThis.warn = console.warn;
globalThis.error_log = console.error;
"#;

#[derive(Debug, Default)]
pub(crate) struct JavaScriptRunner;

impl JavaScriptRunner {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ScriptRunner for JavaScriptRunner {
    async fn run(&self, request: ScriptRunRequest) -> Result<ScriptRunOutput> {
        let uid = request.uid.clone();
        tokio::task::spawn_blocking(move || execute(request))
            .await
            .with_context(|| format!("javascript transform {uid} runtime task failed"))?
    }
}

fn execute(request: ScriptRunRequest) -> Result<ScriptRunOutput> {
    let runtime = js_result(
        Runtime::new(),
        format!(
            "failed to create javascript runtime for transform {}",
            request.uid
        ),
    )?;
    runtime.set_memory_limit(JS_MEMORY_LIMIT_BYTES);
    runtime.set_max_stack_size(JS_MAX_STACK_BYTES);

    let limit_reason = install_limits(&runtime);
    let context = js_result(
        Context::full(&runtime),
        format!(
            "failed to create javascript context for transform {}",
            request.uid
        ),
    )?;
    let logs = Arc::new(Mutex::new(Vec::new()));
    let config_json = serde_json::to_string(&request.config).with_context(|| {
        format!(
            "failed to encode config for javascript transform {}",
            request.uid
        )
    })?;

    let result = context.with(|ctx| execute_module(ctx, &request, &config_json, logs.clone()));
    let result_json = match result {
        Ok(result) => result,
        Err(error) => match limit_reason.load(Ordering::Relaxed) {
            LIMIT_CALLBACKS => bail!(
                "javascript transform {} exceeded the execution callback budget ({JS_INTERRUPT_CALLBACK_LIMIT})",
                request.uid
            ),
            LIMIT_WALL_TIME => bail!(
                "javascript transform {} exceeded the execution time limit ({} ms)",
                request.uid,
                JS_WALL_TIME_LIMIT.as_millis()
            ),
            _ => return Err(error),
        },
    };

    let config: Mapping = serde_json::from_str(&result_json).with_context(|| {
        format!(
            "javascript transform {} must return a JSON-serializable config mapping",
            request.uid
        )
    })?;
    let logs = logs.lock().clone();

    Ok(ScriptRunOutput { config, logs })
}

fn install_limits(runtime: &Runtime) -> Arc<AtomicU8> {
    let started = Instant::now();
    let reason = Arc::new(AtomicU8::new(LIMIT_NONE));
    let hook_reason = reason.clone();
    let mut callbacks = 0_u64;

    runtime.set_interrupt_handler(Some(Box::new(move || {
        callbacks += 1;
        if callbacks > JS_INTERRUPT_CALLBACK_LIMIT {
            hook_reason.store(LIMIT_CALLBACKS, Ordering::Relaxed);
            return true;
        }
        if started.elapsed() > JS_WALL_TIME_LIMIT {
            hook_reason.store(LIMIT_WALL_TIME, Ordering::Relaxed);
            return true;
        }
        false
    })));

    reason
}

fn execute_module(
    ctx: Ctx<'_>,
    request: &ScriptRunRequest,
    config_json: &str,
    logs: Arc<Mutex<Logs>>,
) -> Result<String> {
    install_log_functions(&ctx, logs)?;

    let config_literal =
        serde_json::to_string(config_json).context("failed to quote javascript config payload")?;
    let config: Value = js_ctx_result(
        &ctx,
        ctx.eval(format!("JSON.parse({config_literal})")),
        format!(
            "failed to decode config for javascript transform {}",
            request.uid
        ),
    )?;

    let module = js_ctx_result(
        &ctx,
        Module::declare(
            ctx.clone(),
            format!("profile/{}.mjs", request.uid),
            request.source.as_bytes(),
        ),
        format!("failed to compile javascript transform {}", request.uid),
    )?;
    let (module, promise) = js_ctx_result(
        &ctx,
        module.eval(),
        format!("failed to evaluate javascript transform {}", request.uid),
    )?;
    js_ctx_result(
        &ctx,
        promise.finish::<()>(),
        format!(
            "javascript transform {} module initialization failed",
            request.uid
        ),
    )?;

    let transform: Function = js_ctx_result(
        &ctx,
        module.get("default"),
        format!(
            "javascript transform {} must export a default function",
            request.uid
        ),
    )?;
    let output: Value = js_ctx_result(
        &ctx,
        transform.call((config,)),
        format!("javascript transform {} failed", request.uid),
    )?;

    if output.is_promise() {
        bail!(
            "javascript transform {} must return a config mapping synchronously",
            request.uid
        );
    }
    if !output.is_object() || output.is_array() {
        bail!(
            "javascript transform {} must return a config mapping",
            request.uid
        );
    }

    js_ctx_result(
        &ctx,
        ctx.globals().set("__chimeraTransformResult", output),
        format!(
            "failed to capture javascript transform {} result",
            request.uid
        ),
    )?;
    js_ctx_result(
        &ctx,
        ctx.eval(
            r#"
(() => {
  const result = JSON.stringify(globalThis.__chimeraTransformResult);
  if (result === undefined) {
    throw new TypeError("transform result is not JSON-serializable");
  }
  return result;
})()
"#,
        ),
        format!(
            "javascript transform {} must return a JSON-serializable config mapping",
            request.uid
        ),
    )
}

fn install_log_functions(ctx: &Ctx<'_>, logs: Arc<Mutex<Logs>>) -> Result<()> {
    let sink = logs;
    let logger = js_ctx_result(
        ctx,
        Function::new(ctx.clone(), move |level: String, message: String| {
            let span = match level.as_str() {
                "info" => LogSpan::Info,
                "warn" => LogSpan::Warn,
                "error" => LogSpan::Error,
                _ => LogSpan::Log,
            };
            push_script_log(&mut sink.lock(), span, message);
        }),
        "failed to create javascript logger".into(),
    )?;
    js_ctx_result(
        ctx,
        ctx.globals().set("__chimeraLog", logger),
        "failed to install javascript logger".into(),
    )?;
    js_ctx_result(
        ctx,
        ctx.eval::<(), _>(LOGGER_BOOTSTRAP),
        "failed to initialize javascript logging helpers".into(),
    )?;
    Ok(())
}

fn js_result<T>(result: rquickjs::Result<T>, context: String) -> Result<T> {
    result.map_err(|error| anyhow::anyhow!("{context}: {error}"))
}

fn js_ctx_result<T>(ctx: &Ctx<'_>, result: rquickjs::Result<T>, context: String) -> Result<T> {
    match result {
        Ok(value) => Ok(value),
        Err(rquickjs::Error::Exception) => {
            let caught = ctx.catch();
            if let Some(exception) = caught.as_exception() {
                let message = exception
                    .message()
                    .unwrap_or_else(|| "javascript exception".into());
                if let Some(stack) = exception.stack() {
                    return Err(anyhow::anyhow!("{context}: {message}\n{stack}"));
                }
                return Err(anyhow::anyhow!("{context}: {message}"));
            }
            Err(anyhow::anyhow!(
                "{context}: javascript threw a {} value",
                caught.type_name()
            ))
        }
        Err(error) => Err(anyhow::anyhow!("{context}: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(source: &str) -> Mapping {
        serde_yaml::from_str(source).unwrap()
    }

    fn request(source: &str, config: Mapping) -> ScriptRunRequest {
        ScriptRunRequest {
            uid: "sj-test".into(),
            source: source.into(),
            config,
        }
    }

    #[tokio::test]
    async fn javascript_runner_returns_and_mutates_config() {
        let output = JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  config["unified-delay"] = true;
  config.dns ??= {};
  config.dns.enable = true;
  return config;
}
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
    async fn javascript_runner_round_trips_nested_sequences() {
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
        let output = JavaScriptRunner::new()
            .run(request(
                "export default (config) => config;",
                config.clone(),
            ))
            .await
            .unwrap();

        assert_eq!(output.config, config);
    }

    #[tokio::test]
    async fn javascript_runner_captures_logs() {
        let output = JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  console.log("hello", 7);
  warn("careful");
  return config;
}
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
    async fn javascript_runner_bounds_retained_logs() {
        use crate::enhance::chain::{SCRIPT_LOG_ENTRY_LIMIT, SCRIPT_LOG_MESSAGE_LIMIT_BYTES};

        let output = JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  console.log("x".repeat(6000));
  for (let index = 0; index < 300; index += 1) {
    console.info(`message-${index}`);
  }
  return config;
}
"#,
                Mapping::new(),
            ))
            .await
            .unwrap();

        assert_eq!(output.logs.len(), SCRIPT_LOG_ENTRY_LIMIT);
        assert!(output.logs[0].1.len() <= SCRIPT_LOG_MESSAGE_LIMIT_BYTES);
        assert!(output.logs[0].1.ends_with("… [truncated]"));
        assert_eq!(output.logs.last().map(|entry| entry.0), Some(LogSpan::Warn));
        assert!(
            output
                .logs
                .last()
                .is_some_and(|entry| entry.1.contains("discarded"))
        );
    }

    #[tokio::test]
    async fn javascript_runner_exposes_no_host_io_apis() {
        JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  if (typeof process !== "undefined") throw new Error("process exposed");
  if (typeof require !== "undefined") throw new Error("require exposed");
  if (typeof Deno !== "undefined") throw new Error("Deno exposed");
  if (typeof Bun !== "undefined") throw new Error("Bun exposed");
  if (typeof fetch !== "undefined") throw new Error("fetch exposed");
  if (typeof XMLHttpRequest !== "undefined") throw new Error("XHR exposed");
  return config;
}
"#,
                Mapping::new(),
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn javascript_runner_rejects_non_mapping_results() {
        let error = JavaScriptRunner::new()
            .run(request("export default () => 42;", Mapping::new()))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("must return a config mapping"));
    }

    #[tokio::test]
    async fn javascript_runner_rejects_async_results() {
        let error = JavaScriptRunner::new()
            .run(request(
                "export default async (config) => config;",
                Mapping::new(),
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("synchronously"));
    }

    #[tokio::test]
    async fn javascript_runner_interrupts_infinite_loops() {
        let error = JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  while (true) {}
  return config;
}
"#,
                Mapping::new(),
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("callback budget") || error.contains("execution time limit"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn javascript_runner_enforces_memory_limit() {
        let error = JavaScriptRunner::new()
            .run(request(
                r#"
export default function (config) {
  const values = [];
  for (let i = 0; i < 1000000; i += 1) {
    values.push(("x".repeat(1024) + i).slice());
  }
  return config;
}
"#,
                Mapping::new(),
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.to_ascii_lowercase().contains("memory")
                || error.contains("callback budget")
                || error.contains("execution time limit"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn javascript_runner_has_no_module_loader() {
        let error = JavaScriptRunner::new()
            .run(request(
                r#"
import * as fs from "node:fs";
export default (config) => config;
"#,
                Mapping::new(),
            ))
            .await
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("could not load module 'node:fs'"),
            "unexpected error: {error}"
        );
    }
}
