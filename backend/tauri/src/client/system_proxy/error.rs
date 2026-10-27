//! Why a system effect the actor owns did not reach its desired state.
//!
//! This is the one place a failure of the OS proxy, the PAC script or the
//! login item is classified: the actor builds the variant where it happens and
//! the degraded status derives its code, its retry flag and its text from it.

use snafu::Snafu;

use super::ports::{AutoLaunchError, OsProxyError, PacError};
use crate::client::effects::status::EffectHealth;

pub(super) fn failure_text(error: &(dyn std::error::Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

/// What happened to the plain proxy that a failed PAC transition falls back to.
#[derive(Debug)]
pub enum PacFallback {
    Installed,
    /// No port is resolved, so there is no proxy endpoint to install.
    SkippedNoPort,
    /// The shutdown began first; the restore owns the OS settings.
    SkippedShuttingDown,
    Failed(Box<SystemProxyError>),
}

impl std::fmt::Display for PacFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Installed => f.write_str("the direct proxy fallback is installed instead"),
            Self::SkippedNoPort => {
                f.write_str("the direct proxy fallback was skipped because no port is resolved")
            }
            Self::SkippedShuttingDown => {
                f.write_str("the direct proxy fallback was skipped because the app is exiting")
            }
            Self::Failed(error) => write!(
                f,
                "the direct proxy fallback failed too: {}",
                failure_text(error)
            ),
        }
    }
}

#[derive(Debug, Snafu)]
#[snafu(visibility(pub(super)))]
pub enum SystemProxyError {
    #[snafu(display("the session has not resolved a mixed port yet"))]
    PortUnresolved,
    #[snafu(display("the desired proxy has not been confirmed"))]
    GuardWaitingDependency,
    #[snafu(display("the system proxy was not applied"))]
    ApplyProxy { source: OsProxyError },
    #[snafu(display("the system proxy was not restored"))]
    RestoreProxy { source: OsProxyError },
    #[snafu(display("the system proxy did not match the restored settings"))]
    RestoreVerification,
    #[snafu(display("the system proxy changed during a host action"))]
    ProxyStateChanged,
    #[snafu(display("the system proxy state could not be verified before rollback"))]
    ProxyStateUnknown,
    #[snafu(display("the system proxy was not handed back from PAC"))]
    RestorePac { source: PacError },
    /// The PAC script was refused; the plain proxy stands in for it.
    #[snafu(display("could not apply the PAC script; {fallback}"))]
    ApplyPac {
        source: PacError,
        fallback: PacFallback,
    },
    /// The PAC script was refused and the one installed before it would not
    /// come off either, so the OS still resolves against the old one.
    #[snafu(display(
        "could not apply the PAC script, and the previous one could not be cleared either ({}); {fallback}",
        failure_text(stale)
    ))]
    ApplyPacKeepingStale {
        source: PacError,
        stale: Box<PacError>,
        fallback: PacFallback,
    },
    /// A PAC url the OS would not give up is still installed beside whatever
    /// was written, and the OS prefers it.
    #[snafu(display("{}", clear_pac_text(write.as_deref())))]
    ClearPac {
        source: PacError,
        write: Option<Box<SystemProxyError>>,
    },
    #[snafu(display("could not {} auto-launch", if *enabled { "enable" } else { "disable" }))]
    ApplyAutoLaunch {
        enabled: bool,
        source: AutoLaunchError,
    },
    #[snafu(display("the system proxy owner is shutting down and stopped accepting changes"))]
    ShutDown,
    #[snafu(display("the system proxy actor stopped before answering"))]
    Stopped,
}

fn clear_pac_text(write: Option<&SystemProxyError>) -> String {
    match write {
        Some(error) => format!(
            "the auto-config url could not be cleared, and the proxy write beside it failed too: {}",
            failure_text(error)
        ),
        None => "the auto-config url could not be cleared and is still installed".to_owned(),
    }
}

impl SystemProxyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::PortUnresolved => "system_proxy_port_unresolved",
            Self::GuardWaitingDependency => "proxy_guard_waiting_dependency",
            Self::ApplyProxy { .. } => "system_proxy_apply_failed",
            Self::RestoreProxy { .. } | Self::RestoreVerification | Self::RestorePac { .. } => {
                "system_proxy_restore_failed"
            }
            Self::ProxyStateChanged => "system_proxy_state_changed",
            Self::ProxyStateUnknown => "system_proxy_state_unknown",
            Self::ApplyPac { .. } => "pac_apply_failed",
            Self::ApplyPacKeepingStale { .. } | Self::ClearPac { .. } => "pac_disable_failed",
            Self::ApplyAutoLaunch { .. } => "auto_launch_failed",
            Self::ShutDown => "system_proxy_shutdown",
            Self::Stopped => "system_proxy_stopped",
        }
    }

    /// Nothing about the app's exit, or about an actor that is gone, changes
    /// on a retry; every other failure is worth the effects actor's budget.
    pub fn retryable(&self) -> bool {
        !matches!(
            self,
            Self::ShutDown | Self::Stopped | Self::ProxyStateChanged
        )
    }

    pub fn health(&self) -> EffectHealth {
        EffectHealth::Degraded {
            code: self.code(),
            message: failure_text(self),
            retryable: self.retryable(),
        }
    }
}
