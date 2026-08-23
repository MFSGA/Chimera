use std::sync::Arc;

use super::ports::{HostConnectivityPort, PlatformReadinessPort};

pub(crate) mod fs_history;
mod http_bridge;
mod http_bridge_health;
mod http_network_probe;
mod legacy_config;
mod legacy_core;
mod legacy_mutation;
mod legacy_routing_probe;
mod legacy_runtime;
mod legacy_service;
mod legacy_snapshot;
mod legacy_system_proxy;
#[cfg(target_os = "linux")]
mod linux_host_connectivity;
#[cfg(any(target_os = "linux", test))]
mod linux_host_connectivity_core;
#[cfg(target_os = "linux")]
mod linux_platform_readiness;
#[cfg(target_os = "macos")]
mod macos_platform_readiness;
mod tauri_confirmation;
mod tauri_telemetry;
mod tool_executor;
#[cfg(any(not(any(windows, target_os = "linux")), test))]
mod unavailable_host_connectivity;
#[cfg(any(not(any(windows, target_os = "linux", target_os = "macos")), test))]
mod unavailable_platform_readiness;
#[cfg(any(target_os = "linux", target_os = "macos", test))]
mod unix_platform_readiness;
#[cfg(windows)]
mod windows_host_connectivity;
#[cfg(windows)]
mod windows_platform_readiness;

pub(crate) use fs_history::FsAgentHistoryPersistence;
pub(crate) use http_bridge::HttpAgentBridge;
pub(crate) use http_bridge_health::HttpBridgeHealth;
pub(crate) use http_network_probe::HttpNetworkProbe;
pub(crate) use legacy_config::LegacyAgentConfiguration;
pub(crate) use legacy_core::LegacyCoreLifecycle;
pub(crate) use legacy_mutation::LegacyAgentMutation;
pub(crate) use legacy_routing_probe::LegacyCoreRoutingProbe;
pub(crate) use legacy_runtime::LegacyAgentRuntime;
pub(crate) use legacy_service::LegacyServiceControl;
pub(crate) use legacy_system_proxy::LegacySystemProxy;
#[cfg(target_os = "linux")]
pub(crate) use linux_host_connectivity::LinuxHostConnectivity;
#[cfg(target_os = "linux")]
pub(crate) use linux_platform_readiness::LinuxPlatformReadiness;
#[cfg(target_os = "macos")]
pub(crate) use macos_platform_readiness::MacosPlatformReadiness;
pub(crate) use tauri_confirmation::TauriAgentConfirmation;
pub(crate) use tauri_telemetry::TauriAgentTelemetry;
pub(crate) use tool_executor::RegistryAgentToolExecutor;
#[cfg(any(not(any(windows, target_os = "linux")), test))]
pub(crate) use unavailable_host_connectivity::UnavailableHostConnectivity;
#[cfg(any(not(any(windows, target_os = "linux", target_os = "macos")), test))]
pub(crate) use unavailable_platform_readiness::UnavailablePlatformReadiness;
#[cfg(windows)]
pub(crate) use windows_host_connectivity::WindowsHostConnectivity;
#[cfg(windows)]
pub(crate) use windows_platform_readiness::WindowsPlatformReadiness;

pub(crate) fn platform_host_connectivity() -> Arc<dyn HostConnectivityPort> {
    #[cfg(windows)]
    {
        Arc::new(WindowsHostConnectivity::new())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxHostConnectivity::new())
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        Arc::new(UnavailableHostConnectivity::new())
    }
}

pub(crate) fn platform_readiness() -> Arc<dyn PlatformReadinessPort> {
    #[cfg(windows)]
    {
        Arc::new(WindowsPlatformReadiness::new())
    }
    #[cfg(target_os = "linux")]
    {
        Arc::new(LinuxPlatformReadiness::new())
    }
    #[cfg(target_os = "macos")]
    {
        Arc::new(MacosPlatformReadiness::new())
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        Arc::new(UnavailablePlatformReadiness::new())
    }
}

#[cfg(test)]
const fn platform_host_connectivity_name() -> &'static str {
    #[cfg(windows)]
    {
        "windows_native"
    }
    #[cfg(target_os = "linux")]
    {
        "linux_native"
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        "unavailable"
    }
}

#[cfg(test)]
const fn platform_readiness_name() -> &'static str {
    #[cfg(windows)]
    {
        "windows_native"
    }
    #[cfg(target_os = "linux")]
    {
        "linux_native"
    }
    #[cfg(target_os = "macos")]
    {
        "macos_native"
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        "unavailable"
    }
}

#[cfg(test)]
mod platform_tests {
    use super::{
        UnavailableHostConnectivity, UnavailablePlatformReadiness, platform_host_connectivity,
        platform_host_connectivity_name, platform_readiness, platform_readiness_name,
    };
    use crate::features::agent::{
        model::{
            AgentHostConnectivityReason, AgentHostConnectivityStatus, AgentProcessPrivilegeStatus,
        },
        ports::{HostConnectivityPort, PlatformReadinessPort},
    };

    #[test]
    fn platform_selection_is_compile_time_and_stable() {
        let _connectivity = platform_host_connectivity();
        let _readiness = platform_readiness();
        #[cfg(windows)]
        {
            assert_eq!(platform_host_connectivity_name(), "windows_native");
            assert_eq!(platform_readiness_name(), "windows_native");
        }
        #[cfg(target_os = "linux")]
        {
            assert_eq!(platform_host_connectivity_name(), "linux_native");
            assert_eq!(platform_readiness_name(), "linux_native");
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(platform_host_connectivity_name(), "unavailable");
            assert_eq!(platform_readiness_name(), "macos_native");
        }
        #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
        {
            assert_eq!(platform_host_connectivity_name(), "unavailable");
            assert_eq!(platform_readiness_name(), "unavailable");
        }
    }

    #[tokio::test]
    async fn unavailable_platform_result_never_claims_the_host_is_offline() {
        let snapshot = UnavailableHostConnectivity::new().snapshot().await;

        assert_eq!(snapshot.status, AgentHostConnectivityStatus::Indeterminate);
        assert_eq!(
            snapshot.reasons,
            vec![AgentHostConnectivityReason::ProbeUnavailable]
        );
        assert_eq!(snapshot.link_up, None);
        assert_eq!(snapshot.ipv4.internet_reachable, None);
        assert_eq!(snapshot.ipv6.internet_reachable, None);
        assert_eq!(snapshot.dns_configured, None);
        assert_eq!(snapshot.dns_resolves, None);
        assert_eq!(snapshot.captive_portal_suspected, None);
        assert_eq!(
            UnavailablePlatformReadiness::new()
                .process_privilege()
                .await,
            AgentProcessPrivilegeStatus::Unknown
        );
    }
}
