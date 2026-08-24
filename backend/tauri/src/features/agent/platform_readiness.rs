use super::model::{
    AgentAppliedState, AgentCoreSnapshot, AgentHostConnectivitySnapshot,
    AgentPlatformReadinessReason, AgentPlatformReadinessSnapshot, AgentProcessPrivilegeStatus,
    AgentRunType, AgentServiceSnapshot, AgentServiceState, AgentSystemDnsVerificationStatus,
    AgentTunPermissionReadiness, AgentTunSnapshot, AgentTunVerificationStatus,
};

pub(crate) fn classify_platform_readiness(
    process_privilege: AgentProcessPrivilegeStatus,
    core: &AgentCoreSnapshot,
    service: &AgentServiceSnapshot,
    tun: &AgentTunSnapshot,
    connectivity: &AgentHostConnectivitySnapshot,
) -> AgentPlatformReadinessSnapshot {
    let service_mode_available = service_mode_available(service);
    let tun_verification = tun_verification(tun);
    let tun_permission = tun_permission(
        process_privilege,
        core,
        service,
        tun,
        tun_verification,
        service_mode_available,
    );
    let system_dns_verification = system_dns_verification(tun, connectivity);
    let reasons = readiness_reasons(
        process_privilege,
        core,
        tun,
        tun_permission,
        tun_verification,
        system_dns_verification,
    );

    AgentPlatformReadinessSnapshot {
        process_privilege,
        service_mode_available,
        tun_permission,
        tun_verification,
        system_dns_verification,
        reasons,
    }
}

fn service_mode_available(service: &AgentServiceSnapshot) -> Option<bool> {
    match (service.state, service.runtime_compatible) {
        (AgentServiceState::NotInstalled, _) => Some(false),
        (AgentServiceState::Stopped | AgentServiceState::Running, Some(true)) => Some(true),
        (AgentServiceState::Stopped | AgentServiceState::Running, Some(false)) => Some(false),
        (AgentServiceState::Stopped | AgentServiceState::Running, None)
        | (AgentServiceState::Unknown, _) => None,
    }
}

fn tun_verification(tun: &AgentTunSnapshot) -> AgentTunVerificationStatus {
    if !tun.desired_enabled {
        return AgentTunVerificationStatus::NotRequested;
    }
    if tun.applied_consistency == AgentAppliedState::Consistent
        && tun.generated_runtime_enabled == Some(true)
        && tun.observed_enabled == Some(true)
    {
        return AgentTunVerificationStatus::Verified;
    }
    if tun.generated_runtime_enabled.is_none()
        || tun.observed_enabled.is_none()
        || tun.applied_consistency == AgentAppliedState::Unknown
    {
        AgentTunVerificationStatus::Unavailable
    } else {
        AgentTunVerificationStatus::Inconsistent
    }
}

fn tun_permission(
    process_privilege: AgentProcessPrivilegeStatus,
    core: &AgentCoreSnapshot,
    service: &AgentServiceSnapshot,
    tun: &AgentTunSnapshot,
    verification: AgentTunVerificationStatus,
    service_available: Option<bool>,
) -> AgentTunPermissionReadiness {
    if !tun.desired_enabled {
        return AgentTunPermissionReadiness::NotRequired;
    }
    let service_active = core.run_type == AgentRunType::Service
        && service.state == AgentServiceState::Running
        && service.ipc_connected
        && service.runtime_compatible == Some(true);
    if verification == AgentTunVerificationStatus::Verified
        || core.run_type == AgentRunType::Elevated
        || process_privilege == AgentProcessPrivilegeStatus::Elevated
        || service_active
    {
        return AgentTunPermissionReadiness::Satisfied;
    }
    if service_available == Some(true) {
        return AgentTunPermissionReadiness::ServiceAlternativeAvailable;
    }
    match (process_privilege, service_available) {
        (AgentProcessPrivilegeStatus::Standard, Some(false)) => {
            AgentTunPermissionReadiness::Required
        }
        _ => AgentTunPermissionReadiness::Indeterminate,
    }
}

fn system_dns_verification(
    tun: &AgentTunSnapshot,
    connectivity: &AgentHostConnectivitySnapshot,
) -> AgentSystemDnsVerificationStatus {
    if !tun.desired_enabled {
        return AgentSystemDnsVerificationStatus::NotRequired;
    }
    match (connectivity.dns_configured, connectivity.dns_resolves) {
        (Some(true), Some(true)) => AgentSystemDnsVerificationStatus::Verified,
        (Some(false), _) => AgentSystemDnsVerificationStatus::NotConfigured,
        (Some(true), Some(false)) => AgentSystemDnsVerificationStatus::ResolutionFailed,
        _ => AgentSystemDnsVerificationStatus::Unavailable,
    }
}

