use std::time::{Duration, Instant};

use super::{
    ActionPreconditions, PendingProposal, ProposalStore, cleanup_store, enforce_store_limits,
    execute_pending, is_fixed_lower_hex, plan_action, plan_proxy_endpoint_repair,
    plan_reconnect_telemetry, plan_restart_core, plan_routing_mode, plan_service_control,
    plan_service_mode_change, plan_start_core, plan_system_proxy_change, plan_tun_change,
    proposal_digest, recommendations, take_owned_proposal, tun_impacts, validate_preconditions,
    verify_action,
};
use crate::features::agent::{
    history::AgentAuditOutcome,
    model::{
        AgentActionRequest, AgentAppliedState, AgentCommandError, AgentConnectorState,
        AgentCoreSnapshot, AgentCoreState, AgentHealth, AgentHostScope, AgentImpact,
        AgentNetworkSnapshot, AgentOsFamily, AgentPrivacyBoundary, AgentProfileSnapshot,
        AgentProposal, AgentRoutingMode, AgentRunType, AgentSelectedCore, AgentServiceSnapshot,
        AgentServiceState, AgentStateField, AgentStateValue, AgentSystemProxySnapshot,
        AgentTelemetrySnapshot, AgentTunSnapshot,
    },
    ports::{AgentConfirmationPort, AgentRuntimePort},
};

struct NoopRuntime;

#[async_trait::async_trait]
impl AgentRuntimePort for NoopRuntime {
    async fn snapshot(&self) -> AgentNetworkSnapshot {
        snapshot()
    }

    async fn set_tun_enabled(&self, _before: bool, _target: bool) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn set_system_proxy_enabled(
        &self,
        _before: bool,
        _target: bool,
    ) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn set_service_mode(
        &self,
        _before: bool,
        _target: bool,
    ) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn ensure_core_running(&self) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn restart_core(&self) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn reconnect_telemetry(&self) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn control_service(&self, _action: &AgentActionRequest) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn set_routing_mode(
        &self,
        _before: AgentRoutingMode,
        _target: AgentRoutingMode,
    ) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn repair_system_proxy_endpoint(
        &self,
        _snapshot: &AgentNetworkSnapshot,
        _expected_port: u16,
        _desired_before: bool,
    ) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }

    async fn disable_stale_system_proxy(
        &self,
        _snapshot: &AgentNetworkSnapshot,
        _expected_port: u16,
        _desired_before: bool,
    ) -> Result<(), AgentCommandError> {
        Err(AgentCommandError::ActionFailed)
    }
}

struct PendingConfirmation;

#[async_trait::async_trait]
impl AgentConfirmationPort for PendingConfirmation {
    async fn confirm(
        &self,
        _owner_label: &str,
        _proposal: &AgentProposal,
    ) -> Result<bool, AgentCommandError> {
        std::future::pending().await
    }

    async fn confirm_history_clear(&self, _owner_label: &str) -> Result<bool, AgentCommandError> {
        std::future::pending().await
    }
}

#[tokio::test]
async fn confirmation_is_bounded_by_the_proposal_expiry() {
    let mut pending = pending("owner", Instant::now() + Duration::from_millis(20));
    pending.proposal.digest = "digest".into();

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        execute_pending(
            &NoopRuntime,
            &PendingConfirmation,
            "owner",
            pending,
            "digest",
        ),
    )
    .await
    .expect("confirmation expiry must remain bounded");

    assert!(matches!(result, Err(AgentCommandError::ProposalExpired)));
}

#[test]
fn invalid_proposal_references_do_not_consume_a_valid_pending_proposal() {
    let now = Instant::now();
    let mut store = ProposalStore::default();
    let mut valid = pending("owner", now + Duration::from_secs(30));
    valid.proposal.digest = "correct".into();
    store.pending.insert("proposal".into(), valid);

    assert!(matches!(
        take_owned_proposal(&mut store, "owner", "proposal", "wrong"),
        Err(AgentCommandError::ProposalDigestMismatch)
    ));
    assert!(store.pending.contains_key("proposal"));

    assert!(matches!(
        take_owned_proposal(&mut store, "other", "proposal", "correct"),
        Err(AgentCommandError::ProposalNotFound)
    ));
    assert!(store.pending.contains_key("proposal"));

    let consumed = take_owned_proposal(&mut store, "owner", "proposal", "correct")
        .expect("correct reference consumes proposal once");
    assert_eq!(consumed.owner_label, "owner");
    assert!(!store.pending.contains_key("proposal"));
}

