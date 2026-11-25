//! System DNS cache flush through the injected OS port.

#[cfg(test)]
use chimera_contracts::system_dns::{SystemDnsCache, SystemDnsError};

#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct NoopSystemDnsCache;

#[cfg(test)]
impl SystemDnsCache for NoopSystemDnsCache {
    fn flush(&self) -> Result<(), SystemDnsError> {
        Ok(())
    }
}
