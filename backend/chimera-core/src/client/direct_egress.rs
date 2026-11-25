//! Explicit, consent-gated direct-egress discovery using the shared probe port.
use super::traffic::LocalSourceCache;
use crate::geo::GeoIndexClient;
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, TransparentProxyMode},
};
use chimera_contracts::net::{DirectEgress, DirectEgressProbe, EgressAddress, EgressAnswer};
use std::{net::IpAddr, sync::Arc};

pub fn blocked(app: &ChimeraAppConfig, clash: &ClashConfig) -> Option<DirectEgress> {
    if !app.enable_local_ip_probe {
        Some(DirectEgress::Disabled)
    } else if clash.enable_tun_mode {
        Some(DirectEgress::TunEnabled)
    } else if clash.transparent_proxy.local
        && clash.transparent_proxy.mode != TransparentProxyMode::Disabled
    {
        Some(DirectEgress::TransparentProxyEnabled)
    } else {
        None
    }
}

fn locate(geo: Option<&GeoIndexClient>, answer: EgressAnswer<impl Into<IpAddr>>) -> EgressAddress {
    let ip = answer.address.into();
    let country = geo.and_then(|geo| {
        let published = geo.subscribe();
        let current = published.borrow();
        current
            .as_ref()?
            .index
            .lookup(ip)?
            .country()
            .map(str::to_ascii_uppercase)
    });
    EgressAddress {
        address: ip.to_string(),
        country,
        source: answer.source,
    }
}

/// Called only on explicit request. Each destination checks live consent.
pub async fn probe(
    cache: &Arc<LocalSourceCache>,
    probe: &dyn DirectEgressProbe,
    geo: Option<&GeoIndexClient>,
) -> DirectEgress {
    if let Some(reason) = cache.blocked() {
        return reason;
    }
    let source_versions = cache.source_versions();
    let sources = cache.probe_sources();
    let (ipv4, ipv6) = tokio::join!(probe.ipv4(&sources), probe.ipv6(&sources));
    if let Some(reason) = cache.blocked() {
        return reason;
    }
    let result = DirectEgress::Probed {
        ipv4: ipv4.map(|answer| locate(geo, answer)),
        ipv6: ipv6.map(|answer| locate(geo, answer)),
    };
    cache.set(&result, source_versions);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_is_disabled_and_capture_guards_take_priority() {
        let mut app = ChimeraAppConfig::default();
        let mut clash = ClashConfig::default();
        assert_eq!(blocked(&app, &clash), Some(DirectEgress::Disabled));
        app.enable_local_ip_probe = true;
        assert_eq!(blocked(&app, &clash), None);
        clash.enable_tun_mode = true;
        assert_eq!(blocked(&app, &clash), Some(DirectEgress::TunEnabled));
        clash.enable_tun_mode = false;
        clash.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        assert_eq!(
            blocked(&app, &clash),
            Some(DirectEgress::TransparentProxyEnabled)
        );
        clash.transparent_proxy.local = false;
        assert_eq!(blocked(&app, &clash), None);
    }

    #[tokio::test]
    async fn disabled_probe_never_calls_network_and_consent_changes_fence_cached_source() {
        use crate::client::{application::ApplicationClient, clash_config::ClashConfigClient};
        use crate::traffic::LocalSourceLocation;
        use chimera_config::application::{ChimeraAppConfigPatch, IpLookupProvider};
        use chimera_contracts::net::{EgressSource, MockDirectEgressProbe, ProbePermission};

        let app = ApplicationClient::static_state(ChimeraAppConfig::default());
        let clash = ClashConfigClient::legacy_with(ClashConfig::default()).unwrap();
        let cache = Arc::new(LocalSourceCache::new(app.clone(), clash));
        let unused_probe = MockDirectEgressProbe::new();
        assert_eq!(
            probe(&cache, &unused_probe, None).await,
            DirectEgress::Disabled
        );
        app.patch_typed(ChimeraAppConfigPatch {
            enable_local_ip_probe: Some(true),
            ip_lookup_providers: Some(vec![IpLookupProvider::Ipify]),
            ..Default::default()
        })
        .await
        .unwrap();
        let started = cache.source_versions();
        assert!(cache.allows_lookup(IpLookupProvider::Ipify));
        let mut mock = MockDirectEgressProbe::new();
        mock.expect_ipv4().once().returning(|sources| {
            assert!(sources.permission.allows_lookup(IpLookupProvider::Ipify));
            Some(EgressAnswer {
                address: "203.0.113.7".parse().unwrap(),
                source: EgressSource::Lookup {
                    provider: IpLookupProvider::Ipify,
                },
            })
        });
        mock.expect_ipv6().once().returning(|_| None);
        let result = probe(&cache, &mock, None).await;
        assert_eq!(cache.addresses().ipv4, Some("203.0.113.7".parse().unwrap()));
        app.patch_typed(ChimeraAppConfigPatch {
            enable_local_ip_probe: Some(false),
            ..Default::default()
        })
        .await
        .unwrap();
        assert!(!cache.allows_lookup(IpLookupProvider::Ipify));
        assert!(cache.addresses().ipv4.is_none());
        app.patch_typed(ChimeraAppConfigPatch {
            enable_local_ip_probe: Some(true),
            ..Default::default()
        })
        .await
        .unwrap();
        cache.set(&result, started);
        assert!(
            cache.addresses().ipv4.is_none(),
            "stale probe must not revive after re-enabling consent"
        );
    }
}
