use tauri_specta::collect_commands;

use crate::utils::{init, resolve};

mod ipc;

mod config;
/// 5
mod consts;
/// 3
mod core;
/// 7
mod enhance;
/// 6
mod feat;
/// 8
#[cfg(windows)]
mod shutdown_hook;
/// 4
mod utils;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[specta::specta]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() -> std::io::Result<()> {
    crate::log_err!(init::init_config());

    // setup specta
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(collect_commands![
        // demo
        greet,
        // profile
        ipc::get_profiles,
        ipc::import_profile,
        ipc::view_profile,
        ipc::patch_profiles_config,
        // verge
        ipc::get_verge_config,
        ipc::patch_verge_config,
        // clash
        ipc::get_clash_info,
        // updater layer
        ipc::check_update,
        // utils
        ipc::is_appimage,
        ipc::open_that,
    ]);

    #[cfg(debug_assertions)]
    {
        const SPECTA_BINDINGS_PATH: &str = "../../frontend/interface/src/ipc/bindings.ts";

        match specta_builder.export(
            specta_typescript::Typescript::default()
                .formatter(specta_typescript::formatter::prettier)
                .formatter(|file| {
                    let npx_command = if cfg!(target_os = "windows") {
                        "npx.cmd"
                    } else {
                        "npx"
                    };

                    std::process::Command::new(npx_command)
                        .arg("prettier")
                        .arg("--write")
                        .arg(file)
                        .output()
                        .map(|_| ())
                        .map_err(std::io::Error::other)
                })
                .bigint(specta_typescript::BigIntExportBehavior::Number)
                .header("/* eslint-disable */\n// @ts-nocheck"),
            SPECTA_BINDINGS_PATH,
        ) {
            Ok(_) => {
                log::debug!("Exported typescript bindings, path: {SPECTA_BINDINGS_PATH}");
            }
            Err(e) => {
                panic!("Failed to export typescript bindings: {e}");
            }
        };
    }

    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .invoke_handler(specta_builder.invoke_handler())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            specta_builder.mount_events(app);

            resolve::resolve_setup(app);

            Ok(())
        });

    let app = builder
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(|app_handle, e| match e {
        tauri::RunEvent::ExitRequested { .. } => {
            utils::help::cleanup_processes(app_handle);
        }
        e => {
            log::debug!("Tauri Event: {:?}", e);
        }
    });

    Ok(())
}
