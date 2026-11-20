#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> std::process::ExitCode {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    if let Some(result) = chimera_lib::migration_child::maybe_run_from_command_line() {
        return result;
    }

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
