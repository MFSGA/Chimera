//! Shared RPC registry. Transport listeners remain separate.

extern crate self as chimera_rpc;
pub use chimera_rpc_macro::rpc;

/// Declares an event, HTTP route or stream for the resource table, in the namespace module it
/// belongs to: `resource!(event "name", targets = [desktop, http], sensitive,
/// capabilities(app_update))`. It reads its namespace's resource.
///
/// It expands to nothing. The resource table generator reads the declaration, checks it and
/// writes its row; admission looks the row up by kind and name.
#[macro_export]
macro_rules! resource {
    ($($declaration:tt)*) => {};
}

/// Declares the settings of the enclosing command module and everything below it:
/// `namespace!(expose = [http, local], sensitive)`.
///
/// A top-level module that declares it becomes a resource of the permission model and its
/// commands derive their requirement from their position (design section 3.2 of the permission
/// namespace spec): the effect decides the level unless a command writes `access = ...`. A
/// top-level module must name its exposure; a module below may only narrow it, and `sensitive`
/// marks the whole subtree. It expands to nothing; the resource table generator reads it.
#[macro_export]
macro_rules! namespace {
    ($($settings:tt)*) => {};
}
pub mod commands;
pub mod error;

pub mod audit;
pub mod events;
pub mod icons;
pub mod permission;
pub mod ports;
pub mod redaction;
pub mod registry;

pub mod privacy;
