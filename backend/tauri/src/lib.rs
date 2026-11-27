use tauri::Manager;
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
use tauri_plugin_deep_link::DeepLinkExt;

use crate::utils::{init, path::PathResolver, resolve};

#[cfg(feature = "e2e")]
mod e2e_test;
mod ipc;
// Mobile platforms cannot launch a copy of the desktop executable; they
// retain the in-process Runner and do not compile the child-process entry.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod migration_child;
mod network_info;
mod network_probe;

mod adapters;
mod bridge;
mod client;
mod config;
/// 5
mod consts;
/// 3
mod core;
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
mod desktop;
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
#[cfg(all(
    any(target_os = "macos", target_os = "linux", windows),
    any(not(feature = "e2e"), test)
))]
mod single_instance_activation;
mod specta_export;
mod startup_failure;
// Standalone desktop panic dialog, dispatched before Tauri and storage setup.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod panic_reporter;
// The reporter is a separate desktop process, not a Tauri window/owner.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod startup_reporter;
mod state;
/// 4
mod utils;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub mod widget;
/// 9
mod window;

mod host_paths;

mod bundle;

rust_i18n::i18n!("./locales");

#[cfg(feature = "deadlock-detection")]
fn deadlock_detection() {
    use parking_lot::deadlock;
    use std::{thread, time::Duration};
    use tracing::error;
    thread::spawn(move || {
        loop {
            thread::sleep(Duration::from_secs(10));
            let deadlocks = deadlock::check_deadlock();
            if deadlocks.is_empty() {
                continue;
            }

            error!("{} deadlocks detected", deadlocks.len());
            for (i, threads) in deadlocks.iter().enumerate() {
                error!("Deadlock #{}", i);
                for t in threads {
                    error!("Thread Id {:#?}", t.thread_id());
                    error!("{:#?}", t.backtrace());
                }
            }
        }
    });
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[specta::specta]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> std::process::ExitCode {
    // Install after internal child-command dispatch in main(): reporter and
    // migration subprocesses never recursively register this panic hook.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    panic_reporter::install_main_thread_hook();

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
    let mut profilers = utils::profiling::Profilers::default();
    #[cfg(feature = "dhat-heap")]
    profilers.start_heap(&paths);
    // Defer filesystem initialization until Tauri has initialized the
    // single-instance plugin. A losing secondary process must not write config.
    #[cfg(feature = "deadlock-detection")]
    deadlock_detection();

    let mut context = tauri::generate_context!();
    let executable_dir = match paths.app_install_dir() {
        Ok(directory) => directory.as_std_path(),
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
    let metadata =
        match bundle::resolve(cfg!(windows), context.config_mut(), executable_dir, || {
            tauri::webview_version().is_ok()
        }) {
            Ok(metadata) => metadata,
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
    let updater = match bundle::setup(&metadata, context.config_mut()) {
        Ok(updater) => updater,
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

    let specta_builder = specta_export::build_specta_builder();

    // Secondary invocations may arrive before primary application setup finishes.
    #[cfg(all(
        any(target_os = "macos", target_os = "linux", windows),
        not(feature = "e2e")
    ))]
    let secondary_activation =
        std::sync::Arc::new(single_instance_activation::PendingWindowActivation::default());

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default();
    builder = builder
        .manage(utils::exit::ExitBoundary::default())
        // Deep links wait here until a frontend takes them; macOS can deliver
        // one before setup runs.
        .manage(crate::ipc::PendingDeepLink::default());

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
        let activation = secondary_activation.clone();
        builder = builder.plugin(tauri_plugin_single_instance::init(
            move |app, _args, _cwd| {
                // Before readiness, enqueue one activation instead of opening a window.
                if activation.request() {
                    resolve::create_window(app);
                }
            },
        ));
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
        .register_asynchronous_uri_scheme_protocol("chimera-icons", adapters::icons::handle)
        .invoke_handler(invoke_handler)
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(updater.build())
        .plugin(tauri_plugin_global_shortcut::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .on_page_load(|webview, payload| {
            // A reloaded page keeps the same webview, so `Channel::send`
            // cannot tell its old connection-detail subscriptions are gone.
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                if let Some(registry) = webview.app_handle().try_state::<
                    crate::core::clash::connection_details::ConnectionDetailSubscriptions,
                >() {
                    registry.cancel_for_webview(webview.label());
                }
            }
        })
        .setup(move |app| {
            // Only the primary instance reaches this callback. Keep the legacy
            // implementation's PathBuf API as a narrow compatibility adapter:
            // both owners use the exact roots discovered once by chimera-paths.
            let setup_paths = PathResolver::with_base_dirs(
                paths.app_config_dir().as_std_path().to_path_buf(),
                paths.app_data_dir().as_std_path().to_path_buf(),
            );
            let setup_failure_capture = parking_lot::Mutex::new(None);
            let failure_host = setup::TauriStartupFailureHost {
                handle: app.handle().clone(),
            };

            setup::run_setup(
                setup::StartupOwners::default(),
                |owners| {
                    // Do not use the reference directory helper here: it can remove a
                    // file obstructing a root. Chimera must preserve user data on error.
                    setup_paths.create_base_dirs()?;
                    // Before Migration, initialize only the logger. Default
                    // YAML files would alter baselines and snapshot contents.
                    init::init_logging(&setup_paths)?;

                    #[cfg(all(debug_assertions, not(feature = "e2e")))]
                    {
                        const SPECTA_BINDINGS_PATH: &str = "../../frontend/interface/src/ipc/bindings.ts";
                        let bindings_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join(SPECTA_BINDINGS_PATH);
                        specta_builder.export(
                            specta_typescript::Typescript::default()
                                .header("/* oxlint-disable */\n// @ts-nocheck"),
                            &bindings_path,
                        )?;
                        let prettier_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../node_modules/.bin")
                            .join(if cfg!(target_os = "windows") {
                                "prettier.cmd"
                            } else {
                                "prettier"
                            });
                        let mut prettier = if cfg!(target_os = "windows") {
                            let mut command = std::process::Command::new("cmd");
                            command.arg("/C").arg(prettier_path);
                            command
                        } else {
                            std::process::Command::new(prettier_path)
                        };
                        let _ = prettier.args(["--write"]).arg(&bindings_path).output();
                        specta_export::normalize_typescript_bindings(&bindings_path)?;
                        log::debug!(
                            "Exported TypeScript bindings, path: {}",
                            bindings_path.display()
                        );
                    }

                    specta_builder.mount_events(app);
                    setup::setup(
                        app,
                        setup_paths.clone(),
                        paths.clone(),
                        metadata,
                        owners,
                        &setup_failure_capture,
                    )?;
                    app.manage(owners.clone());
                    core::updater::recover_interrupted_update();
                    core::clash::setup(app)?;

                    #[cfg(feature = "agent")]
                    features::agent::setup(app);

                    // All window/IPC state must exist before the first webview loads.
                    app.manage(crate::window::WindowManager::new(app.handle().clone()));
                    app.manage(crate::utils::resolve::AppStartupStatus::default());

                    #[cfg(target_os = "macos")]
                    {
                        use tauri::menu::{MenuBuilder, SubmenuBuilder};
                        let menu = (|| -> tauri::Result<()> {
                            let submenu = SubmenuBuilder::new(app, "Edit")
                                .undo()
                                .redo()
                                .copy()
                                .paste()
                                .cut()
                                .select_all()
                                .close_window()
                                .quit()
                                .build()?;
                            let menu = MenuBuilder::new(app).item(&submenu).build()?;
                            app.set_menu(menu)?;
                            Ok(())
                        })();
                        if let Err(error) = menu {
                            tracing::warn!(%error, "Edit menu unavailable; continuing without it");
                        }
                    }

                    resolve::resolve_setup(app);

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

                    #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
                    app.deep_link().register_all()?;

                    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
                    {
                        let app_handle = app.handle().clone();
                        let on_open_url_handle = app_handle.clone();

                        app.deep_link().on_open_url(move |event| {
                            for url in event.urls().into_iter().filter_map(desktop::deep_link::accepted) {
                                desktop::deep_link::deliver(&on_open_url_handle, &url);
                            }
                        });

                        // The Client, window state and deep-link listener now exist.
                        // Deliver any activation coalesced during the primary startup.
                        #[cfg(not(feature = "e2e"))]
                        if secondary_activation.mark_ready() {
                            desktop::deep_link::show_main_window(app.handle());
                        }

                        if let Some(urls) = app.deep_link().get_current()? {
                            for url in urls.into_iter().filter_map(desktop::deep_link::accepted) {
                                desktop::deep_link::deliver(&app_handle, &url);
                            }
                        }
                    }

                    Ok(())
                },
                |error| {
                    setup_failure_capture.lock().take().unwrap_or_else(|| {
                        startup_failure::Report::from_error(
                            error.as_ref(),
                            startup_failure::StartupStage::Setup,
                            &setup_paths,
                        )
                    })
                },
                &failure_host,
            )
        });

    let app = match builder.build(context) {
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

    app.run(move |app_handle, e| match e {
        tauri::RunEvent::ExitRequested { api, code, .. } => {
            if code.is_some() {
                profilers.finish_heap();
            }
            utils::exit::on_exit_requested(app_handle, code, &api);
        }
        tauri::RunEvent::Exit => {
            profilers.finish();
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            // The event loop can no longer complete scheduled UI work.
            // Release hotkey actors awaiting a main-thread result.
            if let Some(handoff) = app_handle
                .try_state::<std::sync::Arc<crate::client::hotkey::main_thread::MainThreadHandoff>>(
                )
            {
                handoff.mark_event_loop_lost();
            }
        }
        tauri::RunEvent::WindowEvent { label, event, .. } => {
            if label == crate::consts::LEGACY_WINDOW_LABEL
                || label == crate::consts::MAIN_WINDOW_LABEL
            {
                match &event {
                    tauri::WindowEvent::CloseRequested { .. } => {
                        // WindowManager queues geometry through SessionState.
                        log::debug!(target: "app", "window close requested: {label}");
                    }
                    tauri::WindowEvent::Destroyed => {
                        log::debug!(target: "app", "window destroyed");
                    }
                    _ => {}
                }
            }
            #[cfg(any(windows, target_os = "linux"))]
            if matches!(event, tauri::WindowEvent::ScaleFactorChanged { .. }) {
                crate::log_err!(crate::core::tray::Tray::on_scale_factor_changed(app_handle));
            }

            // Every webview's connection-detail subscriptions must end on
            // destroy, not just "main"'s: `Channel::send` cannot detect it.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(registry) = app_handle.try_state::<
                    crate::core::clash::connection_details::ConnectionDetailSubscriptions,
                >() {
                    registry.cancel_for_webview(&label);
                }
                resolve::mark_frontend_unmounted(&label);
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { .. } => {
            resolve::create_window(app_handle);
        }
        _ => {}
    });

    std::process::ExitCode::SUCCESS
}
