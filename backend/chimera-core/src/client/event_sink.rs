//! Shared typed UI notifications.
use chimera_config::profile::ProfileId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateChanged {
    NyanpasuConfig,
    ClashConfig,
    Profiles { files: Vec<ProfileId> },
    Proxies,
    // Chimera-only runtime transform inspector; ref does not expose it.
    RuntimeTransformDiagnostics,
}

/// Transport-neutral notification of committed state. Presentation effects remain shell-owned.
pub trait UiEventSink: Send + Sync + 'static {
    fn state_changed(&self, state: StateChanged);
}
