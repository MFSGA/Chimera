use super::model::{
    AgentActionRequest, AgentActionRisk, AgentAppliedState, AgentCommandError, AgentConnectorState,
    AgentCoreState, AgentHostScope, AgentImpact, AgentNetworkSnapshot, AgentOsFamily,
    AgentRecommendation, AgentRecommendationUnavailableReason, AgentResult, AgentRoutingMode,
    AgentRunType, AgentSelectedCore, AgentServiceState, AgentStateChange, AgentStateField,
    AgentStateValue,
};

#[derive(Debug, Clone)]
pub(super) enum ActionPreconditions {
    SetRoutingMode {
        before: AgentRoutingMode,
        core_state_changed_at: i64,
    },
    SetTunEnabled {
        desired_before: bool,
        generated_before: Option<bool>,
        core_state: AgentCoreState,
        core_run_type: AgentRunType,
        core_state_changed_at: i64,
        selected_core: AgentSelectedCore,
        service_desired_enabled: bool,
        service_state: AgentServiceState,
        service_ipc_connected: bool,
    },
    SetSystemProxyEnabled {
        desired_before: bool,
        observed_before: bool,
        expected_port: u16,
        core_state: AgentCoreState,
        core_state_changed_at: i64,
    },
    SetServiceMode {
        desired_before: bool,
        service_state: AgentServiceState,
        ipc_connected: bool,
        runtime_compatible: Option<bool>,
        core_run_type: AgentRunType,
        core_state_changed_at: i64,
        selected_core: AgentSelectedCore,
    },
    StartCore {
        core_state_changed_at: i64,
        selected_core: AgentSelectedCore,
    },
    RestartCore {
        core_state_changed_at: i64,
        selected_core: AgentSelectedCore,
    },
    ReconnectTelemetry {
        core_state_changed_at: i64,
        selected_core: AgentSelectedCore,
    },
    ControlService {
        state_before: AgentServiceState,
        desired_enabled: bool,
        ipc_connected: bool,
        runtime_compatible: Option<bool>,
        core_state: AgentCoreState,
        core_run_type: AgentRunType,
        core_state_changed_at: i64,
    },
    RepairSystemProxyEndpoint {
        core_state_changed_at: i64,
        expected_port: u16,
        observed_host_scope: AgentHostScope,
        observed_port: Option<u16>,
        desired_before: bool,
    },
    DisableStaleSystemProxy {
        core_state_changed_at: i64,
        expected_port: u16,
        desired_before: bool,
    },
}

pub(super) struct ActionPlan {
    pub(super) risk: AgentActionRisk,
    pub(super) impacts: Vec<AgentImpact>,
    pub(super) changes: Vec<AgentStateChange>,
    pub(super) preconditions: ActionPreconditions,
}

pub(crate) fn recommendations(snapshot: &AgentNetworkSnapshot) -> Vec<AgentRecommendation> {
    recommendation_candidates(snapshot)
        .into_iter()
        .map(|action| match plan_action(snapshot, &action) {
            Ok(plan) => AgentRecommendation {
                action,
                available: true,
                unavailable_reason: None,
                risk: Some(plan.risk),
                impacts: plan.impacts,
            },
            Err(_) => AgentRecommendation {
                action,
                available: false,
                unavailable_reason: Some(
                    AgentRecommendationUnavailableReason::CurrentStateNotSupported,
                ),
                risk: None,
                impacts: Vec::new(),
            },
        })
        .collect()
}

fn recommendation_candidates(snapshot: &AgentNetworkSnapshot) -> Vec<AgentActionRequest> {
    vec![
        AgentActionRequest::SetRoutingMode {
            mode: AgentRoutingMode::Rule,
        },
        AgentActionRequest::SetRoutingMode {
            mode: AgentRoutingMode::Global,
        },
        AgentActionRequest::SetRoutingMode {
            mode: AgentRoutingMode::Direct,
        },
        AgentActionRequest::SetTunEnabled {
            enabled: !snapshot.tun.desired_enabled,
        },
        AgentActionRequest::SetSystemProxyEnabled {
            enabled: !snapshot.system_proxy.desired_enabled,
        },
        AgentActionRequest::SetServiceMode {
            enabled: !snapshot.service.desired_enabled,
        },
        AgentActionRequest::StartCore,
        AgentActionRequest::RestartCore,
        AgentActionRequest::ReconnectTelemetry,
        AgentActionRequest::StartService,
        AgentActionRequest::StopService,
        AgentActionRequest::RestartService,
        AgentActionRequest::RepairSystemProxyEndpoint,
        AgentActionRequest::DisableStaleSystemProxy,
    ]
}

