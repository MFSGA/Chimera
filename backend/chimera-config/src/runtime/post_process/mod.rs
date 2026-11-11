//! Declarative ownership of runtime config fields.

mod apply;
mod ownership;

pub use apply::{apply_defaults, apply_settings, keep_inherited};
pub use ownership::{OwnedField, OwnedFields, Ownership, OwnershipEntries};
