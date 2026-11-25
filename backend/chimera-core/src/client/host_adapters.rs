//! Host-provided adapters consumed while assembling the application facade.
use super::{core_spec::CoreSpecResolver, reconciler::ports::ScriptRunnerFactory};
use crate::state::profiles::ports::{
    ProfileFsPort, ProfileMaterializationPort, SubscriptionFetcher,
};
use chimera_contracts::net::{ChangelogHttp, IconFetcher, NetworkDiagnostics, SelfProxyPortSource};
use std::sync::Arc;

/// Matching profile-store capabilities returned by the host's profile factory.
pub struct ProfileFilePorts {
    /// Filesystem access shared by the profiles owner and facade.
    pub fs: Arc<dyn ProfileFsPort>,
    /// Subscription downloading supplied to the profiles owner.
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    /// Transactional file materialization supplied to the profiles owner.
    pub materialization: Arc<dyn ProfileMaterializationPort>,
}

/// Builds matching host profile-store ports after session setup and before profiles startup.
pub type ProfileFileFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> ProfileFilePorts + Send + Sync;

/// Builds host changelog HTTP access from the session proxy source after app-updater startup.
pub type ChangelogHttpFactory =
    dyn Fn(Arc<dyn SelfProxyPortSource>) -> Arc<dyn ChangelogHttp> + Send + Sync;

/// Host-built capabilities and factories consumed during facade assembly.
/// The runtime build port is constructed with the committed profile reader
/// and runtime paths; the host supplies only the binary and script factories.
pub struct ClientHostAdapters {
    pub icon_fetcher: Arc<dyn IconFetcher>,
    pub network_diagnostics: Arc<dyn NetworkDiagnostics>,
    pub core_specs: Arc<CoreSpecResolver>,
    pub scripts: Arc<ScriptRunnerFactory>,
    pub profile_files: Arc<ProfileFileFactory>,
    pub changelog_http: Arc<ChangelogHttpFactory>,
}