pub(super) fn plan_action(
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
) -> AgentResult<ActionPlan> {
    if snapshot.core.state == AgentCoreState::Unknown
        || snapshot.core.run_type == AgentRunType::Unknown
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    match action {
        AgentActionRequest::SetRoutingMode { mode } => plan_routing_mode(snapshot, *mode),
        AgentActionRequest::SetTunEnabled { enabled } => plan_tun_change(snapshot, *enabled),
        AgentActionRequest::SetSystemProxyEnabled { enabled } => {
            plan_system_proxy_change(snapshot, *enabled)
        }
        AgentActionRequest::SetServiceMode { enabled } => {
            plan_service_mode_change(snapshot, *enabled)
        }
        AgentActionRequest::StartCore => plan_start_core(snapshot),
        AgentActionRequest::RestartCore => plan_restart_core(snapshot),
        AgentActionRequest::ReconnectTelemetry => plan_reconnect_telemetry(snapshot),
        AgentActionRequest::StartService
        | AgentActionRequest::StopService
        | AgentActionRequest::RestartService => plan_service_control(snapshot, action),
        AgentActionRequest::RepairSystemProxyEndpoint => plan_proxy_endpoint_repair(snapshot),
        AgentActionRequest::DisableStaleSystemProxy => plan_stale_proxy(snapshot),
    }
}

fn tun_state_value(enabled: bool) -> AgentStateValue {
    if enabled {
        AgentStateValue::Enabled
    } else {
        AgentStateValue::Disabled
    }
}

pub(super) fn tun_impacts(os_family: AgentOsFamily) -> Vec<AgentImpact> {
    let mut impacts = vec![
        AgentImpact::ExistingConnectionsMayChange,
        AgentImpact::CoreMayRestart,
    ];
    if os_family == AgentOsFamily::Macos {
        impacts.push(AgentImpact::HostDnsMayChange);
    }
    if matches!(os_family, AgentOsFamily::Macos | AgentOsFamily::Linux) {
        impacts.push(AgentImpact::AdminPermissionMayBeRequired);
    }
    impacts
}

pub(super) fn plan_tun_change(
    snapshot: &AgentNetworkSnapshot,
    target: bool,
) -> AgentResult<ActionPlan> {
    let current = snapshot.tun.desired_enabled;
    if current == target
        || snapshot.core.state != AgentCoreState::Running
        || !snapshot.core.runtime_config_present
        || snapshot.profiles.active_count == 0
        || !snapshot.profiles.active_references_valid
        || snapshot.tun.generated_runtime_enabled != Some(current)
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    Ok(ActionPlan {
        risk: AgentActionRisk::TrafficChange,
        impacts: tun_impacts(snapshot.os_family),
        changes: vec![AgentStateChange {
            field: AgentStateField::Tun,
            before: tun_state_value(current),
            after: tun_state_value(target),
        }],
        preconditions: ActionPreconditions::SetTunEnabled {
            desired_before: current,
            generated_before: snapshot.tun.generated_runtime_enabled,
            core_state: snapshot.core.state,
            core_run_type: snapshot.core.run_type,
            core_state_changed_at: snapshot.core.state_changed_at,
            selected_core: snapshot.core.selected_core,
            service_desired_enabled: snapshot.service.desired_enabled,
            service_state: snapshot.service.state,
            service_ipc_connected: snapshot.service.ipc_connected,
        },
    })
}

