use super::super::runtime;
use chimera_contracts::runtime_error::RuntimeCheckUnavailable;

use super::inputs::FrozenProfileContent;
use async_trait::async_trait;
use chimera_config::{
    profile::Profiles,
    runtime::executor::{PortError, ProfileContentSource, ScriptRunner},
};
use chimera_contracts::publish::PublishRuntimeError;
use chimera_core::client::core_spec::CoreSpecError;
use chimera_core::client::runtime_build::RuntimeBuildError;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Builds a script runner on the blocking thread that will use and drop it.
pub type ScriptRunnerFactory = dyn Fn() -> Result<Box<dyn ScriptRunner>, PortError> + Send + Sync;

#[async_trait]
pub trait RuntimeBuildPort: Send + Sync + 'static {
    async fn capture_content(
        &self,
        profiles: &chimera_config::profile::Profiles,
    ) -> super::inputs::FrozenProfileContent;
    fn core_spec(
        &self,
        core: &chimera_config::application::ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, CoreSpecError>;
    /// Builds a candidate runtime from explicit inputs. The resolved ports are
    /// one of them: resolving inside the build would hide a probe that races
    /// the running core and would tie the build to a shared port cache.
    async fn build(
        &self,
        revision: runtime::RuntimeRevision,
        inputs: super::inputs::RuntimeInputs,
        ports: chimera_config::runtime::executor::ResolvedPortBindings,
        strict_transforms: bool,
    ) -> Result<Arc<runtime::RuntimeSnapshot>, RuntimeBuildError>;
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
}

/// One advisory check of a candidate runtime. The document is the built
/// `RuntimeIntent`, never a re-serialization: the check and the reconcile have
/// to consume the identical to-be-committed bytes.
pub(in crate::client) struct RuntimeCheckRequest<'a> {
    pub core_spec: chimera_core_manager::CoreSpec,
    pub intent: &'a chimera_core::actor_v2::intent::RuntimeIntent,
}

/// What the core said about a candidate document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::client) enum RuntimeCheckOutcome {
    /// The core's own validation accepted the document.
    Passed,
    /// The core rejected it. Deterministic: the same bytes fail again.
    Rejected {
        kind: Option<chimera_core_manager::CoreErrorKind>,
        message: String,
    },
    Unavailable(RuntimeCheckUnavailable),
}

/// The check capability, as the application consumes it.
#[async_trait]
pub(in crate::client) trait RuntimeValidatorPort: Send + Sync + 'static {
    async fn check(&self, request: RuntimeCheckRequest<'_>) -> RuntimeCheckOutcome;
}

// Chimera temporary compatibility: the active CoreLifecycle still asks its
// injected filesystem content source to capture staged bytes. Remove after
// RuntimeBuildPort takes ownership of the candidate build.
/// Capture the active managed-file dependency closure from one supplied
/// content source. A staged candidate may replace bytes it explicitly owns.
/// File reads are fenced by RuntimePreparation, not by this port.
#[async_trait::async_trait]
pub(in crate::client) trait RuntimeContentPort: Send + Sync {
    async fn capture_content(
        &self,
        profiles: &Profiles,
        staged_content: BTreeMap<String, String>,
    ) -> FrozenProfileContent;
}

#[async_trait::async_trait]
impl<T> RuntimeContentPort for T
where
    T: ProfileContentSource + Send + Sync,
{
    async fn capture_content(
        &self,
        profiles: &Profiles,
        staged_content: BTreeMap<String, String>,
    ) -> FrozenProfileContent {
        FrozenProfileContent::capture_content(profiles, self, staged_content)
    }
}
