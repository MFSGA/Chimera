//! Network automation and network probes.

use chimera_config::application::IpLookupProviderInfo;

#[chimera_rpc::rpc(query, expose = [http])]
pub fn get_ip_lookup_providers() -> Vec<IpLookupProviderInfo> {
    chimera_config::application::IpLookupProvider::ALL
        .into_iter()
        .map(Into::into)
        .collect()
}