#[test]
fn proposal_references_accept_only_fixed_lower_hex() {
    assert!(is_fixed_lower_hex("0123456789abcdef0123456789abcdef", 32));
    assert!(is_fixed_lower_hex(&"a".repeat(64), 64));

    for invalid in [
        "",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789ABCDEF0123456789ABCDEF",
        "0123456789abcdef0123456789abcdeg",
    ] {
        assert!(!is_fixed_lower_hex(invalid, 32), "{invalid}");
    }
}

#[test]
fn unknown_core_state_disables_every_write_plan_and_recommendation() {
    let mut snapshot = snapshot();
    snapshot.core.state = AgentCoreState::Unknown;
    snapshot.core.run_type = AgentRunType::Unknown;

    let actions = [
        AgentActionRequest::SetRoutingMode {
            mode: AgentRoutingMode::Global,
        },
        AgentActionRequest::SetTunEnabled { enabled: true },
        AgentActionRequest::SetSystemProxyEnabled { enabled: true },
        AgentActionRequest::StartCore,
        AgentActionRequest::RestartCore,
        AgentActionRequest::ReconnectTelemetry,
        AgentActionRequest::StartService,
        AgentActionRequest::StopService,
        AgentActionRequest::RestartService,
        AgentActionRequest::RepairSystemProxyEndpoint,
        AgentActionRequest::DisableStaleSystemProxy,
    ];

    for action in actions {
        assert!(matches!(
            plan_action(&snapshot, &action),
            Err(AgentCommandError::ActionNotAvailable)
        ));
    }
    let recommendations = recommendations(&snapshot);
    assert!(!recommendations.is_empty());
    assert!(recommendations.iter().all(|recommendation| {
        !recommendation.available
            && recommendation.unavailable_reason
                == Some(crate::features::agent::model::AgentRecommendationUnavailableReason::CurrentStateNotSupported)
            && recommendation.risk.is_none()
            && recommendation.impacts.is_empty()
    }));
}

#[test]
fn action_audit_tracing_contains_only_stable_fields() {
    let source = include_str!("actions.rs");
    let marker = ["tracing", "::"].concat();
    let start = source.find(&marker).expect("expected action audit tracing");
    let remaining = &source[start..];
    let end = remaining
        .find(");")
        .expect("action audit tracing must terminate");
    let invocation = &remaining[..end + 2];

    assert!(invocation.contains("action"));
    assert!(invocation.contains("outcome"));
    for forbidden in [
        "proposal_id",
        "snapshot_revision",
        "digest",
        "owner_label",
        "?error",
        "%error",
        "token",
        "url",
        "changes",
    ] {
        assert!(
            !invocation.contains(forbidden),
            "action audit tracing must not include {forbidden}: {invocation}"
        );
    }
    assert!(
        remaining[end + 2..].find(&marker).is_none(),
        "unexpected additional action tracing invocation"
    );
}

#[test]
fn command_errors_map_to_closed_audit_outcomes() {
    let cases = [
        (
            AgentCommandError::ActionNotAvailable,
            AgentAuditOutcome::ActionNotAvailable,
        ),
        (
            AgentCommandError::ProposalNotFound,
            AgentAuditOutcome::ProposalNotFound,
        ),
        (
            AgentCommandError::ProposalExpired,
            AgentAuditOutcome::ProposalExpired,
        ),
        (
            AgentCommandError::ProposalDigestMismatch,
            AgentAuditOutcome::DigestMismatch,
        ),
        (
            AgentCommandError::NetworkStateChanged,
            AgentAuditOutcome::StateChanged,
        ),
        (
            AgentCommandError::ProposalRateLimited,
            AgentAuditOutcome::RateLimited,
        ),
        (
            AgentCommandError::ProposalLimitReached,
            AgentAuditOutcome::LimitReached,
        ),
        (
            AgentCommandError::ConfirmationDeclined,
            AgentAuditOutcome::ConfirmationDeclined,
        ),
        (
            AgentCommandError::ActionFailed,
            AgentAuditOutcome::ActionFailed,
        ),
        (
            AgentCommandError::PartialApply,
            AgentAuditOutcome::PartialApply,
        ),
        (
            AgentCommandError::VerificationFailed,
            AgentAuditOutcome::VerificationFailed,
        ),
        (
            AgentCommandError::BridgeStartFailed,
            AgentAuditOutcome::BridgeStartFailed,
        ),
        (
            AgentCommandError::HistoryClearFailed,
            AgentAuditOutcome::HistoryClearFailed,
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.audit_outcome(), expected);
    }
}

