//! Core kinds, launch profiles, and config checking.

use std::{ffi::OsString, time::Duration};

use camino::Utf8Path;
use chimera_utils::process::ProcessError;

use crate::{
    error::Error,
    log::{CapturedOutput, summarize_output},
};

pub use chimera_core_metadata::ClashCoreKind as CoreKind;

/// The environment variable Mihomo consults for permitted file-system roots.
pub const MIHOMO_SAFE_PATHS_ENV_NAME: &str = "SAFE_PATHS";

/// Logrus honours this when `EnvironmentOverrideColors` is set, which Mihomo
/// does (`log/log.go`). Pinning it to `0` keeps the logfmt layout the log parser
/// expects even when the service inherits a colour-forcing environment.
pub(crate) const CLICOLOR_FORCE_ENV_NAME: &str = "CLICOLOR_FORCE";

#[cfg(windows)]
const SAFE_PATHS_SEPARATOR: &str = ";";
#[cfg(not(windows))]
const SAFE_PATHS_SEPARATOR: &str = ":";

/// The two paths every core is launched with. They are both `Utf8Path` and
/// were adjacent parameters, so a transposed call produces a well-formed
/// command line that names the config as the working directory.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CorePaths<'a> {
    pub working_dir: &'a Utf8Path,
    pub config_path: &'a Utf8Path,
}

/// Launch arguments for this kind.
pub(crate) fn run_args(kind: CoreKind, paths: CorePaths<'_>) -> Result<Vec<OsString>, Error> {
    let dir = OsString::from(paths.working_dir.as_str());
    let cfg = OsString::from(paths.config_path.as_str());
    Ok(match kind {
        // Meow accepts the mihomo CLI for compatibility.
        CoreKind::Mihomo | CoreKind::Meow => {
            vec!["-m".into(), "-d".into(), dir, "-f".into(), cfg]
        }
        // Chimera Client retains the clash-rs `-c` CLI while keeping its own
        // kind for identity, reporting, and core selection.
        CoreKind::ClashRust | CoreKind::ChimeraClient => {
            vec!["-d".into(), dir, "-c".into(), cfg]
        }
        CoreKind::ClashPremium => vec!["-d".into(), dir, "-f".into(), cfg],
    })
}

