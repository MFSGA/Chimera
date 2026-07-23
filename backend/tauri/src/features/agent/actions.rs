use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use serde_yaml::{Mapping, Value};
use sha2::Digest;
use sysproxy::Sysproxy;
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::Mutex;

use crate::{config::chimera::IVerge, feat};

use super::{
    collect_network_snapshot, core_probe,
    diagnostics::host_scope,
    model::{
        AgentActionRequest, AgentActionResult, AgentActionRisk, AgentAppliedState,
        AgentCommandError, AgentCoreState, AgentHostScope, AgentImpact, AgentNetworkSnapshot,
        AgentProposal, AgentResult, AgentRoutingMode, AgentStateChange,
    },
};

const PROPOSAL_TTL: Duration = Duration::from_secs(60);
const MIN_PROPOSAL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_PENDING_PROPOSALS: usize = 24;
const MAX_PENDING_PER_OWNER: usize = 4;
const CORE_ACTION_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
enum ActionPreconditions {
    SetRoutingMode {
        before: AgentRoutingMode,
        core_state_changed_at: i64,
    },
    DisableStaleSystemProxy {
        core_state_changed_at: i64,
        expected_port: u16,
        desired_before: bool,
    },
}

#[derive(Debug, Clone)]
pub(super) struct PendingProposal {
    proposal: AgentProposal,
    preconditions: ActionPreconditions,
    owner_label: String,
    expires_at: Instant,
}

#[derive(Default)]
pub(super) struct ProposalStore {
    pending: HashMap<String, PendingProposal>,
    last_proposed_at: HashMap<String, Instant>,
}

pub(crate) struct AgentFeatureState {
    pub(super) proposals: Mutex<ProposalStore>,
    pub(super) execution: Mutex<()>,
}

struct ActionPlan {
    risk: AgentActionRisk,
    impacts: Vec<AgentImpact>,
    changes: Vec<AgentStateChange>,
    preconditions: ActionPreconditions,
}

impl AgentFeatureState {
    pub(crate) async fn propose(
        &self,
        app: &AppHandle,
        owner_label: &str,
        action: AgentActionRequest,
    ) -> AgentResult<AgentProposal> {
        self.reserve_proposal_slot(owner_label).await?;
        let snapshot = collect_network_snapshot(app).await;
        let plan = plan_action(&snapshot, &action)?;
        let created_at = chrono::Utc::now().timestamp_millis();
        let expires_at = created_at + PROPOSAL_TTL.as_millis() as i64;
        let id = nanoid::nanoid!();
        let digest = proposal_digest(&id, &action, &snapshot.revision, expires_at);
        let proposal = AgentProposal {
            id: id.clone(),
            digest,
            action,
            risk: plan.risk,
            impacts: plan.impacts,
            changes: plan.changes,
            snapshot_revision: snapshot.revision,
            created_at,
            expires_at,
            requires_confirmation: true,
        };
        self.insert_proposal(
            id,
            PendingProposal {
                proposal: proposal.clone(),
                preconditions: plan.preconditions,
                owner_label: owner_label.to_owned(),
                expires_at: Instant::now() + PROPOSAL_TTL,
            },
        )
        .await?;
        audit_proposal(&proposal, "proposed");
        Ok(proposal)
    }

    pub(crate) async fn execute(
        &self,
        app: &AppHandle,
        window: &WebviewWindow,
        proposal_id: &str,
        digest: &str,
    ) -> AgentResult<AgentActionResult> {
        let _execution = self.execution.lock().await;
        let pending = self.take_proposal(window.label(), proposal_id).await?;
        let result = execute_pending(app, window, pending.clone(), digest).await;
        let outcome = result
            .as_ref()
            .map(|_| "verified")
            .unwrap_or_else(|error| error.audit_code());
        audit_proposal(&pending.proposal, outcome);
        result
    }

    pub(crate) async fn cancel(&self, owner_label: &str, proposal_id: &str) -> bool {
        let mut store = self.proposals.lock().await;
        let is_owner = store
            .pending
            .get(proposal_id)
            .is_some_and(|pending| pending.owner_label == owner_label);
        if is_owner {
            store.pending.remove(proposal_id);
        }
        is_owner
    }

    async fn reserve_proposal_slot(&self, owner_label: &str) -> AgentResult<()> {
        let now = Instant::now();
        let mut store = self.proposals.lock().await;
        cleanup_store(&mut store, now);
        if store
            .last_proposed_at
            .get(owner_label)
            .is_some_and(|last| now.duration_since(*last) < MIN_PROPOSAL_INTERVAL)
        {
            return Err(AgentCommandError::ProposalRateLimited);
        }
        enforce_store_limits(&store, owner_label)?;
        store.last_proposed_at.insert(owner_label.to_owned(), now);
        Ok(())
    }

