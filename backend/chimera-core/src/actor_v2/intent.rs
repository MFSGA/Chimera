//! RuntimeIntentBuilder: a pure service deriving the portable reconcile
//! intent from an already-built runtime document. No I/O, no globals, no
//! implicit inputs — `RunType::default()`-style hidden state has no entry
//! here.
//!
//! Scope note: the snapshots → document merge itself remains the existing
//! enhance pipeline (already pure); the bridge stage composes the two. This
//! type owns the portable half: the to-be-committed text and its digest.

use std::sync::Arc;

use chimera_core_manager::payload_digest;
use chimera_utils::core::CoreType;

/// The portable convergence intent: everything a `Reconcile` envelope needs
/// that is not host-resolved (binary paths stay host-side).
///
/// Built once per candidate and then handed to both the advisory check and the
/// reconcile, so the two provably consume the identical to-be-committed bytes
/// (v2 T4). The CAS token is deliberately *not* here: it is read
/// authoritatively at admission time, long after these bytes exist.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeIntent {
    pub local_ipc: chimera_core_manager::LocalIpcSettings,
    pub core_type: CoreType,
    /// The full runtime config document, serialized.
    pub config_text: Arc<str>,
    /// [`payload_digest`] of `config_text` — the change identity the daemon
    /// verifies on receipt.
    pub digest: String,
}

pub struct RuntimeIntentBuilder;

impl RuntimeIntentBuilder {
    /// Deterministic: the same document and inputs produce the same intent,
    /// digest included.
    pub fn build(
        core_type: CoreType,
        config_text: Arc<str>,
        local_ipc: chimera_core_manager::LocalIpcSettings,
    ) -> RuntimeIntent {
        let digest = payload_digest(config_text.as_bytes());
        RuntimeIntent {
            local_ipc,
            core_type,
            config_text,
            digest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_utils::core::ClashCoreType;

    fn test_settings() -> chimera_core_manager::LocalIpcSettings {
        chimera_core_manager::LocalIpcSettings {
            policy: chimera_core_manager::LocalIpcPolicy::Prefer,
            keep_http_controller: true,
        }
    }
    fn document() -> Arc<str> {
        Arc::from("external-controller: 127.0.0.1:9090\n")
    }

    #[test]
    fn the_same_inputs_produce_the_same_intent() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let first = RuntimeIntentBuilder::build(core_type.clone(), document(), test_settings());
        let second = RuntimeIntentBuilder::build(core_type, document(), test_settings());
        assert_eq!(first, second);
        assert_eq!(first.digest, payload_digest(first.config_text.as_bytes()));
    }

    #[test]
    fn original_text_bytes_are_never_reserialized_between_check_and_reconcile() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let input: Arc<str> =
            Arc::from("mode: rule  # preserve comment\n  # indentation matters\n");
        let intent = RuntimeIntentBuilder::build(core_type, input.clone(), test_settings());
        assert!(Arc::ptr_eq(&input, &intent.config_text));
        assert_eq!(intent.config_text.as_ref(), input.as_ref());
        assert_eq!(intent.digest, payload_digest(input.as_bytes()));
    }

    #[test]
    fn a_document_change_changes_the_digest() {
        let core_type = CoreType::Clash(ClashCoreType::Mihomo);
        let base = RuntimeIntentBuilder::build(core_type.clone(), document(), test_settings());
        let changed = Arc::from(format!("{}mixed-port: 7890\n", document()));
        let next = RuntimeIntentBuilder::build(core_type, changed, test_settings());
        assert_ne!(base.digest, next.digest);
    }
}