#[test]
fn proposal_digest_binds_action_revision_and_expiry() {
    let action = AgentActionRequest::SetRoutingMode {
        mode: AgentRoutingMode::Rule,
    };
    let digest = proposal_digest("id", &action, "revision", 123).expect("digest");
    assert_ne!(
        digest,
        proposal_digest("id", &action, "changed", 123).expect("changed revision digest")
    );
    assert_ne!(
        digest,
        proposal_digest("id", &action, "revision", 124).expect("changed expiry digest")
    );
}

#[test]
fn routing_plan_requires_matching_observed_mode() {
    let mut snapshot = snapshot();
    snapshot.core.observed_routing_mode = None;
    assert!(plan_routing_mode(&snapshot, AgentRoutingMode::Global).is_err());
    snapshot.core.observed_routing_mode = Some(AgentRoutingMode::Rule);
    assert!(plan_routing_mode(&snapshot, AgentRoutingMode::Global).is_ok());
}

#[test]
fn routing_verification_checks_configured_and_observed_modes() {
    let mut snapshot = snapshot();
    let action = AgentActionRequest::SetRoutingMode {
        mode: AgentRoutingMode::Rule,
    };
    let preconditions = ActionPreconditions::SetRoutingMode {
        before: AgentRoutingMode::Global,
        core_state_changed_at: 0,
    };
    assert!(verify_action(&snapshot, &action, &preconditions));
    snapshot.core.observed_routing_mode = Some(AgentRoutingMode::Direct);
    assert!(!verify_action(&snapshot, &action, &preconditions));
}

#[test]
fn service_mode_plan_requires_a_distinct_target_and_healthy_service() {
    let mut current = snapshot();
    current.service = AgentServiceSnapshot {
        desired_enabled: false,
        state: AgentServiceState::Running,
        ipc_connected: true,
        runtime_compatible: Some(true),
    };

    let plan = plan_service_mode_change(&current, true)
        .expect("service mode enable should be available from normal mode");
    assert_eq!(plan.changes[0].field, AgentStateField::ServiceMode);
    assert_eq!(plan.changes[0].before, AgentStateValue::Disabled);
    assert_eq!(plan.changes[0].after, AgentStateValue::Enabled);
    assert!(validate_preconditions(&current, &plan.preconditions).is_ok());
    assert!(plan_service_mode_change(&current, false).is_err());

    current.core.run_type = AgentRunType::Service;
    assert!(
        plan_service_mode_change(&current, true).is_ok(),
        "an inconsistent source run type remains repairable under new-instance verification"
    );
    current.core.run_type = AgentRunType::Normal;
    current.service.ipc_connected = false;
    assert!(plan_service_mode_change(&current, true).is_err());
    current.service.ipc_connected = true;
    current.service.runtime_compatible = Some(false);
    assert!(plan_service_mode_change(&current, true).is_err());
}

