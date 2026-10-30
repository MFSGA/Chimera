#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() {
    #[cfg(windows)]
    if let Some(result) = chimera_utils::process::windows_shutdown_helper_entry() {
        if let Err(error) = result {
            eprintln!("Windows core shutdown helper failed: {error}");
            std::process::exit(1);
        }
        return;
    }
    chimera_lib::run().unwrap();
}