pub(super) fn plan_system_proxy_change(
    snapshot: &AgentNetworkSnapshot,
    target: bool,
) -> AgentResult<ActionPlan> {
    let proxy = &snapshot.system_proxy;
    let observed = proxy
        .observed_enabled
        .ok_or(AgentCommandError::ActionNotAvailable)?;
    let enable_available = target
        && !proxy.desired_enabled
        && !observed
        && snapshot.core.state == AgentCoreState::Running
        && snapshot.core.runtime_config_present
        && snapshot.profiles.active_count > 0
        && snapshot.profiles.active_references_valid;
    let disable_available = !target && proxy.desired_enabled && observed;
    if !enable_available && !disable_available {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::HostNetworkChange,
        impacts: vec![if target {
            AgentImpact::HostSystemProxyEnabled
        } else {
            AgentImpact::HostSystemProxyDisabled
        }],
        changes: vec![AgentStateChange {
            field: AgentStateField::SystemProxy,
            before: tun_state_value(observed),
            after: tun_state_value(target),
        }],
        preconditions: ActionPreconditions::SetSystemProxyEnabled {
            desired_before: proxy.desired_enabled,
            observed_before: observed,
            expected_port: proxy.expected_mixed_port,
            core_state: snapshot.core.state,
            core_state_changed_at: snapshot.core.state_changed_at,
        },
    })
}

fn routing_state_value(mode: AgentRoutingMode) -> AgentStateValue {
    match mode {
        AgentRoutingMode::Rule => AgentStateValue::Rule,
        AgentRoutingMode::Global => AgentStateValue::Global,
        AgentRoutingMode::Direct => AgentStateValue::Direct,
    }
}

pub(super) fn plan_routing_mode(
    snapshot: &AgentNetworkSnapshot,
    target: AgentRoutingMode,
) -> AgentResult<ActionPlan> {
    let current = snapshot
        .core
        .routing_mode
        .ok_or(AgentCommandError::ActionNotAvailable)?;
    if current == target
        || snapshot.core.state != AgentCoreState::Running
        || snapshot.core.observed_routing_mode != Some(current)
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::TrafficChange,
        impacts: routing_impacts(target),
        changes: vec![AgentStateChange {
            field: AgentStateField::RoutingMode,
            before: routing_state_value(current),
            after: routing_state_value(target),
        }],
        preconditions: ActionPreconditions::SetRoutingMode {
            before: current,
            core_state_changed_at: snapshot.core.state_changed_at,
        },
    })
}

pub(super) fn plan_service_mode_change(
    snapshot: &AgentNetworkSnapshot,
    target: bool,
) -> AgentResult<ActionPlan> {
    let service = &snapshot.service;
    if service.desired_enabled == target
        || service.state != AgentServiceState::Running
        || !service.ipc_connected
        || service.runtime_compatible != Some(true)
        || snapshot.core.state != AgentCoreState::Running
        || !snapshot.core.runtime_config_present
        || snapshot.profiles.active_count == 0
        || !snapshot.profiles.active_references_valid
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    Ok(ActionPlan {
        risk: AgentActionRisk::ServiceControl,
        impacts: vec![
            AgentImpact::ServiceAvailabilityMayChange,
            AgentImpact::ExistingConnectionsMayChange,
        ],
        changes: vec![AgentStateChange {
            field: AgentStateField::ServiceMode,
            before: tun_state_value(service.desired_enabled),
            after: tun_state_value(target),
        }],
        preconditions: ActionPreconditions::SetServiceMode {
            desired_before: service.desired_enabled,
            service_state: service.state,
            ipc_connected: service.ipc_connected,
            runtime_compatible: service.runtime_compatible,
            core_run_type: snapshot.core.run_type,
            core_state_changed_at: snapshot.core.state_changed_at,
            selected_core: snapshot.core.selected_core,
        },
    })
}

pub(super) fn plan_start_core(snapshot: &AgentNetworkSnapshot) -> AgentResult<ActionPlan> {
    if snapshot.core.state != AgentCoreState::Stopped
        || !snapshot.core.runtime_config_present
        || snapshot.profiles.active_count == 0
        || !snapshot.profiles.active_references_valid
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::TrafficChange,
        impacts: vec![AgentImpact::ExistingConnectionsMayChange],
        changes: vec![AgentStateChange {
            field: AgentStateField::CoreProcess,
            before: AgentStateValue::Stopped,
            after: AgentStateValue::Running,
        }],
        preconditions: ActionPreconditions::StartCore {
            core_state_changed_at: snapshot.core.state_changed_at,
            selected_core: snapshot.core.selected_core,
        },
    })
}

