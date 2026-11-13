//! Shared values and ports for consumers and implementations outside the application core.

pub mod bundle_metadata;
pub mod core_version;
pub mod elevation;
pub mod net;
pub mod service;
pub mod system_dns;
pub mod system_proxy;

#[cfg(test)]
mod os_port_contract_tests;