fn readiness_reasons(
    process_privilege: AgentProcessPrivilegeStatus,
    core: &AgentCoreSnapshot,
    tun: &AgentTunSnapshot,
    permission: AgentTunPermissionReadiness,
    verification: AgentTunVerificationStatus,
    dns: AgentSystemDnsVerificationStatus,
) -> Vec<AgentPlatformReadinessReason> {
    if !tun.desired_enabled {
        return Vec::new();
    }
    let mut reasons = Vec::new();
    match process_privilege {
        AgentProcessPrivilegeStatus::Elevated => {
            reasons.push(AgentPlatformReadinessReason::ElevatedProcess)
        }
        AgentProcessPrivilegeStatus::Unknown => {
            reasons.push(AgentPlatformReadinessReason::PrivilegeProbeUnavailable)
        }
        AgentProcessPrivilegeStatus::Standard => {}
    }
    if core.run_type == AgentRunType::Service {
        reasons.push(AgentPlatformReadinessReason::ServiceModeActive);
    }
    match permission {
        AgentTunPermissionReadiness::ServiceAlternativeAvailable => {
            reasons.push(AgentPlatformReadinessReason::ServiceModeAvailable)
        }
        AgentTunPermissionReadiness::Required => {
            reasons.push(AgentPlatformReadinessReason::PermissionRequired)
        }
        AgentTunPermissionReadiness::NotRequired
        | AgentTunPermissionReadiness::Satisfied
        | AgentTunPermissionReadiness::Indeterminate => {}
    }
    match verification {
        AgentTunVerificationStatus::Unavailable => {
            reasons.push(AgentPlatformReadinessReason::TunStateUnavailable)
        }
        AgentTunVerificationStatus::Inconsistent => {
            reasons.push(AgentPlatformReadinessReason::TunStateInconsistent)
        }
        AgentTunVerificationStatus::NotRequested | AgentTunVerificationStatus::Verified => {}
    }
    match dns {
        AgentSystemDnsVerificationStatus::NotConfigured => {
            reasons.push(AgentPlatformReadinessReason::SystemDnsNotConfigured)
        }
        AgentSystemDnsVerificationStatus::ResolutionFailed => {
            reasons.push(AgentPlatformReadinessReason::SystemDnsResolutionFailed)
        }
        AgentSystemDnsVerificationStatus::Unavailable => {
            reasons.push(AgentPlatformReadinessReason::SystemDnsUnavailable)
        }
        AgentSystemDnsVerificationStatus::NotRequired
        | AgentSystemDnsVerificationStatus::Verified => {}
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::classify_platform_readiness;
    use crate::features::agent::{
        host_connectivity::unavailable_host_connectivity,
        model::{
            AgentAppliedState, AgentCoreSnapshot, AgentCoreState, AgentPlatformReadinessReason,
            AgentProcessPrivilegeStatus, AgentRoutingMode, AgentRunType, AgentSelectedCore,
            AgentServiceSnapshot, AgentServiceState, AgentSystemDnsVerificationStatus,
            AgentTunPermissionReadiness, AgentTunSnapshot, AgentTunVerificationStatus,
        },
    };

    #[test]
    fn tun_not_requested_never_degrades_for_unavailable_privilege_or_dns_observation() {
        let readiness = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Unknown,
            &core(AgentRunType::Unknown),
            &service(AgentServiceState::Unknown, false),
            &tun(
                false,
                Some(false),
                Some(false),
                AgentAppliedState::Consistent,
            ),
            &unavailable_host_connectivity(),
        );

        assert_eq!(
            readiness.tun_permission,
            AgentTunPermissionReadiness::NotRequired
        );
        assert_eq!(
            readiness.tun_verification,
            AgentTunVerificationStatus::NotRequested
        );
        assert_eq!(
            readiness.system_dns_verification,
            AgentSystemDnsVerificationStatus::NotRequired
        );
        assert!(readiness.reasons.is_empty());
    }

    #[test]
    fn verified_tun_proves_permission_readiness_without_guessing_platform_details() {
        let mut connectivity = unavailable_host_connectivity();
        connectivity.dns_configured = Some(true);
        connectivity.dns_resolves = Some(true);
        let readiness = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Standard,
            &core(AgentRunType::Normal),
            &service(AgentServiceState::NotInstalled, false),
            &tun(true, Some(true), Some(true), AgentAppliedState::Consistent),
            &connectivity,
        );

        assert_eq!(
            readiness.tun_permission,
            AgentTunPermissionReadiness::Satisfied
        );
        assert_eq!(
            readiness.tun_verification,
            AgentTunVerificationStatus::Verified
        );
        assert_eq!(
            readiness.system_dns_verification,
            AgentSystemDnsVerificationStatus::Verified
        );
        assert!(readiness.reasons.is_empty());
    }

    #[test]
    fn standard_process_uses_service_as_a_closed_alternative_or_requires_permission() {
        let unavailable_tun = tun(true, Some(true), None, AgentAppliedState::Unknown);
        let with_service = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Standard,
            &core(AgentRunType::Normal),
            &service(AgentServiceState::Stopped, false),
            &unavailable_tun,
            &unavailable_host_connectivity(),
        );
        assert_eq!(
            with_service.tun_permission,
            AgentTunPermissionReadiness::ServiceAlternativeAvailable
        );
        assert!(
            with_service
                .reasons
                .contains(&AgentPlatformReadinessReason::ServiceModeAvailable)
        );

        let without_service = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Standard,
            &core(AgentRunType::Normal),
            &service(AgentServiceState::NotInstalled, false),
            &unavailable_tun,
            &unavailable_host_connectivity(),
        );
        assert_eq!(
            without_service.tun_permission,
            AgentTunPermissionReadiness::Required
        );
        assert!(
            without_service
                .reasons
                .contains(&AgentPlatformReadinessReason::PermissionRequired)
        );

        let incompatible_service = AgentServiceSnapshot {
            desired_enabled: true,
            state: AgentServiceState::Stopped,
            ipc_connected: false,
            runtime_compatible: Some(false),
        };
        let incompatible = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Standard,
            &core(AgentRunType::Normal),
            &incompatible_service,
            &unavailable_tun,
            &unavailable_host_connectivity(),
        );
        assert_eq!(
            incompatible.tun_permission,
            AgentTunPermissionReadiness::Required
        );
        assert_eq!(incompatible.service_mode_available, Some(false));
    }

    #[test]
    fn dns_acceptance_states_are_closed_and_privacy_safe() {
        let mut connectivity = unavailable_host_connectivity();
        connectivity.dns_configured = Some(false);
        let not_configured = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Elevated,
            &core(AgentRunType::Elevated),
            &service(AgentServiceState::NotInstalled, false),
            &tun(true, Some(true), Some(true), AgentAppliedState::Consistent),
            &connectivity,
        );
        assert_eq!(
            not_configured.system_dns_verification,
            AgentSystemDnsVerificationStatus::NotConfigured
        );

        connectivity.dns_configured = Some(true);
        connectivity.dns_resolves = Some(false);
        let resolution_failed = classify_platform_readiness(
            AgentProcessPrivilegeStatus::Elevated,
            &core(AgentRunType::Elevated),
            &service(AgentServiceState::NotInstalled, false),
            &tun(true, Some(true), Some(true), AgentAppliedState::Consistent),
            &connectivity,
        );
        assert_eq!(
            resolution_failed.system_dns_verification,
            AgentSystemDnsVerificationStatus::ResolutionFailed
        );
        let value = serde_json::to_value(resolution_failed).expect("serialize readiness");
        let object = value.as_object().expect("readiness object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<Vec<_>>(),
            vec![
                "process_privilege",
                "reasons",
                "service_mode_available",
                "system_dns_verification",
                "tun_permission",
                "tun_verification",
            ]
        );
    }

    fn core(run_type: AgentRunType) -> AgentCoreSnapshot {
        AgentCoreSnapshot {
            state: AgentCoreState::Running,
            run_type,
            selected_core: AgentSelectedCore::Mihomo,
            state_changed_at: 0,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Rule),
            observed_routing_mode: Some(AgentRoutingMode::Rule),
            applied_consistency: AgentAppliedState::Consistent,
        }
    }

    fn service(state: AgentServiceState, ipc_connected: bool) -> AgentServiceSnapshot {
        AgentServiceSnapshot {
            desired_enabled: false,
            state,
            ipc_connected,
            runtime_compatible: match state {
                AgentServiceState::Stopped | AgentServiceState::Running => Some(true),
                AgentServiceState::NotInstalled | AgentServiceState::Unknown => None,
            },
        }
    }

    fn tun(
        desired_enabled: bool,
        generated_runtime_enabled: Option<bool>,
        observed_enabled: Option<bool>,
        applied_consistency: AgentAppliedState,
    ) -> AgentTunSnapshot {
        AgentTunSnapshot {
            desired_enabled,
            generated_runtime_enabled,
            observed_enabled,
            applied_consistency,
        }
    }
}