#[test]
fn service_mode_verification_requires_a_new_core_instance_for_both_targets() {
    let mut normal = snapshot();
    normal.service = AgentServiceSnapshot {
        desired_enabled: false,
        state: AgentServiceState::Running,
        ipc_connected: true,
        runtime_compatible: Some(true),
    };
    let enable_plan = plan_service_mode_change(&normal, true).expect("enable plan");
    let enable_action = AgentActionRequest::SetServiceMode { enabled: true };
    let mut enabled = normal.clone();
    enabled.service.desired_enabled = true;
    enabled.core.run_type = AgentRunType::Service;
    assert!(
        !verify_action(&enabled, &enable_action, &enable_plan.preconditions),
        "changing only the desired flag and run type must not verify the same instance"
    );
    enabled.core.state_changed_at += 1;
    assert!(verify_action(
        &enabled,
        &enable_action,
        &enable_plan.preconditions
    ));

    let mut inconsistent = normal.clone();
    inconsistent.core.run_type = AgentRunType::Service;
    let repair_plan = plan_service_mode_change(&inconsistent, true).expect("repair plan");
    inconsistent.service.desired_enabled = true;
    assert!(!verify_action(
        &inconsistent,
        &enable_action,
        &repair_plan.preconditions
    ));
    inconsistent.core.state_changed_at += 1;
    assert!(verify_action(
        &inconsistent,
        &enable_action,
        &repair_plan.preconditions
    ));

    let disable_plan = plan_service_mode_change(&enabled, false).expect("disable plan");
    let disable_action = AgentActionRequest::SetServiceMode { enabled: false };
    let mut disabled = enabled;
    disabled.service.desired_enabled = false;
    disabled.core.run_type = AgentRunType::Normal;
    assert!(!verify_action(
        &disabled,
        &disable_action,
        &disable_plan.preconditions
    ));
    disabled.core.state_changed_at += 1;
    assert!(verify_action(
        &disabled,
        &disable_action,
        &disable_plan.preconditions
    ));
    disabled.service.state = AgentServiceState::Stopped;
    assert!(!verify_action(
        &disabled,
        &disable_action,
        &disable_plan.preconditions
    ));
}

#[test]
fn tun_plan_requires_a_consistent_running_source_and_explicit_target() {
    let mut snapshot = snapshot();
    snapshot.os_family = AgentOsFamily::Linux;
    let plan = plan_tun_change(&snapshot, true).expect("TUN enable should be available");

    assert_eq!(
        plan.impacts,
        vec![
            AgentImpact::ExistingConnectionsMayChange,
            AgentImpact::CoreMayRestart,
            AgentImpact::AdminPermissionMayBeRequired,
        ]
    );
    assert_eq!(plan.changes.len(), 1);
    assert_eq!(plan.changes[0].field, AgentStateField::Tun);
    assert_eq!(plan.changes[0].before, AgentStateValue::Disabled);
    assert_eq!(plan.changes[0].after, AgentStateValue::Enabled);

    assert!(plan_tun_change(&snapshot, false).is_err());
    snapshot.core.state = AgentCoreState::Stopped;
    assert!(plan_tun_change(&snapshot, true).is_err());
    snapshot.core.state = AgentCoreState::Running;
    snapshot.tun.generated_runtime_enabled = Some(true);
    assert!(plan_tun_change(&snapshot, true).is_err());
}

#[test]
fn tun_preconditions_reject_core_service_and_runtime_drift() {
    let snapshot = snapshot();
    let plan = plan_tun_change(&snapshot, true).expect("TUN enable should be available");
    assert!(validate_preconditions(&snapshot, &plan.preconditions).is_ok());

    let mut changed = snapshot.clone();
    changed.core.selected_core = AgentSelectedCore::ClashRs;
    assert!(validate_preconditions(&changed, &plan.preconditions).is_err());

    let mut changed = snapshot.clone();
    changed.service.ipc_connected = true;
    assert!(validate_preconditions(&changed, &plan.preconditions).is_err());

    let mut changed = snapshot;
    changed.tun.generated_runtime_enabled = None;
    assert!(validate_preconditions(&changed, &plan.preconditions).is_err());
}

#[test]
fn tun_verification_requires_the_desired_generated_and_core_state() {
    let before = snapshot();
    let plan = plan_tun_change(&before, true).expect("TUN enable should be available");
    let action = AgentActionRequest::SetTunEnabled { enabled: true };
    let mut applied = before;
    applied.tun.desired_enabled = true;
    applied.tun.generated_runtime_enabled = Some(true);
    applied.tun.applied_consistency = AgentAppliedState::Consistent;

    assert!(verify_action(&applied, &action, &plan.preconditions));
    applied.tun.generated_runtime_enabled = Some(false);
    assert!(!verify_action(&applied, &action, &plan.preconditions));
    applied.tun.generated_runtime_enabled = Some(true);
    applied.core.state = AgentCoreState::Stopped;
    assert!(!verify_action(&applied, &action, &plan.preconditions));
}