pub(super) fn plan_restart_core(snapshot: &AgentNetworkSnapshot) -> AgentResult<ActionPlan> {
    if snapshot.core.state != AgentCoreState::Running
        || !snapshot.core.runtime_config_present
        || snapshot.profiles.active_count == 0
        || !snapshot.profiles.active_references_valid
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::TrafficChange,
        impacts: vec![AgentImpact::ExistingConnectionsMayChange],
        changes: vec![AgentStateChange {
            field: AgentStateField::CoreProcess,
            before: AgentStateValue::Running,
            after: AgentStateValue::Restarted,
        }],
        preconditions: ActionPreconditions::RestartCore {
            core_state_changed_at: snapshot.core.state_changed_at,
            selected_core: snapshot.core.selected_core,
        },
    })
}

pub(super) fn plan_reconnect_telemetry(snapshot: &AgentNetworkSnapshot) -> AgentResult<ActionPlan> {
    if snapshot.core.state != AgentCoreState::Running
        || snapshot.telemetry.state != AgentConnectorState::Disconnected
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::TelemetryRecovery,
        impacts: vec![AgentImpact::TelemetryMayBeUnavailable],
        changes: vec![AgentStateChange {
            field: AgentStateField::TelemetryConnector,
            before: AgentStateValue::Disconnected,
            after: AgentStateValue::Connected,
        }],
        preconditions: ActionPreconditions::ReconnectTelemetry {
            core_state_changed_at: snapshot.core.state_changed_at,
            selected_core: snapshot.core.selected_core,
        },
    })
}

pub(super) fn plan_service_control(
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
) -> AgentResult<ActionPlan> {
    let service = &snapshot.service;
    let available = match action {
        AgentActionRequest::StartService => {
            service.desired_enabled && service.state == AgentServiceState::Stopped
        }
        AgentActionRequest::StopService => {
            !service.desired_enabled
                && service.state == AgentServiceState::Running
                && snapshot.core.run_type != AgentRunType::Service
        }
        AgentActionRequest::RestartService => {
            service.desired_enabled
                && service.state == AgentServiceState::Running
                && service.ipc_connected
                && service.runtime_compatible == Some(true)
                && snapshot.core.state == AgentCoreState::Running
                && snapshot.core.run_type == AgentRunType::Service
                && snapshot.profiles.active_count > 0
                && snapshot.profiles.active_references_valid
        }
        _ => false,
    };
    if !available {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    let (before, after) = match action {
        AgentActionRequest::StartService => (AgentStateValue::Stopped, AgentStateValue::Running),
        AgentActionRequest::StopService => (AgentStateValue::Running, AgentStateValue::Stopped),
        AgentActionRequest::RestartService => {
            (AgentStateValue::Running, AgentStateValue::Restarted)
        }
        _ => return Err(AgentCommandError::ActionNotAvailable),
    };
    let mut impacts = vec![AgentImpact::ServiceAvailabilityMayChange];
    if matches!(action, AgentActionRequest::RestartService) {
        impacts.push(AgentImpact::ExistingConnectionsMayChange);
    }

    Ok(ActionPlan {
        risk: AgentActionRisk::ServiceControl,
        impacts,
        changes: vec![AgentStateChange {
            field: AgentStateField::Service,
            before,
            after,
        }],
        preconditions: ActionPreconditions::ControlService {
            state_before: service.state,
            desired_enabled: service.desired_enabled,
            ipc_connected: service.ipc_connected,
            runtime_compatible: service.runtime_compatible,
            core_state: snapshot.core.state,
            core_run_type: snapshot.core.run_type,
            core_state_changed_at: snapshot.core.state_changed_at,
        },
    })
}

pub(super) fn plan_proxy_endpoint_repair(
    snapshot: &AgentNetworkSnapshot,
) -> AgentResult<ActionPlan> {
    let proxy = &snapshot.system_proxy;
    if snapshot.core.state != AgentCoreState::Running
        || !proxy.desired_enabled
        || proxy.observed_enabled != Some(true)
        || proxy.matches_expected_endpoint != Some(false)
        || proxy.observed_host_scope == AgentHostScope::Unknown
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::HostNetworkChange,
        impacts: vec![AgentImpact::HostSystemProxyEndpointChanged],
        changes: vec![AgentStateChange {
            field: AgentStateField::SystemProxyEndpoint,
            before: AgentStateValue::Unexpected,
            after: AgentStateValue::ExpectedLoopbackEndpoint,
        }],
        preconditions: ActionPreconditions::RepairSystemProxyEndpoint {
            core_state_changed_at: snapshot.core.state_changed_at,
            expected_port: proxy.expected_mixed_port,
            observed_host_scope: proxy.observed_host_scope,
            observed_port: proxy.observed_port,
            desired_before: proxy.desired_enabled,
        },
    })
}

