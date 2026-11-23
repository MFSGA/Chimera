//! Standalone startup-failure reporter entry, modelled on ref's
//! `cmds/startup_failed.rs`. A reporter process reads a structured JSON
//! report until EOF and displays it without constructing Tauri owners.
use std::{
    ffi::OsStr,
    io::{self, Read as _, Write as _},
    process::{Command, ExitCode, Stdio},
};

use anyhow::{Context as _, bail};

use crate::startup_failure::{self, Report};

const REPORTER_FLAG: &str = "--chimera-internal-startup-failed";
const MAX_INPUT_BYTES: u64 = 1024 * 1024;

/// This dispatch must run before `chimera_lib::run()`, and hence before the
/// Tauri application, single-instance hook, windows or client are started.
pub fn maybe_run_from_command_line() -> Option<ExitCode> {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(OsStr::new(REPORTER_FLAG)) {
        return None;
    }
    if args.next().is_some() {
        eprintln!("startup failure reporter accepts no additional arguments");
        return Some(ExitCode::FAILURE);
    }
    Some(match read_and_display(io::stdin().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("failed to display Chimera startup failure: {error:#}");
            ExitCode::FAILURE
        }
    })
}

/// The same serialized Report is used by the existing in-process fallback.
/// Limit input before deserialization to avoid unbounded memory usage in an
/// independently launched process.
fn read_and_display(input: impl io::Read) -> anyhow::Result<()> {
    read_and_show(input, |report| {
        crate::utils::dialog::error_dialog(startup_failure::report_message(&report));
        Ok(())
    })
}

/// Keep the parser independent of GUI construction, as upstream's
/// `cmds/startup_failed::read_and_show` does.
fn read_and_show(
    mut input: impl io::Read,
    show: impl FnOnce(Report) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let mut json = Vec::new();
    input
        .by_ref()
        .take(MAX_INPUT_BYTES + 1)
        .read_to_end(&mut json)
        .context("failed to read startup failure from stdin")?;
    if json.len() as u64 > MAX_INPUT_BYTES {
        bail!("startup failure report exceeds the maximum permitted size");
    }
    let report: Report =
        serde_json::from_slice(&json).context("failed to decode startup failure report")?;
    show(report)
}

/// Wait until the independent reporter dialog is dismissed, as upstream does.
/// Close stdin before waiting or the child will wait forever for EOF. This
/// process never relaunches migration or opens application databases.
pub(crate) fn show_in_subprocess(report: &Report) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(report).context("failed to encode startup failure")?;
    if payload.len() as u64 > MAX_INPUT_BYTES {
        bail!("startup failure report exceeds the maximum permitted size");
    }
    let exe = std::env::current_exe().context("failed to locate reporter executable")?;
    let mut child = Command::new(exe)
        .arg(REPORTER_FLAG)
        .stdin(Stdio::piped())
        .spawn()
        .context("failed to spawn startup failure reporter")?;
    let write_result = (|| -> anyhow::Result<()> {
        let mut input = child
            .stdin
            .take()
            .context("startup failure reporter stdin was not piped")?;
        input
            .write_all(&payload)
            .context("failed to transmit startup failure report")
    })();
    // `stdin` is dropped by the closure before waiting for the child.
    let status = child
        .wait()
        .context("failed to wait for startup reporter")?;
    write_result?;
    if !status.success() {
        bail!("startup reporter exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup_failure::{FailureKind, StartupFailure, StartupStage};
    use std::path::PathBuf;

    fn sample() -> Report {
        Report {
            failure: StartupFailure {
                stage: StartupStage::MigrationRun,
                kind: FailureKind::NewerVersion,
                path: Some(PathBuf::from("/isolated/config/profiles.yaml")),
                detail: "future document schema".to_owned(),
                revision: None,
            },
            version: "Chimera test build".to_owned(),
            logs_dir: PathBuf::from("/isolated/logs"),
            backups_dir: None,
        }
    }

    #[test]
    fn reports_structured_stdin_until_eof_without_tauri_or_gui() {
        let json = serde_json::to_vec(&sample()).unwrap();
        let mut shown = false;
        read_and_show(json.as_slice(), |received| {
            shown = true;
            assert_eq!(received, sample());
            let message = startup_failure::report_message(&received);
            assert!(message.contains("future document schema"));
            Ok(())
        })
        .unwrap();
        assert!(shown);
    }

    #[test]
    fn rejects_malformed_or_oversized_stdin_without_showing_window() {
        let mut shown = false;
        let invalid = read_and_show(b"{invalid".as_slice(), |_| {
            shown = true;
            Ok(())
        });
        assert!(invalid.is_err());
        assert!(!shown);
        let oversized = vec![b'x'; MAX_INPUT_BYTES as usize + 1];
        let invalid = read_and_show(oversized.as_slice(), |_| {
            shown = true;
            Ok(())
        });
        assert!(invalid.is_err());
        assert!(!shown);
    }
}
