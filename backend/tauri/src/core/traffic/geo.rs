//! Uses GeoIP labels already attached to the core snapshot.
use std::net::IpAddr;

use chimera_clash_api::{ConfigEnum, ConnectionMetadata, ConnectionNetwork, DnsMode};
use chimera_traffic::{GeoBasis, normalize_region};

use super::LocalSourceIps;

const UNKNOWN: &str = "unknown";
const DIRECT: &str = "DIRECT";

pub(crate) fn locate_source(meta: &ConnectionMetadata, _local: LocalSourceIps) -> String {
    let source = meta.source_ip.as_deref().unwrap_or_default();
    if source.eq_ignore_ascii_case("local")
        || source.eq_ignore_ascii_case("lan")
        || source.parse::<IpAddr>().is_ok_and(is_local)
    {
        return UNKNOWN.to_owned();
    }
    normalize_region(meta.source_geo_ip.iter().flatten())
}

pub(crate) fn locate_destination(
    meta: &ConnectionMetadata,
    exit: Option<&str>,
) -> (String, Option<GeoBasis>) {
    let region = normalize_region(meta.destination_geo_ip.iter().flatten());
    let basis = (region != UNKNOWN).then(|| destination_basis(meta, exit));
    (region, basis)
}

fn destination_basis(meta: &ConnectionMetadata, exit: Option<&str>) -> GeoBasis {
    let tcp = matches!(
        meta.network,
        Some(ConfigEnum::Known(ConnectionNetwork::Tcp))
    );
    let hosts = matches!(meta.dns_mode, Some(ConfigEnum::Known(DnsMode::Hosts)));
    let no_host = meta.host.as_deref().is_none_or(str::is_empty);
    if (exit == Some(DIRECT) && !tcp) || no_host || (tcp && hosts) {
        GeoBasis::Dialed
    } else {
        GeoBasis::Resolved
    }
}

fn is_local(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_unique_local()
                || ip.is_loopback()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    }
}
