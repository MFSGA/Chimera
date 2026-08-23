use super::model::{
    AgentHostConnectivityReason, AgentHostConnectivitySnapshot, AgentHostConnectivityStatus,
    AgentIpFamilyConnectivity, AgentNetworkInterfaceKind,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HostConnectivityEvidence {
    pub(crate) wireless_present: bool,
    pub(crate) wireless_connected: bool,
    pub(crate) ethernet_present: bool,
    pub(crate) ethernet_connected: bool,
    pub(crate) other_interface_connected: bool,
    pub(crate) ipv4_usable_address: bool,
    pub(crate) ipv4_default_route: bool,
    pub(crate) ipv4_internet_reachable: Option<bool>,
    pub(crate) ipv6_usable_address: bool,
    pub(crate) ipv6_default_route: bool,
    pub(crate) ipv6_internet_reachable: Option<bool>,
    pub(crate) dns_configured: Option<bool>,
    pub(crate) dns_resolves: Option<bool>,
    pub(crate) captive_portal_suspected: Option<bool>,
    pub(crate) probe_complete: bool,
}

pub(crate) fn unavailable_host_connectivity() -> AgentHostConnectivitySnapshot {
    diagnose_host_connectivity(HostConnectivityEvidence::default())
}

pub(crate) fn diagnose_host_connectivity(
    evidence: HostConnectivityEvidence,
) -> AgentHostConnectivitySnapshot {
    let interface_kind = active_interface_kind(evidence);
    let link_up = link_is_up(evidence);
    let mut reasons = connectivity_reasons(evidence, link_up);
    let status = connectivity_status(evidence, link_up, &mut reasons);

    AgentHostConnectivitySnapshot {
        status,
        active_interface_kind: interface_kind,
        link_up: evidence.probe_complete.then_some(link_up),
        ipv4: AgentIpFamilyConnectivity {
            usable_ip: evidence.ipv4_usable_address,
            default_route: evidence.ipv4_default_route,
            internet_reachable: evidence.ipv4_internet_reachable,
        },
        ipv6: AgentIpFamilyConnectivity {
            usable_ip: evidence.ipv6_usable_address,
            default_route: evidence.ipv6_default_route,
            internet_reachable: evidence.ipv6_internet_reachable,
        },
        dns_configured: evidence.dns_configured,
        dns_resolves: evidence.dns_resolves,
        captive_portal_suspected: evidence.captive_portal_suspected,
        reasons,
    }
}

fn active_interface_kind(evidence: HostConnectivityEvidence) -> AgentNetworkInterfaceKind {
    match (
        evidence.wireless_connected,
        evidence.ethernet_connected,
        evidence.other_interface_connected,
    ) {
        (true, true, _) | (true, false, true) | (false, true, true) => {
            AgentNetworkInterfaceKind::Multiple
        }
        (true, false, false) => AgentNetworkInterfaceKind::Wireless,
        (false, true, false) => AgentNetworkInterfaceKind::Ethernet,
        (false, false, true) => AgentNetworkInterfaceKind::Other,
        (false, false, false) if evidence.probe_complete => AgentNetworkInterfaceKind::None,
        _ => AgentNetworkInterfaceKind::Unknown,
    }
}

fn link_is_up(evidence: HostConnectivityEvidence) -> bool {
    evidence.wireless_connected || evidence.ethernet_connected || evidence.other_interface_connected
}

fn connectivity_reasons(
    evidence: HostConnectivityEvidence,
    link_up: bool,
) -> Vec<AgentHostConnectivityReason> {
    if !evidence.probe_complete {
        return vec![AgentHostConnectivityReason::ProbeUnavailable];
    }

    let mut reasons = Vec::new();
    if !link_up {
        reasons.push(AgentHostConnectivityReason::NoActiveInterface);
        push_reason(
            &mut reasons,
            evidence.wireless_present && !evidence.wireless_connected,
            AgentHostConnectivityReason::WirelessDisconnected,
        );
        push_reason(
            &mut reasons,
            evidence.ethernet_present && !evidence.ethernet_connected,
            AgentHostConnectivityReason::EthernetDisconnected,
        );
    }
    append_address_route_reasons(&mut reasons, evidence, link_up);
    append_dns_reasons(&mut reasons, evidence);
    append_reachability_reasons(&mut reasons, evidence);
    reasons
}

fn append_address_route_reasons(
    reasons: &mut Vec<AgentHostConnectivityReason>,
    evidence: HostConnectivityEvidence,
    link_up: bool,
) {
    if link_up && !evidence.ipv4_usable_address {
        reasons.push(AgentHostConnectivityReason::NoUsableIpv4Address);
    }
    if link_up && !evidence.ipv6_usable_address {
        reasons.push(AgentHostConnectivityReason::NoUsableIpv6Address);
    }
    if evidence.ipv4_usable_address && !evidence.ipv4_default_route {
        reasons.push(AgentHostConnectivityReason::NoIpv4DefaultRoute);
    }
    if evidence.ipv6_usable_address && !evidence.ipv6_default_route {
        reasons.push(AgentHostConnectivityReason::NoIpv6DefaultRoute);
    }
}

fn append_dns_reasons(
    reasons: &mut Vec<AgentHostConnectivityReason>,
    evidence: HostConnectivityEvidence,
) {
    if evidence.dns_configured == Some(false) {
        reasons.push(AgentHostConnectivityReason::DnsNotConfigured);
    }
    if evidence.dns_resolves == Some(false) {
        reasons.push(AgentHostConnectivityReason::DnsResolutionFailed);
    }
}

fn append_reachability_reasons(
    reasons: &mut Vec<AgentHostConnectivityReason>,
    evidence: HostConnectivityEvidence,
) {
    if evidence.ipv4_usable_address && evidence.ipv4_internet_reachable == Some(false) {
        reasons.push(AgentHostConnectivityReason::Ipv4InternetUnreachable);
    }
    if evidence.ipv6_usable_address && evidence.ipv6_internet_reachable == Some(false) {
        reasons.push(AgentHostConnectivityReason::Ipv6InternetUnreachable);
    }
    if evidence.captive_portal_suspected == Some(true) {
        reasons.push(AgentHostConnectivityReason::CaptivePortalSuspected);
    }
}

fn connectivity_status(
    evidence: HostConnectivityEvidence,
    link_up: bool,
    reasons: &mut Vec<AgentHostConnectivityReason>,
) -> AgentHostConnectivityStatus {
    if !evidence.probe_complete {
        return AgentHostConnectivityStatus::Indeterminate;
    }
    if !link_up {
        return AgentHostConnectivityStatus::LinkDisconnected;
    }
    if !evidence.ipv4_usable_address && !evidence.ipv6_usable_address {
        return AgentHostConnectivityStatus::AddressUnavailable;
    }
    if !has_default_route(evidence) {
        return AgentHostConnectivityStatus::DefaultRouteUnavailable;
    }
    if evidence.dns_configured == Some(false) || evidence.dns_resolves == Some(false) {
        return AgentHostConnectivityStatus::DnsUnavailable;
    }
    if evidence.captive_portal_suspected == Some(true) {
        return AgentHostConnectivityStatus::CaptivePortalSuspected;
    }

    match (
        evidence.ipv4_internet_reachable,
        evidence.ipv6_internet_reachable,
    ) {
        (Some(true), Some(true)) => AgentHostConnectivityStatus::OnlineDualStack,
        (Some(true), _) => AgentHostConnectivityStatus::OnlineIpv4Only,
        (_, Some(true)) => AgentHostConnectivityStatus::OnlineIpv6Only,
        (Some(false), Some(false)) => AgentHostConnectivityStatus::InternetUnreachable,
        (Some(false), None) if !evidence.ipv6_usable_address => {
            AgentHostConnectivityStatus::InternetUnreachable
        }
        (None, Some(false)) if !evidence.ipv4_usable_address => {
            AgentHostConnectivityStatus::InternetUnreachable
        }
        _ => {
            reasons.push(AgentHostConnectivityReason::ProbeUnavailable);
            AgentHostConnectivityStatus::Indeterminate
        }
    }
}

fn has_default_route(evidence: HostConnectivityEvidence) -> bool {
    (evidence.ipv4_usable_address && evidence.ipv4_default_route)
        || (evidence.ipv6_usable_address && evidence.ipv6_default_route)
}

fn push_reason(
    reasons: &mut Vec<AgentHostConnectivityReason>,
    condition: bool,
    reason: AgentHostConnectivityReason,
) {
    if condition {
        reasons.push(reason);
    }
}

#[cfg(test)]
mod tests {
    use super::{HostConnectivityEvidence, diagnose_host_connectivity};
    use crate::features::agent::model::{
        AgentHostConnectivityReason, AgentHostConnectivityStatus, AgentNetworkInterfaceKind,
    };

    fn connected_wifi() -> HostConnectivityEvidence {
        HostConnectivityEvidence {
            wireless_present: true,
            wireless_connected: true,
            ipv4_usable_address: true,
            ipv4_default_route: true,
            ipv4_internet_reachable: Some(true),
            dns_configured: Some(true),
            dns_resolves: Some(true),
            captive_portal_suspected: Some(false),
            probe_complete: true,
            ..HostConnectivityEvidence::default()
        }
    }

    #[test]
    fn dual_stack_online_requires_both_families_to_reach_the_internet() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            ipv6_usable_address: true,
            ipv6_default_route: true,
            ipv6_internet_reachable: Some(true),
            ..connected_wifi()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::OnlineDualStack
        );
        assert_eq!(
            snapshot.active_interface_kind,
            AgentNetworkInterfaceKind::Wireless
        );
        assert_eq!(snapshot.link_up, Some(true));
        assert!(snapshot.reasons.is_empty());
    }

    #[test]
    fn disconnected_wireless_and_ethernet_are_reported_without_guessing() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            wireless_present: true,
            ethernet_present: true,
            probe_complete: true,
            ..HostConnectivityEvidence::default()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::LinkDisconnected
        );
        assert_eq!(
            snapshot.active_interface_kind,
            AgentNetworkInterfaceKind::None
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::NoActiveInterface)
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::WirelessDisconnected)
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::EthernetDisconnected)
        );
    }

    #[test]
    fn connected_adapter_without_a_usable_ip_is_address_unavailable() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            ethernet_present: true,
            ethernet_connected: true,
            probe_complete: true,
            ..HostConnectivityEvidence::default()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::AddressUnavailable
        );
        assert_eq!(
            snapshot.active_interface_kind,
            AgentNetworkInterfaceKind::Ethernet
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::NoUsableIpv4Address)
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::NoUsableIpv6Address)
        );
    }

    #[test]
    fn usable_ip_without_a_default_route_is_reported_separately() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            ipv4_default_route: false,
            ipv4_internet_reachable: None,
            ..connected_wifi()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::DefaultRouteUnavailable
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::NoIpv4DefaultRoute)
        );
    }

    #[test]
    fn dns_configuration_and_resolution_failures_have_distinct_reasons() {
        let not_configured = diagnose_host_connectivity(HostConnectivityEvidence {
            dns_configured: Some(false),
            dns_resolves: None,
            ..connected_wifi()
        });
        assert_eq!(
            not_configured.status,
            AgentHostConnectivityStatus::DnsUnavailable
        );
        assert!(
            not_configured
                .reasons
                .contains(&AgentHostConnectivityReason::DnsNotConfigured)
        );

        let cannot_resolve = diagnose_host_connectivity(HostConnectivityEvidence {
            dns_resolves: Some(false),
            ..connected_wifi()
        });
        assert_eq!(
            cannot_resolve.status,
            AgentHostConnectivityStatus::DnsUnavailable
        );
        assert!(
            cannot_resolve
                .reasons
                .contains(&AgentHostConnectivityReason::DnsResolutionFailed)
        );
    }

    #[test]
    fn ipv4_only_and_ipv6_only_are_stable_final_states() {
        let ipv4_only = diagnose_host_connectivity(HostConnectivityEvidence {
            ipv6_usable_address: true,
            ipv6_default_route: true,
            ipv6_internet_reachable: Some(false),
            ..connected_wifi()
        });
        assert_eq!(
            ipv4_only.status,
            AgentHostConnectivityStatus::OnlineIpv4Only
        );
        assert!(
            ipv4_only
                .reasons
                .contains(&AgentHostConnectivityReason::Ipv6InternetUnreachable)
        );

        let ipv6_only = diagnose_host_connectivity(HostConnectivityEvidence {
            wireless_present: false,
            wireless_connected: false,
            ethernet_present: true,
            ethernet_connected: true,
            ipv4_usable_address: true,
            ipv4_default_route: true,
            ipv4_internet_reachable: Some(false),
            ipv6_usable_address: true,
            ipv6_default_route: true,
            ipv6_internet_reachable: Some(true),
            dns_configured: Some(true),
            dns_resolves: Some(true),
            captive_portal_suspected: Some(false),
            probe_complete: true,
            ..HostConnectivityEvidence::default()
        });
        assert_eq!(
            ipv6_only.status,
            AgentHostConnectivityStatus::OnlineIpv6Only
        );
        assert!(
            ipv6_only
                .reasons
                .contains(&AgentHostConnectivityReason::Ipv4InternetUnreachable)
        );
    }

    #[test]
    fn captive_portal_takes_precedence_over_transport_reachability() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            captive_portal_suspected: Some(true),
            ..connected_wifi()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::CaptivePortalSuspected
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::CaptivePortalSuspected)
        );
    }

    #[test]
    fn routed_network_without_external_reachability_is_internet_unreachable() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence {
            ipv4_internet_reachable: Some(false),
            ..connected_wifi()
        });

        assert_eq!(
            snapshot.status,
            AgentHostConnectivityStatus::InternetUnreachable
        );
        assert!(
            snapshot
                .reasons
                .contains(&AgentHostConnectivityReason::Ipv4InternetUnreachable)
        );
    }

    #[test]
    fn incomplete_probe_fails_closed_without_claiming_the_link_is_down() {
        let snapshot = diagnose_host_connectivity(HostConnectivityEvidence::default());

        assert_eq!(snapshot.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(
            snapshot.active_interface_kind,
            AgentNetworkInterfaceKind::Unknown
        );
        assert_eq!(snapshot.link_up, None);
        assert_eq!(
            snapshot.reasons,
            vec![AgentHostConnectivityReason::ProbeUnavailable]
        );
    }
}
