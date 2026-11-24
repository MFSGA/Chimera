//! Host construction inputs for script capabilities.
use chimera_config::profile::ScriptRuntime;

/// A host can decline an engine; it cannot grant ambient filesystem/network
/// access or disable the sandbox limits through this capability boundary.
pub trait ScriptExtensionFactory: Send + Sync {
    fn allows(&self, runtime: ScriptRuntime) -> bool;
}

/// The desktop's existing sandboxed engine policy.
#[derive(Default)]
pub struct SandboxedExtensions;

impl ScriptExtensionFactory for SandboxedExtensions {
    fn allows(&self, _: ScriptRuntime) -> bool {
        true
    }
}
