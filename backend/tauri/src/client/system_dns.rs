//! System DNS cache flush through the injected OS port.

use anyhow::Context as _;
#[cfg(test)]
use chimera_contracts::system_dns::{SystemDnsCache, SystemDnsError};

use super::ChimeraClient;

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct NoopSystemDnsCache;

#[cfg(test)]
impl SystemDnsCache for NoopSystemDnsCache {
    fn flush(&self) -> Result<(), SystemDnsError> {
        Ok(())
    }
}

impl ChimeraClient {
    pub(crate) async fn flush_system_dns_cache(&self) -> anyhow::Result<()> {
        let system_dns = self.inner.system_dns.clone();
        tokio::task::spawn_blocking(move || system_dns.flush())
            .await
            .context("system DNS cache flush task failed")??;
        Ok(())
    }
}
