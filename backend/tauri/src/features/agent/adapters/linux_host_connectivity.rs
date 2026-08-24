use std::{
    net::{Ipv4Addr, Ipv6Addr},
    os::fd::AsRawFd,
    sync::Arc,
    time::Duration,
};

use nix::{
    ifaddrs::getifaddrs,
    net::if_::{InterfaceFlags, if_nametoindex},
    sys::{
        socket::{
            AddressFamily, MsgFlags, NetlinkAddr, SockFlag, SockProtocol, SockType, bind, recv,
            sendto, setsockopt, socket, sockopt,
        },
        time::{TimeVal, TimeValLike},
    },
};

use super::{
    super::{
        host_connectivity::{HostConnectivityEvidence, diagnose_host_connectivity},
        model::AgentHostConnectivitySnapshot,
        ports::HostConnectivityPort,
    },
    linux_host_connectivity_core::{
        LinuxConnectivityCollector, LinuxConnectivityEvidenceError, LinuxHostConnectivityCore,
        MAX_INTERFACE_ROWS, ROUTE_DUMP_SEQUENCE, RouteDumpParser, SanitizedAddressFamily,
        SanitizedInterfaceRow, merge_linux_evidence, route_dump_request,
    },
};

const HOST_CONNECTIVITY_TIMEOUT: Duration = Duration::from_millis(500);
const NETLINK_RECEIVE_TIMEOUT_MILLIS: i64 = 100;
const NETLINK_BUFFER_BYTES: usize = 16 * 1024;
const MAX_NETLINK_RECEIVES: usize = 8;

struct NativeLinuxConnectivityCollector;

#[async_trait::async_trait]
impl LinuxConnectivityCollector for NativeLinuxConnectivityCollector {
    async fn collect(&self) -> AgentHostConnectivitySnapshot {
        let evidence = tokio::task::spawn_blocking(collect_linux_evidence)
            .await
            .ok()
            .and_then(Result::ok);
        evidence
            .map(diagnose_host_connectivity)
            .unwrap_or_else(unavailable_snapshot)
    }
}

pub(crate) struct LinuxHostConnectivity {
    core: LinuxHostConnectivityCore,
}

impl LinuxHostConnectivity {
    pub(crate) fn new() -> Self {
        Self {
            core: LinuxHostConnectivityCore::new(
                Arc::new(NativeLinuxConnectivityCollector),
                HOST_CONNECTIVITY_TIMEOUT,
            ),
        }
    }
}

#[async_trait::async_trait]
impl HostConnectivityPort for LinuxHostConnectivity {
    async fn snapshot(&self) -> AgentHostConnectivitySnapshot {
        self.core.snapshot().await
    }
}

fn collect_linux_evidence() -> Result<HostConnectivityEvidence, LinuxConnectivityEvidenceError> {
    let interface_rows = collect_interface_rows()?;
    let route_rows = collect_route_rows()?;
    merge_linux_evidence(&interface_rows, &route_rows)
}

fn collect_interface_rows() -> Result<Vec<SanitizedInterfaceRow>, LinuxConnectivityEvidenceError> {
    let interfaces = getifaddrs().map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
    let mut rows = Vec::new();
    let mut visited = 0_usize;
    for interface in interfaces {
        visited += 1;
        if visited > MAX_INTERFACE_ROWS {
            return Err(LinuxConnectivityEvidenceError::BudgetExceeded);
        }
        if interface.flags.contains(InterfaceFlags::IFF_LOOPBACK) {
            continue;
        }
        let interface_index = if_nametoindex(interface.interface_name.as_str())
            .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
        if interface_index == 0 {
            return Err(LinuxConnectivityEvidenceError::Malformed);
        }
        let up = interface.flags.contains(InterfaceFlags::IFF_UP)
            && interface.flags.contains(InterfaceFlags::IFF_RUNNING);
        let (family, usable_address) = match interface.address {
            Some(address) => {
                if let Some(address) = address.as_sockaddr_in() {
                    (
                        Some(SanitizedAddressFamily::Ipv4),
                        usable_ipv4(address.ip()),
                    )
                } else if let Some(address) = address.as_sockaddr_in6() {
                    (
                        Some(SanitizedAddressFamily::Ipv6),
                        usable_ipv6(address.ip()),
                    )
                } else {
                    (None, false)
                }
            }
            None => (None, false),
        };
        rows.push(SanitizedInterfaceRow {
            interface_index,
            up,
            family,
            usable_address,
        });
    }
    Ok(rows)
}