#[test]
fn system_proxy_plan_requires_consistent_observed_state_and_verifies_endpoint() {
    let mut current = snapshot();
    let plan =
        plan_system_proxy_change(&current, true).expect("system proxy enable should be available");
    assert_eq!(plan.impacts, vec![AgentImpact::HostSystemProxyEnabled]);
    assert_eq!(plan.changes[0].field, AgentStateField::SystemProxy);
    assert!(validate_preconditions(&current, &plan.preconditions).is_ok());

    current.system_proxy.desired_enabled = true;
    current.system_proxy.observed_enabled = Some(true);
    assert!(verify_action(
        &current,
        &AgentActionRequest::SetSystemProxyEnabled { enabled: true },
        &plan.preconditions,
    ));
    current.system_proxy.matches_expected_endpoint = Some(false);
    assert!(!verify_action(
        &current,
        &AgentActionRequest::SetSystemProxyEnabled { enabled: true },
        &plan.preconditions,
    ));

    let mut unavailable = snapshot();
    unavailable.core.state = AgentCoreState::Stopped;
    assert!(plan_system_proxy_change(&unavailable, true).is_err());
    unavailable.core.state = AgentCoreState::Running;
    unavailable.system_proxy.observed_enabled = None;
    assert!(plan_system_proxy_change(&unavailable, true).is_err());
}

#[test]
fn recommendations_reuse_the_action_planner_and_include_tun() {
    let snapshot = snapshot();
    let recommendations = super::recommendations(&snapshot);
    let tun = recommendations
        .iter()
        .find(|recommendation| {
            recommendation.action == AgentActionRequest::SetTunEnabled { enabled: true }
        })
        .expect("expected TUN recommendation");
    assert!(tun.available);
    assert!(tun.risk.is_some());
    assert!(recommendations.iter().any(|recommendation| {
        recommendation.action
            == AgentActionRequest::SetRoutingMode {
                mode: AgentRoutingMode::Rule,
            }
            && !recommendation.available
            && recommendation.unavailable_reason.is_some()
    }));
}

#[test]
fn tun_impacts_are_platform_specific_and_closed() {
    assert_eq!(
        tun_impacts(AgentOsFamily::Macos),
        vec![
            AgentImpact::ExistingConnectionsMayChange,
            AgentImpact::CoreMayRestart,
            AgentImpact::HostDnsMayChange,
            AgentImpact::AdminPermissionMayBeRequired,
        ]
    );
    assert_eq!(
        tun_impacts(AgentOsFamily::Windows),
        vec![
            AgentImpact::ExistingConnectionsMayChange,
            AgentImpact::CoreMayRestart,
        ]
    );
}

#[test]
fn start_core_plan_requires_a_stopped_core_and_valid_active_profile() {
    let mut snapshot = snapshot();
    snapshot.core.state = AgentCoreState::Stopped;
    assert!(plan_start_core(&snapshot).is_ok());
    snapshot.core.state = AgentCoreState::Running;
    assert!(plan_start_core(&snapshot).is_err());
    snapshot.core.state = AgentCoreState::Stopped;
    snapshot.profiles.active_references_valid = false;
    assert!(plan_start_core(&snapshot).is_err());
}

#[test]
fn start_core_preconditions_and_verification_require_a_new_running_instance() {
    let mut snapshot = snapshot();
    snapshot.core.state = AgentCoreState::Stopped;
    let plan = plan_start_core(&snapshot).expect("stopped core should be startable");
    assert!(validate_preconditions(&snapshot, &plan.preconditions).is_ok());

    let action = AgentActionRequest::StartCore;
    assert!(!verify_action(&snapshot, &action, &plan.preconditions));

    let mut running = snapshot.clone();
    running.core.state = AgentCoreState::Running;
    running.core.state_changed_at += 1;
    assert!(verify_action(&running, &action, &plan.preconditions));

    running.core.selected_core = AgentSelectedCore::ClashRs;
    assert!(!verify_action(&running, &action, &plan.preconditions));

    let mut drifted = snapshot;
    drifted.core.state_changed_at += 1;
    assert!(validate_preconditions(&drifted, &plan.preconditions).is_err());
}

