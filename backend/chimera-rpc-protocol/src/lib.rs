//! Neutral RPC envelopes, caller labels and shared event identities.
//! Typed operations, receiving projections and the typed client remain pending
//! until the runtime-value contracts and Runtime IPC dependencies are aligned.

mod command;
mod error;
mod event;
mod owner;
mod request;

pub use command::{CommandKind, ShellCapability};
pub use error::{ResponseVisibility, RpcError, RpcValue};
pub use event::EventName;
/// Canonical authentication types shared across core and transports.
pub use chimera_contracts::auth::{AuthContext, Principal};
pub use chimera_rpc_wire::{
    CommandEffect, DescribedCommand, RpcDescribeResponse, RpcTarget, ServerInfo, ServerKind,
};
pub use owner::{ClientNonce, ClientNonceError, MAX_CLIENT_NONCE_LEN, RpcOwner};
pub use request::Request;

pub use chimera_rpc_wire::frame;
pub mod icons;
pub mod operation;
pub mod permission;
pub mod session;
pub mod values;

#[cfg(test)]
mod frame_tests;
