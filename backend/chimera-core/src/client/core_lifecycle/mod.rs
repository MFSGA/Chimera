//! Shared lower lifecycle policy; it never owns an actor or Core receipt.
pub mod workflow;
pub const RECOVERY_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

pub mod ports;

pub mod apply;

pub mod status;

pub mod ownership;
pub mod service;