#[test]
fn restart_core_plan_requires_a_running_core_and_valid_active_profile() {
    let mut snapshot = snapshot();
    assert!(plan_restart_core(&snapshot).is_ok());
    snapshot.core.state = AgentCoreState::Stopped;
    assert!(plan_restart_core(&snapshot).is_err());
    snapshot.core.state = AgentCoreState::Running;
    snapshot.profiles.active_references_valid = false;
    assert!(plan_restart_core(&snapshot).is_err());
}

#[test]
fn restart_core_verification_requires_a_new_running_instance() {
    let mut snapshot = snapshot();
    let action = AgentActionRequest::RestartCore;
    let preconditions = ActionPreconditions::RestartCore {
        core_state_changed_at: snapshot.core.state_changed_at,
        selected_core: snapshot.core.selected_core,
    };
    assert!(!verify_action(&snapshot, &action, &preconditions));
    snapshot.core.state_changed_at += 1;
    assert!(verify_action(&snapshot, &action, &preconditions));
    snapshot.core.selected_core = AgentSelectedCore::ClashRs;
    assert!(!verify_action(&snapshot, &action, &preconditions));
}

#[test]
fn telemetry_reconnect_requires_a_running_core_and_disconnected_connector() {
    let mut snapshot = snapshot();
    snapshot.telemetry.state = AgentConnectorState::Disconnected;
    assert!(plan_reconnect_telemetry(&snapshot).is_ok());

    snapshot.telemetry.state = AgentConnectorState::Connecting;
    assert!(plan_reconnect_telemetry(&snapshot).is_err());
    snapshot.telemetry.state = AgentConnectorState::Disconnected;
    snapshot.core.state = AgentCoreState::Stopped;
    assert!(plan_reconnect_telemetry(&snapshot).is_err());
}

#[test]
fn telemetry_reconnect_verification_preserves_the_core_instance() {
    let mut snapshot = snapshot();
    let action = AgentActionRequest::ReconnectTelemetry;
    let preconditions = ActionPreconditions::ReconnectTelemetry {
        core_state_changed_at: snapshot.core.state_changed_at,
        selected_core: snapshot.core.selected_core,
    };
    snapshot.telemetry.state = AgentConnectorState::Disconnected;
    assert!(!verify_action(&snapshot, &action, &preconditions));
    snapshot.telemetry.state = AgentConnectorState::Connected;
    assert!(verify_action(&snapshot, &action, &preconditions));
    snapshot.core.selected_core = AgentSelectedCore::ClashRs;
    assert!(!verify_action(&snapshot, &action, &preconditions));
}

#[test]
fn service_plans_require_safe_consistent_states() {
    let mut snapshot = snapshot();
    snapshot.service.desired_enabled = true;
    snapshot.service.state = AgentServiceState::Stopped;
    assert!(plan_service_control(&snapshot, &AgentActionRequest::StartService).is_ok());
    assert!(plan_service_control(&snapshot, &AgentActionRequest::StopService).is_err());

    snapshot.service.desired_enabled = false;
    snapshot.service.state = AgentServiceState::Running;
    assert!(plan_service_control(&snapshot, &AgentActionRequest::StopService).is_ok());
    snapshot.core.run_type = AgentRunType::Service;
    assert!(plan_service_control(&snapshot, &AgentActionRequest::StopService).is_err());

    snapshot.service.desired_enabled = true;
    snapshot.service.ipc_connected = true;
    snapshot.service.runtime_compatible = Some(true);
    assert!(plan_service_control(&snapshot, &AgentActionRequest::RestartService).is_ok());
    snapshot.service.runtime_compatible = Some(false);
    assert!(plan_service_control(&snapshot, &AgentActionRequest::RestartService).is_err());
}

