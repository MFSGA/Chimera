//! Desktop adapters for the shared ref diagnosis, with no separate log capture.
use crate::client::ChimeraClient;
use chimera_contracts::core_version::CoreVersionReader;
use chimera_diagnose::{
    Action, Event, Step,
    archive::{Archive, LOG_CAP},
    report::{CoreInfo, PageCapabilities, Report, WebviewInfo},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

fn home() -> Option<String> {
    dirs::home_dir().map(|path| path.to_string_lossy().into_owned())
}

fn initial_report(page: PageCapabilities) -> Report {
    let mut report = Report::default();
    report.page(page);
    let build =
        serde_json::to_value(&*crate::consts::BUILD_INFO).expect("build metadata serializes");
    report.build(
        build
            .as_object()
            .expect("build metadata is an object")
            .iter()
            .map(|(key, value)| (key.clone(), value.as_str().unwrap_or_default().to_owned())),
    );
    report
}

fn collect_webview_info() -> WebviewInfo {
    let engine = if cfg!(windows) {
        "WebView2"
    } else if cfg!(target_os = "macos") {
        "WKWebView"
    } else {
        "WebKitGTK"
    };
    let (version, version_error) = match tauri::webview_version() {
        Ok(version) => (Some(version), None),
        Err(error) => (None, Some(error.to_string())),
    };
    WebviewInfo {
        engine: engine.into(),
        version,
        version_error,
        // Arbitrary environment values may contain private paths or credentials.
        browser_args: None,
        env: BTreeMap::new(),
        tauri_version: tauri::VERSION.into(),
        wry_version: option_env!("WRY_VERSION").unwrap_or("unknown").into(),
    }
}

fn diagnose(
    logs: &Path,
    client: ChimeraClient,
    page: PageCapabilities,
    with_archive: bool,
    mut sink: impl FnMut(Event),
) {
    let runtime = tokio::runtime::Handle::current();
    let mut steps: Vec<(Step, Action<'_>)> = vec![
        (
            Step::Host,
            Box::new(|_, report, _| {
                let host = chimera_diagnose::host::collect_host_info();
                let error = host.gpu_error.clone();
                report.host(&host);
                error.map_or(Ok(()), Err)
            }),
        ),
        (
            Step::Webview,
            Box::new(|_, report, _| {
                let webview = collect_webview_info();
                let error = webview.version_error.clone();
                report.webview(webview);
                error.map_or(Ok(()), Err)
            }),
        ),
        (
            Step::Core,
            Box::new(|_, report, _| {
                let core = client.get_app_config().map_err(|e| e.to_string())?.core;
                let reader = chimera_os::core_version::SidecarCoreVersionReader::new(Arc::new(
                    crate::adapters::core_version::TauriCoreVersionExecutableSource,
                ));
                let version = runtime
                    .block_on(reader.read(core))
                    .map_err(|e| e.to_string());
                let error = version.as_ref().err().cloned();
                report.core(CoreInfo {
                    name: core.binary_name().into(),
                    version: version.ok(),
                    error: error.clone(),
                });
                error.map_or(Ok(()), Err)
            }),
        ),
    ];
    if with_archive {
        steps.push((
            Step::AppLogs,
            Box::new(|archive, report, fraction| {
                archive
                    .as_mut()
                    .ok_or("Archive unavailable")?
                    .app_logs(logs, report, fraction)
            }),
        ));
        steps.push((Step::CoreLogs, Box::new(|archive, report, fraction| {
            let export = runtime.block_on(client.streams().core_logs().export(home())).map_err(|e| e.to_string())?;
            archive.as_mut().ok_or("Archive unavailable")?.core_logs(&export.lines, fraction)?;
            fraction(1.);
            if export.truncated {
                report.notes.push(format!("Core logs truncated to the newest content within {LOG_CAP} uncompressed bytes."));
            }
            Ok(())
        })));
    }
    let archive = with_archive.then(|| Archive::new(logs, home()));
    chimera_diagnose::run(steps, initial_report(page), archive, |event| {
        // The shared runner executes all steps and never logs report contents.
        sink(event);
    });
}

/// Local-only report. No archive unless explicitly requested.
pub async fn collect(
    client: ChimeraClient,
    page: PageCapabilities,
    with_archive: bool,
) -> anyhow::Result<(String, Option<PathBuf>)> {
    let logs = crate::utils::dirs::app_logs_dir()?;
    tokio::task::spawn_blocking(move || {
        let mut finished = None;
        diagnose(&logs, client, page, with_archive, |event| {
            if let Event::Finished {
                environment,
                archive,
                ..
            } = event
            {
                finished = Some((environment, archive));
            }
        });
        let (environment, archive) =
            finished.ok_or_else(|| anyhow::anyhow!("diagnosis did not finish"))?;
        let environment =
            chimera_config::redact::redact_text(&environment, home().as_deref()).into_owned();
        Ok((environment, archive))
    })
    .await?
}
