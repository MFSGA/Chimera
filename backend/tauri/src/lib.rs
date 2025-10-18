use tauri_specta::collect_commands;

mod ipc;

mod config;
/// 3
mod core;
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
    // setup specta
    let specta_builder = tauri_specta::Builder::<tauri::Wry>::new().commands(collect_commands![
        // demo
        greet,
        // profile
        ipc::get_profiles,
        ipc::import_profile,
        // ipc::view_profile,
        // updater layer
        // todo ipc::check_update,
    ]);

    tauri::Builder::default()
        .invoke_handler(specta_builder.invoke_handler())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
