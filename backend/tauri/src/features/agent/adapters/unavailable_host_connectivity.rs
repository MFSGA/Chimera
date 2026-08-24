use super::super::{
    host_connectivity::unavailable_host_connectivity, model::AgentHostConnectivitySnapshot,
    ports::HostConnectivityPort,
};

#[derive(Default)]
pub(crate) struct UnavailableHostConnectivity;

impl UnavailableHostConnectivity {
    pub(crate) fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl HostConnectivityPort for UnavailableHostConnectivity {
    async fn snapshot(&self) -> AgentHostConnectivitySnapshot {
        unavailable_host_connectivity()
    }
}

#[cfg(test)]
mod tests {
    use super::UnavailableHostConnectivity;
    use crate::features::agent::{
        model::{
            AgentHostConnectivityReason, AgentHostConnectivityStatus, AgentNetworkInterfaceKind,
        },
        ports::HostConnectivityPort,
    };

    #[tokio::test]
    async fn unavailable_adapter_fails_closed_without_claiming_network_state() {
        let snapshot = UnavailableHostConnectivity::new().snapshot().await;

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
        assert_eq!(
            serde_json::to_value(snapshot).expect("serialize unavailable connectivity"),
            serde_json::json!({
                "status": "indeterminate",
                "active_interface_kind": "unknown",
                "link_up": null,
                "ipv4": {
                    "usable_ip": false,
                    "default_route": false,
                    "internet_reachable": null
                },
                "ipv6": {
                    "usable_ip": false,
                    "default_route": false,
                    "internet_reachable": null
                },
                "dns_configured": null,
                "dns_resolves": null,
                "captive_portal_suspected": null,
                "reasons": ["probe_unavailable"]
            })
        );
    }
}
