//! Isolated panic reporter based on ref's `cmds/panic_dialog.rs` and
//! `nyanpasu-crash-reporter::PanicDetails`. This command does not construct a
//! Tauri App, claim a single instance or open the running app's storage.
use std::{
    ffi::OsStr,
    io::{self, Read as _, Write as _},
    process::{Command, ExitCode, Stdio},
};

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

const REPORTER_FLAG: &str = "--chimera-internal-panic-dialog";
const MAX_REPORT_BYTES: u64 = 1024 * 1024;

/// Keep the same fields as the upstream crash reporter protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanicDetails {
    pub parent_pid: u32,
    pub payload: Option<String>,
    pub location: Option<String>,
    pub backtrace: String,
}

/// The CLI must dispatch before `chimera_lib::run()`. Reporter processes
/// never install the panic hook or recursively launch another reporter.
pub fn maybe_run_from_command_line() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(REPORTER_FLAG)) {
        return None;
    }
    if args.next().is_some() {
        eprintln!("panic reporter takes no additional arguments");
        return Some(ExitCode::FAILURE);
    }
    Some(match read_and_display(io::stdin().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to display Chimera panic: {error:#}");
            ExitCode::FAILURE
        }
    })
}

fn read_and_display(input: impl io::Read) -> anyhow::Result<()> {
    read_and_show(input, |details| {
        let message = format_report(&details);
        eprintln!("{message}");
        let concise = message.lines().take(44).collect::<Vec<_>>().join("\n");
        crate::utils::dialog::error_dialog(concise);
        Ok(())
    })
}

/// An injectable presentation boundary mirrors the upstream parser and
/// keeps malformed input from creating a GUI or touching user data.
fn read_and_show(
    mut input: impl io::Read,
    show: impl FnOnce(PanicDetails) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut json = Vec::new();
    input
        .by_ref()
        .take(MAX_REPORT_BYTES + 1)
        .read_to_end(&mut json)
        .context("failed to read panic report from stdin")?;
    if json.len() as u64 > MAX_REPORT_BYTES {
        bail!("panic report exceeds maximum permitted size");
    }
    let details: PanicDetails =
        serde_json::from_slice(&json).context("failed to decode panic details")?;
    show(details)
}

fn format_report(details: &PanicDetails) -> String {
    format!(
        "Chimera {} ({}) encountered a fatal panic.\n\nParent PID: {}\nOS: {} / {}\nPayload: {}\nLocation: {}\n\nBacktrace:\n{}",
        crate::consts::BUILD_INFO.pkg_version,
        crate::consts::BUILD_INFO.commit_hash,
        details.parent_pid,
        std::env::consts::OS,
        std::env::consts::ARCH,
        details.payload.as_deref().unwrap_or("<non-string payload>"),
        details.location.as_deref().unwrap_or("<unknown>"),
        details.backtrace,
    )
}

/// A panic hook cannot wait on a GUI process and must not retain its stdin.
/// Synchronous JSON transmission is all we wait for; the independent reporter
/// keeps its own event loop after the panicking parent exits.
#[allow(clippy::zombie_processes)]
pub(crate) fn spawn_from_hook(details: &PanicDetails) -> anyhow::Result<()> {
    let json = serde_json::to_vec(details).context("failed to encode panic details")?;
    if json.len() as u64 > MAX_REPORT_BYTES {
        bail!("panic details exceed maximum permitted size");
    }
    let exe = std::env::current_exe().context("failed to locate panic reporter executable")?;
    let mut child = Command::new(exe)
        .arg(REPORTER_FLAG)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn independent panic reporter")?;
    let mut stdin = child.stdin.take().context("panic reporter stdin missing")?;
    stdin
        .write_all(&json)
        .context("failed to transmit panic details")?;
    drop(stdin); // The child receives EOF before its independent event loop.
    Ok(())
}

/// Use upstream `nyanpasu-panics` frame capture, logging and recoverable
/// scope support. Some existing Chimera async Actors also catch panic via
/// `FutureExt::catch_unwind()`; those are not yet converted to upstream's
/// explicit synchronous `catch_recoverable`. To preserve their isolation,
/// only panic on this designated UI/main thread invokes the fatal reporter.
pub(crate) fn install_main_thread_hook() {
    let main_thread = std::thread::current().id();
    chimera_panics::setup_panic_hook(move |report| {
        if std::thread::current().id() != main_thread {
            return;
        }
        if report
            .payload
            .is_some_and(|text| text.contains("PostMessage failed ; is the messages queue full?"))
        {
            return;
        }
        let details = PanicDetails {
            parent_pid: std::process::id(),
            payload: report.payload.map(|text| text.chars().take(8192).collect()),
            location: report.location.clone(),
            backtrace: report.backtrace.to_string().chars().take(500_000).collect(),
        };
        if let Err(error) = spawn_from_hook(&details) {
            eprintln!("cannot spawn independent panic reporter: {error:#}");
            crate::utils::dialog::error_dialog(format_report(&details));
        }
        // An unwinding Tauri event loop cannot service an async exit request.
        // The subprocess owns presentation; terminate the panicking process.
        std::process::exit(1);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upstream_panic_detail_wire_without_starting_gui() {
        let report = PanicDetails {
            parent_pid: 32,
            payload: Some("unexpected state".into()),
            location: Some("src/main.rs:20".into()),
            backtrace: "0: 0x123".into(),
        };
        let json = serde_json::to_vec(&report).unwrap();
        read_and_show(json.as_slice(), |decoded| {
            assert_eq!(decoded.parent_pid, 32);
            assert_eq!(decoded.payload.as_deref(), Some("unexpected state"));
            assert!(format_report(&decoded).contains("0: 0x123"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn malformed_and_oversized_panic_reports_fail_before_display() {
        assert!(
            read_and_show(b"invalid".as_slice(), |_| {
                panic!("a malformed input must never display a window");
            })
            .is_err()
        );
        assert!(
            read_and_show(vec![b'x'; MAX_REPORT_BYTES as usize + 1].as_slice(), |_| {
                panic!("oversized input must never display a window");
            })
            .is_err()
        );
    }
}