#[test]
fn service_verification_requires_stable_ipc_and_a_new_service_core() {
    let mut snapshot = snapshot();
    let preconditions = ActionPreconditions::ControlService {
        state_before: AgentServiceState::Stopped,
        desired_enabled: true,
        ipc_connected: false,
        runtime_compatible: None,
        core_state: AgentCoreState::Running,
        core_run_type: AgentRunType::Normal,
        core_state_changed_at: 0,
    };
    snapshot.service = AgentServiceSnapshot {
        desired_enabled: true,
        state: AgentServiceState::Running,
        ipc_connected: true,
        runtime_compatible: Some(true),
    };
    snapshot.core.run_type = AgentRunType::Service;
    assert!(!verify_action(
        &snapshot,
        &AgentActionRequest::StartService,
        &preconditions,
    ));
    snapshot.core.state_changed_at = 1;
    assert!(verify_action(
        &snapshot,
        &AgentActionRequest::StartService,
        &preconditions,
    ));
    snapshot.service.ipc_connected = false;
    assert!(!verify_action(
        &snapshot,
        &AgentActionRequest::StartService,
        &preconditions,
    ));
}

#[test]
fn stop_and_restart_service_verify_their_distinct_final_states() {
    let mut snapshot = snapshot();
    let stop_preconditions = ActionPreconditions::ControlService {
        state_before: AgentServiceState::Running,
        desired_enabled: false,
        ipc_connected: true,
        runtime_compatible: Some(true),
        core_state: AgentCoreState::Running,
        core_run_type: AgentRunType::Normal,
        core_state_changed_at: 0,
    };
    snapshot.service = AgentServiceSnapshot {
        desired_enabled: false,
        state: AgentServiceState::Stopped,
        ipc_connected: false,
        runtime_compatible: None,
    };
    assert!(verify_action(
        &snapshot,
        &AgentActionRequest::StopService,
        &stop_preconditions,
    ));

    let restart_preconditions = ActionPreconditions::ControlService {
        state_before: AgentServiceState::Running,
        desired_enabled: true,
        ipc_connected: true,
        runtime_compatible: Some(true),
        core_state: AgentCoreState::Running,
        core_run_type: AgentRunType::Service,
        core_state_changed_at: 10,
    };
    snapshot.service = AgentServiceSnapshot {
        desired_enabled: true,
        state: AgentServiceState::Running,
        ipc_connected: true,
        runtime_compatible: Some(true),
    };
    snapshot.core.run_type = AgentRunType::Service;
    snapshot.core.state_changed_at = 10;
    assert!(!verify_action(
        &snapshot,
        &AgentActionRequest::RestartService,
        &restart_preconditions,
    ));
    snapshot.core.state_changed_at = 11;
    assert!(verify_action(
        &snapshot,
        &AgentActionRequest::RestartService,
        &restart_preconditions,
    ));
}

#[test]
fn proxy_endpoint_repair_requires_running_core_and_a_real_mismatch() {
    let mut snapshot = snapshot();
    snapshot.system_proxy.desired_enabled = true;
    snapshot.system_proxy.observed_enabled = Some(true);
    snapshot.system_proxy.observed_port = Some(7891);
    snapshot.system_proxy.matches_expected_endpoint = Some(false);
    assert!(plan_proxy_endpoint_repair(&snapshot).is_ok());

    snapshot.core.state = AgentCoreState::Stopped;
    assert!(plan_proxy_endpoint_repair(&snapshot).is_err());
    snapshot.core.state = AgentCoreState::Running;
    snapshot.system_proxy.matches_expected_endpoint = Some(true);
    assert!(plan_proxy_endpoint_repair(&snapshot).is_err());
}

#[test]
fn proxy_endpoint_repair_verifies_the_expected_loopback_endpoint() {
    let mut snapshot = snapshot();
    snapshot.system_proxy.desired_enabled = true;
    snapshot.system_proxy.observed_enabled = Some(true);
    let preconditions = ActionPreconditions::RepairSystemProxyEndpoint {
        core_state_changed_at: snapshot.core.state_changed_at,
        expected_port: 7890,
        observed_host_scope: AgentHostScope::Loopback,
        observed_port: Some(7891),
        desired_before: true,
    };
    assert!(verify_action(
        &snapshot,
        &AgentActionRequest::RepairSystemProxyEndpoint,
        &preconditions,
    ));
    snapshot.system_proxy.observed_port = Some(7891);
    snapshot.system_proxy.matches_expected_endpoint = Some(false);
    assert!(!verify_action(
        &snapshot,
        &AgentActionRequest::RepairSystemProxyEndpoint,
        &preconditions,
    ));
}

