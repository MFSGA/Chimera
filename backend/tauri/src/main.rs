#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    #[cfg(windows)]
    if let Some(result) = chimera_utils::process::windows_shutdown_helper_entry() {
        if let Err(error) = result {
            eprintln!("Windows core shutdown helper failed: {error}");
            return std::process::ExitCode::FAILURE;
        }
        return std::process::ExitCode::SUCCESS;
    }

    chimera_lib::run()
}