fn plan_stale_proxy(snapshot: &AgentNetworkSnapshot) -> AgentResult<ActionPlan> {
    let proxy = &snapshot.system_proxy;
    if snapshot.core.state != AgentCoreState::Stopped
        || proxy.observed_enabled != Some(true)
        || proxy.matches_expected_endpoint != Some(true)
    {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    Ok(ActionPlan {
        risk: AgentActionRisk::HostNetworkChange,
        impacts: vec![AgentImpact::HostSystemProxyDisabled],
        changes: vec![AgentStateChange {
            field: AgentStateField::SystemProxy,
            before: AgentStateValue::Enabled,
            after: AgentStateValue::Disabled,
        }],
        preconditions: ActionPreconditions::DisableStaleSystemProxy {
            core_state_changed_at: snapshot.core.state_changed_at,
            expected_port: proxy.expected_mixed_port,
            desired_before: proxy.desired_enabled,
        },
    })
}

pub(super) fn validate_preconditions(
    current: &AgentNetworkSnapshot,
    preconditions: &ActionPreconditions,
) -> AgentResult<()> {
    let valid = match preconditions {
        ActionPreconditions::SetRoutingMode {
            before,
            core_state_changed_at,
        } => {
            current.core.state == AgentCoreState::Running
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.routing_mode == Some(*before)
                && current.core.observed_routing_mode == Some(*before)
        }
        ActionPreconditions::SetTunEnabled {
            desired_before,
            generated_before,
            core_state,
            core_run_type,
            core_state_changed_at,
            selected_core,
            service_desired_enabled,
            service_state,
            service_ipc_connected,
        } => {
            current.tun.desired_enabled == *desired_before
                && current.tun.generated_runtime_enabled == *generated_before
                && current.core.state == *core_state
                && current.core.run_type == *core_run_type
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.selected_core == *selected_core
                && current.core.runtime_config_present
                && current.profiles.active_count > 0
                && current.profiles.active_references_valid
                && current.service.desired_enabled == *service_desired_enabled
                && current.service.state == *service_state
                && current.service.ipc_connected == *service_ipc_connected
        }
        ActionPreconditions::SetSystemProxyEnabled {
            desired_before,
            observed_before,
            expected_port,
            core_state,
            core_state_changed_at,
        } => {
            current.system_proxy.desired_enabled == *desired_before
                && current.system_proxy.observed_enabled == Some(*observed_before)
                && current.system_proxy.expected_mixed_port == *expected_port
                && current.core.state == *core_state
                && current.core.state_changed_at == *core_state_changed_at
        }
        ActionPreconditions::SetServiceMode {
            desired_before,
            service_state,
            ipc_connected,
            runtime_compatible,
            core_run_type,
            core_state_changed_at,
            selected_core,
        } => {
            current.service.desired_enabled == *desired_before
                && current.service.state == *service_state
                && current.service.ipc_connected == *ipc_connected
                && current.service.runtime_compatible == *runtime_compatible
                && current.core.state == AgentCoreState::Running
                && current.core.run_type == *core_run_type
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.selected_core == *selected_core
                && current.core.runtime_config_present
                && current.profiles.active_count > 0
                && current.profiles.active_references_valid
        }
        ActionPreconditions::StartCore {
            core_state_changed_at,
            selected_core,
        } => {
            current.core.state == AgentCoreState::Stopped
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.selected_core == *selected_core
                && current.core.runtime_config_present
                && current.profiles.active_count > 0
                && current.profiles.active_references_valid
        }
        ActionPreconditions::RestartCore {
            core_state_changed_at,
            selected_core,
        } => {
            current.core.state == AgentCoreState::Running
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.selected_core == *selected_core
                && current.core.runtime_config_present
                && current.profiles.active_count > 0
                && current.profiles.active_references_valid
        }
        ActionPreconditions::ReconnectTelemetry {
            core_state_changed_at,
            selected_core,
        } => {
            current.core.state == AgentCoreState::Running
                && current.core.state_changed_at == *core_state_changed_at
                && current.core.selected_core == *selected_core
                && current.telemetry.state == AgentConnectorState::Disconnected
        }
        ActionPreconditions::ControlService {
            state_before,
            desired_enabled,
            ipc_connected,
            runtime_compatible,
            core_state,
            core_run_type,
            core_state_changed_at,
        } => {
            current.service.state == *state_before
                && current.service.desired_enabled == *desired_enabled
                && current.service.ipc_connected == *ipc_connected
                && current.service.runtime_compatible == *runtime_compatible
                && current.core.state == *core_state
                && current.core.run_type == *core_run_type
                && current.core.state_changed_at == *core_state_changed_at
        }
        ActionPreconditions::RepairSystemProxyEndpoint {
            core_state_changed_at,
            expected_port,
            observed_host_scope,
            observed_port,
            ..
        } => {
            current.core.state == AgentCoreState::Running
                && current.core.state_changed_at == *core_state_changed_at
                && current.system_proxy.desired_enabled
                && current.system_proxy.observed_enabled == Some(true)
                && current.system_proxy.matches_expected_endpoint == Some(false)
                && current.system_proxy.expected_mixed_port == *expected_port
                && current.system_proxy.observed_host_scope == *observed_host_scope
                && current.system_proxy.observed_port == *observed_port
        }
        ActionPreconditions::DisableStaleSystemProxy {
            core_state_changed_at,
            expected_port,
            ..
        } => {
            current.core.state == AgentCoreState::Stopped
                && current.core.state_changed_at == *core_state_changed_at
                && current.system_proxy.observed_enabled == Some(true)
                && current.system_proxy.observed_host_scope == AgentHostScope::Loopback
                && current.system_proxy.observed_port == Some(*expected_port)
                && current.system_proxy.expected_mixed_port == *expected_port
        }
    };
    valid
        .then_some(())
        .ok_or(AgentCommandError::NetworkStateChanged)
}

fn routing_impacts(mode: AgentRoutingMode) -> Vec<AgentImpact> {
    let mut impacts = vec![AgentImpact::ExistingConnectionsMayChange];
    impacts.push(match mode {
        AgentRoutingMode::Rule => AgentImpact::RestoreRuleRouting,
        AgentRoutingMode::Global => AgentImpact::AllTrafficUsesProxy,
        AgentRoutingMode::Direct => AgentImpact::TrafficMayBypassProxy,
    });
    impacts
}

pub(super) fn tun_target_is_applied(snapshot: &AgentNetworkSnapshot, target: bool) -> bool {
    snapshot.tun.desired_enabled == target
        && snapshot.tun.generated_runtime_enabled == Some(target)
        && snapshot.tun.applied_consistency == AgentAppliedState::Consistent
        && snapshot.core.state == AgentCoreState::Running
}

pub(super) fn verify_action(
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
    preconditions: &ActionPreconditions,
) -> bool {
    match (action, preconditions) {
        (
            AgentActionRequest::SetRoutingMode { mode },
            ActionPreconditions::SetRoutingMode { .. },
        ) => {
            snapshot.core.routing_mode == Some(*mode)
                && snapshot.core.observed_routing_mode == Some(*mode)
                && snapshot.core.applied_consistency == AgentAppliedState::Consistent
        }
        (
            AgentActionRequest::SetTunEnabled { enabled },
            ActionPreconditions::SetTunEnabled { selected_core, .. },
        ) => {
            tun_target_is_applied(snapshot, *enabled)
                && snapshot.core.selected_core == *selected_core
        }
        (
            AgentActionRequest::SetSystemProxyEnabled { enabled },
            ActionPreconditions::SetSystemProxyEnabled { expected_port, .. },
        ) => {
            snapshot.system_proxy.desired_enabled == *enabled
                && snapshot.system_proxy.observed_enabled == Some(*enabled)
                && (!*enabled
                    || (snapshot.system_proxy.observed_host_scope == AgentHostScope::Loopback
                        && snapshot.system_proxy.observed_port == Some(*expected_port)
                        && snapshot.system_proxy.matches_expected_endpoint == Some(true)))
        }
        (
            AgentActionRequest::SetServiceMode { enabled },
            ActionPreconditions::SetServiceMode {
                core_state_changed_at,
                selected_core,
                ..
            },
        ) => {
            snapshot.service.desired_enabled == *enabled
                && snapshot.service.state == AgentServiceState::Running
                && snapshot.service.ipc_connected
                && snapshot.service.runtime_compatible == Some(true)
                && snapshot.core.state == AgentCoreState::Running
                && snapshot.core.state_changed_at > *core_state_changed_at
                && snapshot.core.selected_core == *selected_core
                && if *enabled {
                    snapshot.core.run_type == AgentRunType::Service
                } else {
                    snapshot.core.run_type != AgentRunType::Service
                }
        }
        (
            AgentActionRequest::StartCore,
            ActionPreconditions::StartCore {
                core_state_changed_at,
                selected_core,
            },
        ) => {
            snapshot.core.state == AgentCoreState::Running
                && snapshot.core.state_changed_at > *core_state_changed_at
                && snapshot.core.selected_core == *selected_core
                && snapshot.core.runtime_config_present
        }
        (
            AgentActionRequest::RestartCore,
            ActionPreconditions::RestartCore {
                core_state_changed_at,
                selected_core,
            },
        ) => {
            snapshot.core.state == AgentCoreState::Running
                && snapshot.core.state_changed_at > *core_state_changed_at
                && snapshot.core.selected_core == *selected_core
                && snapshot.core.runtime_config_present
        }
        (
            AgentActionRequest::ReconnectTelemetry,
            ActionPreconditions::ReconnectTelemetry {
                core_state_changed_at,
                selected_core,
            },
        ) => {
            snapshot.core.state == AgentCoreState::Running
                && snapshot.core.state_changed_at == *core_state_changed_at
                && snapshot.core.selected_core == *selected_core
                && snapshot.telemetry.state == AgentConnectorState::Connected
        }
        (
            AgentActionRequest::StartService,
            ActionPreconditions::ControlService {
                core_state_changed_at,
                ..
            },
        ) => {
            snapshot.service.desired_enabled
                && snapshot.service.state == AgentServiceState::Running
                && snapshot.service.ipc_connected
                && snapshot.service.runtime_compatible == Some(true)
                && snapshot.core.state == AgentCoreState::Running
                && snapshot.core.run_type == AgentRunType::Service
                && snapshot.core.state_changed_at > *core_state_changed_at
        }
        (AgentActionRequest::StopService, ActionPreconditions::ControlService { .. }) => {
            !snapshot.service.desired_enabled
                && snapshot.service.state == AgentServiceState::Stopped
                && !snapshot.service.ipc_connected
        }
        (
            AgentActionRequest::RestartService,
            ActionPreconditions::ControlService {
                core_state_changed_at,
                ..
            },
        ) => {
            snapshot.service.desired_enabled
                && snapshot.service.state == AgentServiceState::Running
                && snapshot.service.ipc_connected
                && snapshot.service.runtime_compatible == Some(true)
                && snapshot.core.state == AgentCoreState::Running
                && snapshot.core.run_type == AgentRunType::Service
                && snapshot.core.state_changed_at > *core_state_changed_at
        }
        (
            AgentActionRequest::RepairSystemProxyEndpoint,
            ActionPreconditions::RepairSystemProxyEndpoint { expected_port, .. },
        ) => {
            snapshot.core.state == AgentCoreState::Running
                && snapshot.system_proxy.desired_enabled
                && snapshot.system_proxy.observed_enabled == Some(true)
                && snapshot.system_proxy.observed_host_scope == AgentHostScope::Loopback
                && snapshot.system_proxy.observed_port == Some(*expected_port)
                && snapshot.system_proxy.matches_expected_endpoint == Some(true)
        }
        (
            AgentActionRequest::DisableStaleSystemProxy,
            ActionPreconditions::DisableStaleSystemProxy { .. },
        ) => {
            snapshot.system_proxy.observed_enabled == Some(false)
                && !snapshot.system_proxy.desired_enabled
        }
        _ => false,
    }
}