    async fn insert_proposal(&self, id: String, pending: PendingProposal) -> AgentResult<()> {
        let mut store = self.proposals.lock().await;
        cleanup_store(&mut store, Instant::now());
        enforce_store_limits(&store, &pending.owner_label)?;
        store.pending.insert(id, pending);
        Ok(())
    }

    async fn take_proposal(
        &self,
        owner_label: &str,
        proposal_id: &str,
    ) -> AgentResult<PendingProposal> {
        let mut store = self.proposals.lock().await;
        let is_owner = store
            .pending
            .get(proposal_id)
            .is_some_and(|pending| pending.owner_label == owner_label);
        if !is_owner {
            return Err(AgentCommandError::ProposalNotFound);
        }
        store
            .pending
            .remove(proposal_id)
            .ok_or(AgentCommandError::ProposalNotFound)
    }
}

impl AgentCommandError {
    fn audit_code(&self) -> &'static str {
        match self {
            Self::ActionNotAvailable => "action_not_available",
            Self::ProposalNotFound => "proposal_not_found",
            Self::ProposalExpired => "proposal_expired",
            Self::ProposalDigestMismatch => "digest_mismatch",
            Self::NetworkStateChanged => "state_changed",
            Self::ProposalRateLimited => "rate_limited",
            Self::ProposalLimitReached => "limit_reached",
            Self::ConfirmationDeclined => "confirmation_declined",
            Self::ActionFailed => "action_failed",
            Self::PartialApply => "partial_apply",
            Self::VerificationFailed => "verification_failed",
        }
    }
}

async fn execute_pending(
    app: &AppHandle,
    window: &WebviewWindow,
    pending: PendingProposal,
    digest: &str,
) -> AgentResult<AgentActionResult> {
    let proposal = &pending.proposal;
    if proposal.digest != digest {
        return Err(AgentCommandError::ProposalDigestMismatch);
    }
    if pending.expires_at <= Instant::now() {
        return Err(AgentCommandError::ProposalExpired);
    }
    if !confirm_network_change(window, proposal).await? {
        return Err(AgentCommandError::ConfirmationDeclined);
    }
    if pending.expires_at <= Instant::now() {
        return Err(AgentCommandError::ProposalExpired);
    }
    let current = collect_network_snapshot(app).await;
    validate_preconditions(&current, &pending.preconditions)?;
    execute_action(&current, &proposal.action, &pending.preconditions).await?;
    let snapshot = collect_network_snapshot(app).await;
    if !verify_action(&snapshot, &proposal.action) {
        return Err(AgentCommandError::VerificationFailed);
    }
    Ok(AgentActionResult {
        proposal_id: proposal.id.clone(),
        action: proposal.action.kind(),
        verified: true,
        snapshot,
    })
}

fn cleanup_store(store: &mut ProposalStore, now: Instant) {
    store.pending.retain(|_, pending| pending.expires_at > now);
    store
        .last_proposed_at
        .retain(|_, last| now.duration_since(*last) < PROPOSAL_TTL);
}

fn enforce_store_limits(store: &ProposalStore, owner_label: &str) -> AgentResult<()> {
    let owner_count = store
        .pending
        .values()
        .filter(|pending| pending.owner_label == owner_label)
        .count();
    if store.pending.len() >= MAX_PENDING_PROPOSALS || owner_count >= MAX_PENDING_PER_OWNER {
        return Err(AgentCommandError::ProposalLimitReached);
    }
    Ok(())
}

fn plan_action(
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
) -> AgentResult<ActionPlan> {
    match action {
        AgentActionRequest::SetRoutingMode { mode } => plan_routing_mode(snapshot, *mode),
        AgentActionRequest::DisableStaleSystemProxy => plan_stale_proxy(snapshot),
    }
}

fn plan_routing_mode(
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
            field: "routing_mode".into(),
            before: current.as_core_value().into(),
            after: target.as_core_value().into(),
        }],
        preconditions: ActionPreconditions::SetRoutingMode {
            before: current,
            core_state_changed_at: snapshot.core.state_changed_at,
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
            field: "system_proxy".into(),
            before: "enabled".into(),
            after: "disabled".into(),
        }],
        preconditions: ActionPreconditions::DisableStaleSystemProxy {
            core_state_changed_at: snapshot.core.state_changed_at,
            expected_port: proxy.expected_mixed_port,
            desired_before: proxy.desired_enabled,
        },
    })
}

