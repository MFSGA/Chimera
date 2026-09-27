use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail};
use async_trait::async_trait;
use parking_lot::Mutex;
use rquickjs::{
    Context, Ctx, Function, Module, Runtime, Value,
    allocator::{Allocator, RustAllocator},
};
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

struct LimitedAllocator {
    allocator: RustAllocator,
    allocated: usize,
    limit: usize,
    limit_reached: Arc<AtomicBool>,
}

impl LimitedAllocator {
    fn new(limit: usize, limit_reached: Arc<AtomicBool>) -> Self {
        Self {
            allocator: RustAllocator,
            allocated: 0,
            limit,
            limit_reached,
        }
    }

    fn rounded_size(size: usize) -> Option<usize> {
        let alignment = std::mem::align_of::<u64>();
        size.checked_add(alignment - 1)
            .map(|size| size / alignment * alignment)
    }

    fn reserve(&mut self, old_size: usize, new_size: usize) -> bool {
        let Some(allocated) = self
            .allocated
            .checked_sub(old_size)
            .and_then(|allocated| allocated.checked_add(new_size))
        else {
            return self.mark_limit_reached();
        };
        if allocated > self.limit {
            return self.mark_limit_reached();
        }
        true
    }

    fn mark_limit_reached(&self) -> bool {
        self.limit_reached.store(true, Ordering::Relaxed);
        false
    }
}

unsafe impl Allocator for LimitedAllocator {
    fn alloc(&mut self, size: usize) -> *mut u8 {
        let Some(size) = Self::rounded_size(size) else {
            self.mark_limit_reached();
            return std::ptr::null_mut();
        };
        if !self.reserve(0, size) {
            return std::ptr::null_mut();
        }
        let ptr = self.allocator.alloc(size);
        if !ptr.is_null() {
            self.allocated += unsafe { RustAllocator::usable_size(ptr) };
        }
        ptr
    }

    fn calloc(&mut self, count: usize, size: usize) -> *mut u8 {
        let Some(size) = count.checked_mul(size).and_then(Self::rounded_size) else {
            self.mark_limit_reached();
            return std::ptr::null_mut();
        };
        if !self.reserve(0, size) {
            return std::ptr::null_mut();
        }
        let ptr = self.allocator.calloc(1, size);
        if !ptr.is_null() {
            self.allocated += unsafe { RustAllocator::usable_size(ptr) };
        }
        ptr
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        self.allocated -= unsafe { RustAllocator::usable_size(ptr) };
        unsafe { self.allocator.dealloc(ptr) };
    }

    unsafe fn realloc(&mut self, ptr: *mut u8, new_size: usize) -> *mut u8 {
        let Some(new_size) = Self::rounded_size(new_size) else {
            self.mark_limit_reached();
            return std::ptr::null_mut();
        };
        let old_size = if ptr.is_null() {
            0
        } else {
            unsafe { RustAllocator::usable_size(ptr) }
        };
        if !self.reserve(old_size, new_size) {
            return std::ptr::null_mut();
        }
        let ptr = unsafe { self.allocator.realloc(ptr, new_size) };
        if !ptr.is_null() {
            let allocated = self.allocated - old_size;
            self.allocated = allocated + unsafe { RustAllocator::usable_size(ptr) };
        }
        ptr
    }

    unsafe fn usable_size(ptr: *mut u8) -> usize {
        unsafe { RustAllocator::usable_size(ptr) }
    }
}

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
pub(crate) struct JSRunner;

