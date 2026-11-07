//! Types shared across the instance-bound core API and Tauri transport.
//! The complete ApiClient lifecycle/endpoint ownership is a subsequent slice.

use clash_api::{IndexMap, ProviderName, Proxy, ProxyName, ProxyProvider};

/// A single instance-bound read of the proxy view, matching ref's actor API.
/// When `/group` is unavailable, its groups are inferred from `/proxies`.
pub struct ProxySnapshot {
    pub proxies: IndexMap<ProxyName, Proxy>,
    pub providers: IndexMap<ProviderName, ProxyProvider>,
    pub groups: Option<IndexMap<ProxyName, Proxy>>,
}
