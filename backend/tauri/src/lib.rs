use tauri_specta::collect_commands;

use crate::utils::init;

mod ipc;

mod config;
/// 5
mod consts;
/// 3
mod core;
/// 6
mod feat;
/// 4
mod utils;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
#[specta::specta]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    crate::log_err!(init::init_config());

    // setup specta
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(collect_commands![
        // demo
        greet,
        // profile
        ipc::get_profiles,
        ipc::import_profile,
        ipc::view_profile,
        // verge
        ipc::get_verge_config,
        ipc::patch_verge_config,
        // updater layer
        // todo ipc::check_update,
    ]);

    tauri::Builder::default()
        .invoke_handler(specta_builder.invoke_handler())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