impl JSRunner {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ScriptRunner for JSRunner {
    async fn run(&self, request: ScriptRunRequest) -> Result<ScriptRunOutput> {
        let uid = request.uid.clone();
        tokio::task::spawn_blocking(move || execute(request))
            .await
            .with_context(|| format!("javascript transform {uid} runtime task failed"))?
    }
}

fn execute(request: ScriptRunRequest) -> Result<ScriptRunOutput> {
    let memory_limit_reached = Arc::new(AtomicBool::new(false));
    let runtime = js_result(
        Runtime::new_with_alloc(LimitedAllocator::new(
            JS_MEMORY_LIMIT_BYTES,
            memory_limit_reached.clone(),
        )),
        format!(
            "failed to create javascript runtime for transform {}",
            request.uid
        ),
    )?;
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
            _ if memory_limit_reached.load(Ordering::Relaxed) => bail!(
                "javascript transform {} exceeded the memory limit ({} bytes)",
                request.uid,
                JS_MEMORY_LIMIT_BYTES
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
    let source = utils::wrap_script_if_not_esm(&request.source).with_context(|| {
        format!(
            "failed to normalize javascript transform {} source",
            request.uid
        )
    })?;

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
            source.as_bytes(),
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

mod utils {
    use std::borrow::Cow;

    use oxc_allocator::Allocator;
    use oxc_ast_visit::{
        Visit,
        walk::{walk_function, walk_module_export_name},
    };
    use oxc_parser::Parser;
    use oxc_span::{SourceType, Span};
    use oxc_syntax::scope::ScopeFlags;

    #[derive(Debug)]
    #[allow(dead_code)]
    struct DefaultExport {
        span: Span,
        is_function: bool,
    }

    #[derive(Debug, Default)]
    struct FunctionVisitor<'n> {
        exported_name: Vec<Cow<'n, str>>,
        declared_functions: Vec<(Cow<'n, str>, Cow<'n, Span>)>,
        default_export: Option<DefaultExport>,
    }

    impl<'n> Visit<'n> for FunctionVisitor<'n> {
        fn visit_module_export_name(&mut self, it: &oxc_ast::ast::ModuleExportName<'n>) {
            match it {
                oxc_ast::ast::ModuleExportName::IdentifierName(id) => {
                    self.exported_name.push(Cow::Borrowed(id.name.as_str()))
                }
                oxc_ast::ast::ModuleExportName::IdentifierReference(id) => {
                    self.exported_name.push(Cow::Borrowed(id.name.as_str()))
                }
                oxc_ast::ast::ModuleExportName::StringLiteral(value) => {
                    self.exported_name.push(Cow::Borrowed(value.value.as_str()))
                }
            }
            walk_module_export_name(self, it);
        }

        fn visit_function(&mut self, it: &oxc_ast::ast::Function<'n>, flags: ScopeFlags) {
            if let Some(id) = it.id.clone() {
                self.declared_functions
                    .push((Cow::Borrowed(id.name.as_str()), Cow::Owned(it.span)));
            }
            walk_function(self, it, flags);
        }

        fn visit_export_default_declaration(
            &mut self,
            it: &oxc_ast::ast::ExportDefaultDeclaration<'n>,
        ) {
            self.default_export = Some(DefaultExport {
                is_function: matches!(
                    it.declaration,
                    oxc_ast::ast::ExportDefaultDeclarationKind::FunctionDeclaration(_)
                ),
                span: it.span,
            });
        }
    }

    /// Wraps ref-style `function main(config) { ... }` scripts as ESM while
    /// preserving scripts that already declare a default export.
    pub(super) fn wrap_script_if_not_esm(script: &str) -> anyhow::Result<Cow<'_, str>> {
        let allocator = Allocator::default();
        let source_type = SourceType::default().with_module(true);
        // Parse the original source so the AST span remains valid when it is
        // used to insert the wrapper, including leading whitespace/comments.
        let result = Parser::new(&allocator, script, source_type).parse();

        if !result.diagnostics.is_empty() {
            let mut errors = String::new();
            for error in result.diagnostics {
                errors.push_str(&format!(
                    "{:?}\n",
                    error.with_source_code(script.to_string())
                ));
            }
            return Err(anyhow::anyhow!("parse error: {errors}"));
        }

        let mut visitor = FunctionVisitor::default();
        visitor.visit_program(&result.program);
        if visitor.default_export.is_some() {
            return Ok(Cow::Borrowed(script));
        }

        match visitor
            .declared_functions
            .iter()
            .find(|(name, _)| name.contains("main"))
        {
            Some((_, span)) => {
                let mut source = script.to_string();
                source.insert_str(span.start as usize, "export default ");
                Ok(Cow::Owned(source))
            }
            None => Err(anyhow::anyhow!("no default export or main function")),
        }
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
        let output = JSRunner::new()
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
    async fn javascript_runner_accepts_ref_main_function_and_captures_logs() {
        let output = JSRunner::new()
            .run(request(
                r#"
function main(config) {
  config.mode = "rule";
  console.log("scoped ran");
  return config;
}
"#,
                mapping("mode: direct\n"),
            ))
            .await
            .unwrap();

        assert_eq!(
            output
                .config
                .get("mode")
                .and_then(serde_yaml::Value::as_str),
            Some("rule")
        );
        assert!(
            output
                .logs
                .iter()
                .any(|(_, message)| message == "scoped ran")
        );
    }

    #[test]
    fn test_wrap_script_if_not_esm() {
        let script = "function main(config) {\n            return config\n        };";
        assert_eq!(
            utils::wrap_script_if_not_esm(script).unwrap().as_ref(),
            "export default function main(config) {\n            return config\n        };"
        );
    }

    #[test]
    fn test_wrap_script_if_esm() {
        let script =
            "export default function main(config) {\n            return config\n        };";
        assert_eq!(
            utils::wrap_script_if_not_esm(script).unwrap().as_ref(),
            script
        );
    }

    #[test]
    fn test_wrap_script_if_not_esm_sample_2() {
        let script = r#"// 国内DNS服务器
const domesticNameservers = [
  "https://dns.alidns.com/dns-query", // 阿里云公共DNS
  "https://doh.pub/dns-query", // 腾讯DNSPod
  "https://doh.360.cn/dns-query" // 360安全DNS
];
// 国外DNS服务器
const foreignNameservers = [
  "https://1.1.1.1/dns-query", // Cloudflare(主)
  "https://1.0.0.1/dns-query", // Cloudflare(备)
  "https://208.67.222.222/dns-query", // OpenDNS(主)
  "https://208.67.220.220/dns-query", // OpenDNS(备)
  "https://194.242.2.2/dns-query", // Mullvad(主)
  "https://194.242.2.3/dns-query" // Mullvad(备)
];
        function main(config) {
            // do something
            return config
        };"#;
        let expected = r#"// 国内DNS服务器
const domesticNameservers = [
  "https://dns.alidns.com/dns-query", // 阿里云公共DNS
  "https://doh.pub/dns-query", // 腾讯DNSPod
  "https://doh.360.cn/dns-query" // 360安全DNS
];
// 国外DNS服务器
const foreignNameservers = [
  "https://1.1.1.1/dns-query", // Cloudflare(主)
  "https://1.0.0.1/dns-query", // Cloudflare(备)
  "https://208.67.222.222/dns-query", // OpenDNS(主)
  "https://208.67.220.220/dns-query", // OpenDNS(备)
  "https://194.242.2.2/dns-query", // Mullvad(主)
  "https://194.242.2.3/dns-query" // Mullvad(备)
];
        export default function main(config) {
            // do something
            return config
        };"#;
        assert_eq!(
            utils::wrap_script_if_not_esm(script).unwrap().as_ref(),
            expected
        );
    }

    #[test]
    fn wrap_script_if_not_esm_preserves_indented_main_spans() {
        let script = "\n  function main(config) { return config; }\n";
        assert_eq!(
            utils::wrap_script_if_not_esm(script).unwrap().as_ref(),
            "\n  export default function main(config) { return config; }\n"
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
        let output = JSRunner::new()
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
        let output = JSRunner::new()
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

        let output = JSRunner::new()
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
        JSRunner::new()
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
        let error = JSRunner::new()
            .run(request("export default () => 42;", Mapping::new()))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("must return a config mapping"));
    }

    #[tokio::test]
    async fn javascript_runner_rejects_async_results() {
        let error = JSRunner::new()
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
        let error = JSRunner::new()
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
        let error = JSRunner::new()
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
        let error = JSRunner::new()
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
