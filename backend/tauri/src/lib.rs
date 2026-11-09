#[cfg(debug_assertions)]
use std::{io::Write, time::SystemTime};

#[cfg(debug_assertions)]
use tauri::Listener;
use tauri::{Emitter, Manager};
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
use tauri_plugin_deep_link::DeepLinkExt;

use crate::utils::{init, path::PathResolver, resolve};

#[cfg(feature = "e2e")]
mod e2e_test;
mod ipc;
mod network_probe;

mod bridge;
mod client;
mod config;
/// 5
mod consts;
/// 3
mod core;
/// 7
mod enhance;
/// 6
mod feat;
mod features;
mod service;
mod setup;
/// 8
#[cfg(windows)]
mod shutdown_hook;
mod specta_export;
mod startup_failure;
mod state;
/// 4
mod utils;
/// 9
mod window;

mod host_paths;

mod bundle;

rust_i18n::i18n!("./locales");

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[specta::specta]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg(debug_assertions)]
fn setup_frontend_console_bridge(app: &mut tauri::App) {
    app.listen("frontend-console", move |event| {
        let Ok(current_dir) = std::env::current_dir() else {
            return;
        };
        let log_dir = current_dir.join(".tmp");

        if std::fs::create_dir_all(&log_dir).is_err() {
            return;
        }

        let timestamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let line = format!("[{timestamp}] {}\n", event.payload());

        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join("frontend-console.log"))
        {
            let _ = file.write_all(line.as_bytes());
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> std::process::ExitCode {
    // Nothing before the logger reaches a trace, so its share is logged.
    let started = std::time::Instant::now();
    rust_i18n::set_locale(utils::help::detect_system_i18n_key());

    // The one resolver: every later stage is given it, none looks the directories up again.
    // Every stage needs both roots, so a host that has none ends the process here.
    let paths = match host_paths::discover() {
        Ok(paths) => paths,
        Err(error) => {
            let path = match &error {
                chimera_paths::DiscoverError::NotUtf8 { path, .. } => Some(path.clone()),
                chimera_paths::DiscoverError::PortableInstallDir {
                    source: chimera_paths::InstallDirError::NotUtf8 { path },
                } => Some(path.clone()),
                _ => None,
            };
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Directories,
                path,
                &error,
                None,
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    // Defer filesystem initialization until Tauri has initialized the
    // single-instance plugin. A losing secondary process must not write config.

    let specta_builder = specta_export::build_specta_builder();

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    builder = builder.manage(utils::exit::ExitBoundary::default());

    #[cfg(feature = "e2e")]
    {
        builder = builder.plugin(tauri_plugin_wdio_webdriver::init());
    }

    #[cfg(all(
        any(target_os = "macos", target_os = "linux", windows),
        not(feature = "e2e")
    ))]
    {
        // The deep-link plugin must exist before a secondary invocation reaches
        // the single-instance callback.
        builder = builder.plugin(tauri_plugin_deep_link::init());
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            resolve::create_window(app);
        }));
    }

    #[cfg(all(
        any(target_os = "macos", target_os = "linux", windows),
        feature = "e2e"
    ))]
    {
        builder = builder.plugin(tauri_plugin_deep_link::init());
    }

    let specta_invoke_handler = specta_builder.invoke_handler();
    #[cfg(feature = "e2e")]
    let e2e_invoke_handler: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
        crate::e2e_test::e2e_arm_widget_storage_failure,
        crate::e2e_test::e2e_widget_storage_failure_status,
        crate::e2e_test::e2e_clear_widget_storage_failure
    ];
    let invoke_handler = move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
        #[cfg(feature = "e2e")]
        if matches!(
            invoke.message.command(),
            "e2e_arm_widget_storage_failure"
                | "e2e_widget_storage_failure_status"
                | "e2e_clear_widget_storage_failure"
        ) {
            return e2e_invoke_handler(invoke);
        }

        specta_invoke_handler(invoke)
    };

    builder = builder
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(registry) = webview.app_handle().try_state::<
                    crate::core::clash::connection_details::ConnectionDetailSubscriptions,
                >() {
                    registry.cancel_for_webview(webview.label());
                }
            }
        })
        .invoke_handler(invoke_handler)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .setup(move |app| {
            // Only the primary instance reaches this callback. Keep the legacy
            // implementation's PathBuf API as a narrow compatibility adapter:
            // both owners use the exact roots discovered once by chimera-paths.
            let setup_paths = PathResolver::with_base_dirs(
                paths.app_config_dir().as_std_path().to_path_buf(),
                paths.app_data_dir().as_std_path().to_path_buf(),
            );
            // Do not use the reference directory helper here: it can remove a
            // file obstructing a root. Chimera must preserve user data on error.
            setup_paths.create_base_dirs()?;
            init::init_config(&setup_paths)?;

            #[cfg(all(debug_assertions, not(feature = "e2e")))]
            {
                const SPECTA_BINDINGS_PATH: &str = "../../frontend/interface/src/ipc/bindings.ts";
                specta_builder.export(
                    specta_typescript::Typescript::default()
                        .header("/* oxlint-disable */\n// @ts-nocheck"),
                    SPECTA_BINDINGS_PATH,
                )?;
                let npx_command = if cfg!(target_os = "windows") { "npx.cmd" } else { "npx" };
                let status = std::process::Command::new(npx_command)
                    .args(["prettier", "--write", SPECTA_BINDINGS_PATH])
                    .status()?;
                if !status.success() {
                    return Err(format!("Prettier failed for {SPECTA_BINDINGS_PATH}").into());
                }
                specta_export::normalize_typescript_bindings(SPECTA_BINDINGS_PATH)?;
            }

            specta_builder.mount_events(app);
            setup::setup(app, setup_paths)?;
            core::updater::recover_interrupted_update();
            core::clash::setup(app)?;

            #[cfg(feature = "agent")]
            features::agent::setup(app);

            resolve::resolve_setup(app);
            app.manage(crate::ipc::PendingDeepLink::default());

            #[cfg(windows)]
            {
                let app_handle = app.handle().clone();
                let stop_strategy = std::sync::Arc::clone(
                    app.state::<std::sync::Arc<chimera_utils::process::AtomicStopStrategy>>()
                        .inner(),
                );
                shutdown_hook::setup_shutdown_hook(move || {
                    // Forced Windows session shutdown must not wait for a graceful core exit.
                    stop_strategy.store(
                        chimera_utils::process::StopStrategy::Kill,
                        std::sync::atomic::Ordering::Release,
                    );
                    crate::log_err!(utils::exit::shutdown_before_exit_blocking(
                        &app_handle
                    ));
                })?;
            }

            #[cfg(debug_assertions)]
            setup_frontend_console_bridge(app);

            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            app.deep_link().register_all()?;

            #[cfg(any(target_os = "macos", target_os = "linux", windows))]
            {
                let app_handle = app.handle().clone();
                let on_open_url_handle = app_handle.clone();

                app.deep_link().on_open_url(move |event| {
                    let Some(url) = event.urls().first().map(ToString::to_string) else {
                        return;
                    };

                    let target_label = resolve::configured_window_label();
                    resolve::create_window(&on_open_url_handle);

                    let entry = on_open_url_handle
                        .state::<crate::ipc::PendingDeepLink>()
                        .store(url.clone());

                    if !resolve::wait_for_frontend_ready(
                        target_label,
                        std::time::Duration::from_secs(15),
                    ) {
                        log::warn!(target: "app", "frontend {target_label} did not become ready before delivering scheme request");
                        return;
                    }

                    if let Err(error) = on_open_url_handle.emit_to(
                        target_label,
                        "scheme-request-received",
                        entry,
                    ) {
                        log::error!(target: "app", "failed to emit scheme request {url} to {target_label}: {error:?}");
                    }
                });

                if let Some(url) = app
                    .deep_link()
                    .get_current()?
                    .and_then(|urls| urls.first().map(ToString::to_string))
                {
                    let target_label = resolve::configured_window_label();
                    let entry = app
                        .state::<crate::ipc::PendingDeepLink>()
                        .store(url.clone());
                    if resolve::wait_for_frontend_ready(
                        target_label,
                        std::time::Duration::from_secs(15),
                    ) {
                        app_handle.emit_to(target_label, "scheme-request-received", entry)?;
                    } else {
                        log::warn!(target: "app", "frontend {target_label} did not become ready before delivering startup scheme request");
                    }
                }
            }

            Ok(())
        });

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(error) => {
            startup_failure::show_one_shot(
                startup_failure::FailureKind::Internal,
                None,
                &error,
                None,
            );
            return std::process::ExitCode::FAILURE;
        }
    };

    app.run(|app_handle, e| match e {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        tauri::RunEvent::Exit => {
            // The event loop can no longer complete scheduled UI work.
            // Release hotkey actors awaiting a main-thread result.
            if let Some(handoff) = app_handle.try_state::<
                std::sync::Arc<crate::client::hotkey::main_thread::MainThreadHandoff>,
            >() {
                handoff.mark_event_loop_lost();
            }
        }
        // pay attention to this event order, otherwise the app will quit immediately after all windows are closed
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            utils::exit::on_exit_requested(app_handle, code, &api);
        }
        tauri::RunEvent::WindowEvent { label, event, .. }
            if label == crate::consts::LEGACY_WINDOW_LABEL
                || label == crate::consts::MAIN_WINDOW_LABEL =>
        {
            match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    log::debug!(target: "app", "window close requested: {label}");
                    match app_handle.try_state::<crate::client::ChimeraClient>() {
                        Some(client) => {
                            if let Err(error) = chimera_utils::runtime::block_on(
                                crate::window::persist_window_state(
                                    app_handle,
                                    client.inner(),
                                    label.as_str(),
                                ),
                            ) {
                                log::error!(target: "app", "failed to persist window state for {label}: {error:?}");
                            }
                        }
                        None => {
                            log::warn!(target: "app", "ChimeraClient unavailable while persisting window state for {label}");
                        }
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    if let Some(registry) = app_handle.try_state::<
                        crate::core::clash::connection_details::ConnectionDetailSubscriptions,
                    >() {
                        registry.cancel_for_webview(label.as_str());
                    }
                    resolve::mark_frontend_unmounted(label.as_str());
                }
                _ => {}
            }
        }
        e => {
            // tracing::debug!("Tauri Event: {:?}", e);
        }
    });

    std::process::ExitCode::SUCCESS
}