#[test]
fn proposal_cleanup_uses_monotonic_expiry_and_enforces_owner_limit() {
    let now = Instant::now();
    let mut store = ProposalStore::default();
    for index in 0..4 {
        let expires_at = if index == 0 {
            now - Duration::from_millis(1)
        } else {
            now + Duration::from_secs(30)
        };
        store
            .pending
            .insert(index.to_string(), pending("main", expires_at));
    }
    cleanup_store(&mut store, now);
    assert_eq!(store.pending.len(), 3);
    assert!(enforce_store_limits(&store, "main").is_ok());
    store.pending.insert(
        "fourth".into(),
        pending("main", now + Duration::from_secs(30)),
    );
    assert!(enforce_store_limits(&store, "main").is_err());
}

fn pending(owner_label: &str, expires_at: Instant) -> PendingProposal {
    PendingProposal {
        proposal: AgentProposal {
            id: "proposal".into(),
            digest: "digest".into(),
            action: AgentActionRequest::SetRoutingMode {
                mode: AgentRoutingMode::Global,
            },
            risk: crate::features::agent::model::AgentActionRisk::TrafficChange,
            impacts: Vec::new(),
            changes: Vec::new(),
            snapshot_revision: "revision".into(),
            created_at: 0,
            expires_at: 1,
            requires_confirmation: true,
        },
        preconditions: ActionPreconditions::SetRoutingMode {
            before: AgentRoutingMode::Rule,
            core_state_changed_at: 0,
        },
        owner_label: owner_label.into(),
        expires_at,
    }
}

fn snapshot() -> AgentNetworkSnapshot {
    AgentNetworkSnapshot {
        schema_version: 1,
        revision: "revision".into(),
        captured_at: 0,
        app_version: "test".into(),
        os_family: AgentOsFamily::Unknown,
        health: AgentHealth::Healthy,
        core: AgentCoreSnapshot {
            state: AgentCoreState::Running,
            run_type: AgentRunType::Normal,
            selected_core: AgentSelectedCore::Mihomo,
            state_changed_at: 0,
            runtime_config_present: true,
            routing_mode: Some(AgentRoutingMode::Rule),
            observed_routing_mode: Some(AgentRoutingMode::Rule),
            applied_consistency: AgentAppliedState::Consistent,
        },
        service: AgentServiceSnapshot {
            desired_enabled: false,
            state: AgentServiceState::NotInstalled,
            ipc_connected: false,
            runtime_compatible: None,
        },
        system_proxy: AgentSystemProxySnapshot {
            desired_enabled: false,
            observed_enabled: Some(false),
            observed_host_scope: AgentHostScope::Loopback,
            observed_port: Some(7890),
            expected_mixed_port: 7890,
            matches_expected_endpoint: Some(true),
        },
        tun: AgentTunSnapshot {
            desired_enabled: false,
            generated_runtime_enabled: Some(false),
            observed_active: AgentAppliedState::Unknown,
            applied_consistency: AgentAppliedState::Unknown,
        },
        profiles: AgentProfileSnapshot {
            total_count: 1,
            active_count: 1,
            remote_count: 0,
            local_count: 1,
            active_references_valid: true,
        },
        telemetry: AgentTelemetrySnapshot {
            state: AgentConnectorState::Connected,
            active_connection_count: Some(0),
            upload_speed: Some(0),
            download_speed: Some(0),
            upload_total: Some(0),
            download_total: Some(0),
            recent_error_count: 0,
        },
        findings: Vec::new(),
        probe_failures: Vec::new(),
        recommendations: Vec::new(),
        privacy: AgentPrivacyBoundary {
            contains_raw_logs: false,
            contains_profile_names: false,
            contains_profile_urls: false,
            contains_connection_targets: false,
            contains_controller_secret: false,
        },
    }
}