fn validate_preconditions(
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

async fn execute_action(
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
    preconditions: &ActionPreconditions,
) -> AgentResult<()> {
    match (action, preconditions) {
        (
            AgentActionRequest::SetRoutingMode { mode },
            ActionPreconditions::SetRoutingMode { before, .. },
        ) => set_routing_mode(*before, *mode).await,
        (
            AgentActionRequest::DisableStaleSystemProxy,
            ActionPreconditions::DisableStaleSystemProxy {
                expected_port,
                desired_before,
                ..
            },
        ) => disable_stale_system_proxy(snapshot, *expected_port, *desired_before).await,
        _ => Err(AgentCommandError::NetworkStateChanged),
    }
}

async fn set_routing_mode(before: AgentRoutingMode, target: AgentRoutingMode) -> AgentResult<()> {
    if patch_running_mode(target).await.is_err() {
        return if rollback_routing_mode(before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    if persist_routing_mode(target).await.is_err() {
        return if rollback_routing_mode(before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    if routing_mode_is_applied(target).await {
        return Ok(());
    }
    if rollback_routing_mode(before).await {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

async fn patch_running_mode(mode: AgentRoutingMode) -> AgentResult<()> {
    let mapping = routing_mode_mapping(mode);
    tokio::time::timeout(
        CORE_ACTION_TIMEOUT,
        crate::core::clash::api::patch_configs(&mapping),
    )
    .await
    .map_err(|_| AgentCommandError::ActionFailed)?
    .map_err(|_| AgentCommandError::ActionFailed)
}

async fn persist_routing_mode(mode: AgentRoutingMode) -> AgentResult<()> {
    tokio::time::timeout(
        CORE_ACTION_TIMEOUT,
        feat::patch_clash(routing_mode_mapping(mode)),
    )
    .await
    .map_err(|_| AgentCommandError::ActionFailed)?
    .map_err(|_| AgentCommandError::ActionFailed)
}

async fn rollback_routing_mode(mode: AgentRoutingMode) -> bool {
    let persisted = persist_routing_mode(mode).await.is_ok();
    let running = patch_running_mode(mode).await.is_ok();
    persisted && running && routing_mode_is_applied(mode).await
}

async fn routing_mode_is_applied(mode: AgentRoutingMode) -> bool {
    let configured = crate::config::core::Config::runtime()
        .latest()
        .config
        .as_ref()
        .and_then(|config| config.get("mode"))
        .and_then(serde_yaml::Value::as_str)
        .and_then(AgentRoutingMode::parse);
    configured == Some(mode) && core_probe::observed_routing_mode().await == Ok(mode)
}

fn routing_mode_mapping(mode: AgentRoutingMode) -> Mapping {
    let mut mapping = Mapping::new();
    mapping.insert(
        Value::String("mode".into()),
        Value::String(mode.as_core_value().into()),
    );
    mapping
}

async fn disable_stale_system_proxy(
    snapshot: &AgentNetworkSnapshot,
    expected_port: u16,
    desired_before: bool,
) -> AgentResult<()> {
    if snapshot.core.state != AgentCoreState::Stopped {
        return Err(AgentCommandError::NetworkStateChanged);
    }
    let original = read_system_proxy().await?;
    if !is_expected_enabled_proxy(&original, expected_port) {
        return Err(AgentCommandError::NetworkStateChanged);
    }
    if persist_system_proxy_desired(false).await.is_err() {
        return if rollback_system_proxy(original, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    let mut disabled = original.clone();
    disabled.enable = false;
    if write_system_proxy(disabled).await.is_err() {
        return if rollback_system_proxy(original, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    let observed = match read_system_proxy().await {
        Ok(observed) => observed,
        Err(_) => {
            return if rollback_system_proxy(original, desired_before).await {
                Err(AgentCommandError::VerificationFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
    };
    if !observed.enable {
        return Ok(());
    }
    if rollback_system_proxy(original, desired_before).await {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

async fn persist_system_proxy_desired(enabled: bool) -> AgentResult<()> {
    feat::patch_verge(IVerge {
        enable_system_proxy: Some(enabled),
        ..Default::default()
    })
    .await
    .map_err(|_| AgentCommandError::ActionFailed)
}

async fn rollback_system_proxy(original: Sysproxy, desired_before: bool) -> bool {
    let persisted = persist_system_proxy_desired(desired_before).await.is_ok();
    let restored = write_system_proxy(original).await.is_ok();
    persisted && restored
}

async fn read_system_proxy() -> AgentResult<Sysproxy> {
    tokio::task::spawn_blocking(Sysproxy::get_system_proxy)
        .await
        .map_err(|_| AgentCommandError::ActionFailed)?
        .map_err(|_| AgentCommandError::ActionFailed)
}

async fn write_system_proxy(proxy: Sysproxy) -> AgentResult<()> {
    tokio::task::spawn_blocking(move || proxy.set_system_proxy())
        .await
        .map_err(|_| AgentCommandError::ActionFailed)?
        .map_err(|_| AgentCommandError::ActionFailed)
}

fn is_expected_enabled_proxy(proxy: &Sysproxy, expected_port: u16) -> bool {
    proxy.enable
        && proxy.port == expected_port
        && host_scope(&proxy.host) == AgentHostScope::Loopback
}

fn verify_action(snapshot: &AgentNetworkSnapshot, action: &AgentActionRequest) -> bool {
    match action {
        AgentActionRequest::SetRoutingMode { mode } => {
            snapshot.core.routing_mode == Some(*mode)
                && snapshot.core.observed_routing_mode == Some(*mode)
                && snapshot.core.applied_consistency == AgentAppliedState::Consistent
        }
        AgentActionRequest::DisableStaleSystemProxy => {
            snapshot.system_proxy.observed_enabled == Some(false)
                && !snapshot.system_proxy.desired_enabled
        }
    }
}

async fn confirm_network_change(
    window: &WebviewWindow,
    proposal: &AgentProposal,
) -> AgentResult<bool> {
    let window = window.clone();
    let message = proposal
        .changes
        .first()
        .map(|change| format!("{}\n{} → {}", change.field, change.before, change.after))
        .unwrap_or_else(|| "Chimera network change".to_owned());
    tokio::task::spawn_blocking(move || {
        window
            .dialog()
            .message(message)
            .title("Chimera")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::YesNo)
            .parent(&window)
            .blocking_show()
    })
    .await
    .map_err(|_| AgentCommandError::ActionFailed)
}

fn proposal_digest(
    id: &str,
    action: &AgentActionRequest,
    revision: &str,
    expires_at: i64,
) -> String {
    let material = serde_json::to_vec(&(id, action, revision, expires_at))
        .expect("agent proposal digest material must serialize");
    hex::encode(sha2::Sha256::digest(material))
}

fn audit_proposal(proposal: &AgentProposal, outcome: &str) {
    tracing::info!(
        target: "agent_audit",
        proposal_id = %proposal.id,
        action = ?proposal.action.kind(),
        snapshot_revision = %proposal.snapshot_revision,
        outcome,
        "network action proposal"
    );
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        ActionPreconditions, PendingProposal, ProposalStore, cleanup_store, enforce_store_limits,
        plan_routing_mode, proposal_digest, verify_action,
    };
    use crate::features::agent::model::{
        AgentActionRequest, AgentAppliedState, AgentConnectorState, AgentCoreSnapshot,
        AgentCoreState, AgentHealth, AgentHostScope, AgentNetworkSnapshot, AgentPrivacyBoundary,
        AgentProfileSnapshot, AgentProposal, AgentRoutingMode, AgentRunType, AgentServiceSnapshot,
        AgentServiceState, AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentTunSnapshot,
    };

    #[test]
    fn proposal_digest_binds_action_revision_and_expiry() {
        let action = AgentActionRequest::SetRoutingMode {
            mode: AgentRoutingMode::Rule,
        };
        let digest = proposal_digest("id", &action, "revision", 123);
        assert_ne!(digest, proposal_digest("id", &action, "changed", 123));
        assert_ne!(digest, proposal_digest("id", &action, "revision", 124));
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
        assert!(verify_action(&snapshot, &action));
        snapshot.core.observed_routing_mode = Some(AgentRoutingMode::Direct);
        assert!(!verify_action(&snapshot, &action));
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
            os_family: "test".into(),
            health: AgentHealth::Healthy,
            core: AgentCoreSnapshot {
                state: AgentCoreState::Running,
                run_type: AgentRunType::Normal,
                selected_core: "test".into(),
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
                upload_total: Some("0".into()),
                download_total: Some("0".into()),
                recent_error_count: 0,
            },
            findings: Vec::new(),
            probe_failures: Vec::new(),
            privacy: AgentPrivacyBoundary {
                contains_raw_logs: false,
                contains_profile_names: false,
                contains_profile_urls: false,
                contains_connection_targets: false,
                contains_controller_secret: false,
            },
        }
    }
}