/// Extra launch flags to enable the controller for kinds that cannot take it
/// from the config file.
///
/// clash-bin unconditionally overwrites the config's `external_controller_ipc`
/// with its CLI flag value (clash-bin/src/main.rs), so for clash-rs and
/// Chimera Client a system IPC endpoint only takes effect when passed as
/// `--controller-ipc`.
pub(crate) fn controller_args(kind: CoreKind, host: &clash_api::Host) -> Vec<OsString> {
    if !matches!(kind, CoreKind::ClashRust | CoreKind::ChimeraClient) {
        return Vec::new();
    }
    match host {
        clash_api::Host::NamedPipe(path) | clash_api::Host::UnixSocket(path) => {
            vec!["--controller-ipc".into(), path.as_os_str().to_owned()]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use camino::Utf8Path;

    use super::{CoreKind, CorePaths, check_args, controller_args, run_args};

    fn strings(args: Vec<OsString>) -> Vec<String> {
        args.into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    fn paths() -> CorePaths<'static> {
        CorePaths {
            working_dir: Utf8Path::new("/tmp/chimera-client"),
            config_path: Utf8Path::new("/tmp/chimera-client/config.yaml"),
        }
    }

    #[test]
    fn chimera_client_keeps_brand_identity_and_uses_its_supported_cli() {
        // Contract: the Chimera Client kind must keep its wire identity while
        // producing the flags accepted by clash-bin/src/main.rs. Collapsing it
        // into ClashRust or using another core's config flag fails these checks.
        assert_eq!(CoreKind::ChimeraClient.as_ref(), "chimera-client");
        assert_ne!(CoreKind::ChimeraClient, CoreKind::ClashRust);
        assert_eq!(
            strings(run_args(CoreKind::ChimeraClient, paths()).unwrap()),
            vec![
                "-d".to_owned(),
                "/tmp/chimera-client".to_owned(),
                "-c".to_owned(),
                "/tmp/chimera-client/config.yaml".to_owned(),
            ]
        );
        // Chimera Client exposes `-f` as a compatibility alias for `-c`.
        assert_eq!(
            strings(check_args(paths())),
            vec![
                "-t".to_owned(),
                "-d".to_owned(),
                "/tmp/chimera-client".to_owned(),
                "-f".to_owned(),
                "/tmp/chimera-client/config.yaml".to_owned(),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn chimera_client_receives_local_ipc_through_its_cli_flag() {
        // The independent observation is the Chimera Client CLI contract:
        // `--controller-ipc` overrides `external_controller_ipc` at startup.
        let args = controller_args(
            CoreKind::ChimeraClient,
            &clash_api::Host::unix_socket("/tmp/chimera-client.sock"),
        );
        assert_eq!(
            strings(args),
            vec![
                "--controller-ipc".to_owned(),
                "/tmp/chimera-client.sock".to_owned(),
            ]
        );
    }
}

/// Arguments for a one-shot `-t` config validation run (same for all kinds,
/// matching the legacy `check_config_`).
pub(crate) fn check_args(paths: CorePaths<'_>) -> Vec<OsString> {
    vec![
        "-t".into(),
        "-d".into(),
        paths.working_dir.as_str().into(),
        "-f".into(),
        paths.config_path.as_str().into(),
    ]
}

/// Joins the directories Mihomo may touch into its `SAFE_PATHS` format.
pub fn mihomo_safe_paths(working_dir: &Utf8Path, config_dir: &Utf8Path) -> String {
    [working_dir.as_str(), config_dir.as_str()].join(SAFE_PATHS_SEPARATOR)
}

/// Upper bound on a single `-t` validation run.
///
/// A `-t` run parses a config and exits; it never serves traffic, so a core that
/// has not answered in this long is wedged, not slow. The bound covers every
/// caller — `/core/check`, the staged check inside `apply_config`
/// (`manager/apply.rs:174`) and the pre-flight checks on the start and switch
/// paths (`manager/switching.rs:431,492,496`, which run while the control lock
/// is held) — and sits far enough under the service's 120s request bound
/// (`routing/middleware.rs:31`) that the caller receives this error rather than
/// a dropped future.
pub const CHECK_CONFIG_TIMEOUT: Duration = Duration::from_secs(30);

/// One-shot `-t` config validation, replacing the legacy `check_config_`.
/// A non-zero exit becomes [`Error::ConfigCheckFailed`] with a condensed message,
/// and so does a run that exceeds [`CHECK_CONFIG_TIMEOUT`].
pub async fn check_config(spec: &crate::spec::InstanceSpec) -> Result<(), Error> {
    run_check(spec, CHECK_CONFIG_TIMEOUT).await
}

/// [`check_config`] with an explicit bound. Public only under `test-hooks`:
/// production has exactly one bound, and a test that had to wait
/// [`CHECK_CONFIG_TIMEOUT`] to observe the timeout would not be worth running.
#[cfg(feature = "test-hooks")]
pub async fn check_config_within(
    spec: &crate::spec::InstanceSpec,
    timeout: Duration,
) -> Result<(), Error> {
    run_check(spec, timeout).await
}

async fn run_check(spec: &crate::spec::InstanceSpec, timeout: Duration) -> Result<(), Error> {
    let config_dir = spec
        .config_path
        .parent()
        .ok_or_else(|| Error::ConfigNotFound(spec.config_path.clone()))?;
    let output = chimera_utils::process::Command::new(spec.core.binary_path.as_str())
        .args(check_args(spec.core_paths()))
        .env(
            MIHOMO_SAFE_PATHS_ENV_NAME,
            mihomo_safe_paths(&spec.working_dir, config_dir),
        )
        .env(CLICOLOR_FORCE_ENV_NAME, "0")
        .timeout(timeout)
        .output()
        .await
        .map_err(|error| match error {
            // The process tree is already killed by the time this arrives
            // (`Command::timeout`). Reported as a check failure rather than a
            // process error because that is what the caller asked about: the
            // config did not validate. Same wire kind (`config_check_failed`),
            // and the message names the bound so a slow core is diagnosable.
            ProcessError::Timeout { after } => {
                Error::ConfigCheckFailed(format!("config check timed out after {after:?}"))
            }
            other => Error::Process(other),
        })?;
    if output.success() {
        return Ok(());
    }
    Err(Error::ConfigCheckFailed(summarize_output(
        spec.core.kind,
        CapturedOutput {
            stdout: &output.stdout,
            stderr: &output.stderr,
        },
    )))
}
