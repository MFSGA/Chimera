//! Shared RPC registry. Transport listeners remain separate.

extern crate self as chimera_rpc;
pub use chimera_rpc_macro::rpc;
pub mod commands;
pub mod error;

pub mod audit;
pub mod events;
pub mod permission;
pub mod redaction;
pub mod registry;

pub mod privacy;
