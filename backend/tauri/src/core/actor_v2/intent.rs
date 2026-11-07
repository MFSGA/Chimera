//! Compatibility re-export of the ref-shaped core-owned runtime intent.
//! The to-be-committed document is already serialized, never reconstructed
//! or reserialized by this Tauri adapter.
pub use chimera_core::actor_v2::intent::{RuntimeIntent, RuntimeIntentBuilder};
