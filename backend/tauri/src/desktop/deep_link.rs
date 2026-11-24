//! Custom-scheme deep links: which launches carry one and how a link
//! reaches the frontend. Follows ref desktop/deep_link.rs, retaining the
//! Chimera scheme, pending-entry IPC and configured main/legacy window.

use tauri::{AppHandle, Emitter, Manager as _};
use url::Url;

use crate::{utils::resolve, window};

/// The scheme this app handles. The installers register the same list.
pub const SCHEMES: [&str; 1] = ["chimera"];

/// `url` when its scheme is one of [`SCHEMES`].
pub fn accepted(url: Url) -> Option<Url> {
    SCHEMES.contains(&url.scheme()).then_some(url)
}

/// Queues `url` for the frontend, pokes any listening frontend and shows
/// the configured application window. With no frontend listening yet the
/// poke is lost but the link is not: the frontend reads the pending queue
/// once it has registered its listener and claims each entry exactly once.
///
/// The queue is managed before the app runs. The window manager is not until
/// setup; an early link then waits for the next frontend to mount.
pub fn deliver(app_handle: &AppHandle, url: &Url) {
    // The URL itself can carry a subscription token, so only its scheme is logged.
    tracing::info!(scheme = url.scheme(), "deep link received");
    let entry = app_handle
        .state::<crate::ipc::PendingDeepLink>()
        .store(url.to_string());
    if app_handle.try_state::<window::WindowManager>().is_some() {
        let target_label = resolve::configured_window_label(app_handle);
        if let Err(error) = app_handle.emit_to(target_label, "scheme-request-received", entry) {
            tracing::error!(%error, "failed to emit scheme-request-received event");
        }
    }
    show_main_window(app_handle);
}

/// Shows and focuses the configured application window, building it when
/// it is closed. Before setup has managed the window manager there is no
/// window to show yet.
pub fn show_main_window(app_handle: &AppHandle) {
    let Some(_windows) = app_handle.try_state::<window::WindowManager>() else {
        return;
    };
    resolve::create_window(app_handle);
}

#[cfg(test)]
mod tests {
    use super::accepted;

    #[test]
    fn other_schemes_are_not_accepted() {
        assert!(accepted("chimera://install-config".parse().unwrap()).is_some());
        assert!(accepted("file:///etc/passwd".parse().unwrap()).is_none());
        assert!(accepted("https://example.com".parse().unwrap()).is_none());
        assert!(accepted("clash://install-config".parse().unwrap()).is_none());
    }
}