fn collect_route_rows() -> Result<
    Vec<super::linux_host_connectivity_core::SanitizedRouteRow>,
    LinuxConnectivityEvidenceError,
> {
    let socket = socket(
        AddressFamily::Netlink,
        SockType::Raw,
        SockFlag::SOCK_CLOEXEC,
        SockProtocol::NetlinkRoute,
    )
    .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
    setsockopt(
        &socket,
        sockopt::ReceiveTimeout,
        &TimeVal::milliseconds(NETLINK_RECEIVE_TIMEOUT_MILLIS),
    )
    .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
    bind(socket.as_raw_fd(), &NetlinkAddr::new(0, 0))
        .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;

    let request = route_dump_request(ROUTE_DUMP_SEQUENCE);
    let sent = sendto(
        socket.as_raw_fd(),
        &request,
        &NetlinkAddr::new(0, 0),
        MsgFlags::empty(),
    )
    .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
    if sent != request.len() {
        return Err(LinuxConnectivityEvidenceError::Incomplete);
    }

    let mut parser = RouteDumpParser::new(ROUTE_DUMP_SEQUENCE);
    let mut buffer = [0_u8; NETLINK_BUFFER_BYTES];
    for _ in 0..MAX_NETLINK_RECEIVES {
        let received = recv(socket.as_raw_fd(), &mut buffer, MsgFlags::empty())
            .map_err(|_| LinuxConnectivityEvidenceError::NativeUnavailable)?;
        if received == 0 {
            return Err(LinuxConnectivityEvidenceError::Incomplete);
        }
        if parser.push_chunk(&buffer[..received])? {
            return parser.finish();
        }
    }
    Err(LinuxConnectivityEvidenceError::BudgetExceeded)
}

fn usable_ipv4(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_multicast()
        && !address.is_broadcast()
}

fn usable_ipv6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && first & 0xffc0 != 0xfe80
}

fn unavailable_snapshot() -> AgentHostConnectivitySnapshot {
    diagnose_host_connectivity(HostConnectivityEvidence::default())
}

#[cfg(test)]
mod tests {
    use super::{LinuxHostConnectivity, collect_interface_rows, usable_ipv4, usable_ipv6};
    use crate::features::agent::ports::HostConnectivityPort;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn native_interface_rows_are_bounded_and_contain_no_raw_identifiers() {
        if let Ok(rows) = collect_interface_rows() {
            assert!(rows.len() <= super::MAX_INTERFACE_ROWS);
            assert!(rows.iter().all(|row| row.interface_index != 0));
        }
    }

    #[test]
    fn usable_address_filter_rejects_loopback_link_local_and_multicast() {
        assert!(!usable_ipv4(Ipv4Addr::LOCALHOST));
        assert!(!usable_ipv4(Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!usable_ipv4(Ipv4Addr::new(224, 0, 0, 1)));
        assert!(usable_ipv4(Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!usable_ipv6(Ipv6Addr::LOCALHOST));
        assert!(!usable_ipv6("fe80::1".parse().unwrap()));
        assert!(!usable_ipv6("ff02::1".parse().unwrap()));
        assert!(usable_ipv6("fd00::1".parse().unwrap()));
    }

    #[tokio::test]
    async fn native_snapshot_exposes_only_the_closed_connectivity_contract() {
        let snapshot = LinuxHostConnectivity::new().snapshot().await;
        let serialized = serde_json::to_value(snapshot).expect("serialize closed snapshot");
        let object = serialized.as_object().expect("snapshot object");
        assert_eq!(
            object
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            [
                "active_interface_kind",
                "captive_portal_suspected",
                "dns_configured",
                "dns_resolves",
                "ipv4",
                "ipv6",
                "link_up",
                "reasons",
                "status",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect()
        );
        let text = serialized.to_string().to_ascii_lowercase();
        for forbidden in [
            "interface_name",
            "gateway",
            "mac",
            "address",
            "target",
            "error",
            "route_table",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
}
