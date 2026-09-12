use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use sha2::Digest;
use sysproxy::Sysproxy;
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tokio::sync::Mutex;

use crate::{
    client::ChimeraClient,
    config::{chimera::IVerge, runtime::ClashConfigOverrides},
    core::clash::transaction::TransactionOutcome,
    feat,
};

use super::{
    collect_network_snapshot, core_probe,
    diagnostics::host_scope,
    history,
    model::{
        AgentActionRequest, AgentActionResult, AgentActionRisk, AgentAppliedState,
        AgentCommandError, AgentCoreState, AgentHostScope, AgentImpact, AgentNetworkSnapshot,
        AgentProposal, AgentResult, AgentRoutingMode, AgentRunType, AgentServiceState,
        AgentStateChange,
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
    SetTunEnabled {
        desired_before: bool,
        generated_before: bool,
        core_state: AgentCoreState,
        core_state_changed_at: i64,
    },
    SetSystemProxyEnabled {
        desired_before: bool,
        observed_before: bool,
        observed_host_scope: AgentHostScope,
        observed_port: Option<u16>,
        core_state: AgentCoreState,
        core_state_changed_at: i64,
        expected_port: u16,
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
    pub(super) client: ChimeraClient,
    pub(super) proposals: Mutex<ProposalStore>,
    pub(super) execution: Mutex<()>,
}

struct ActionPlan {
    risk: AgentActionRisk,
    impacts: Vec<AgentImpact>,
    changes: Vec<AgentStateChange>,
    preconditions: ActionPreconditions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentAuditOutcome {
    Proposed,
    Verified,
    ActionNotAvailable,
    ProposalNotFound,
    ProposalExpired,
    DigestMismatch,
    StateChanged,
    RateLimited,
    LimitReached,
    ConfirmationDeclined,
    ActionFailed,
    PartialApply,
    VerificationFailed,
}

impl AgentAuditOutcome {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Verified => "verified",
            Self::ActionNotAvailable => "action_not_available",
            Self::ProposalNotFound => "proposal_not_found",
            Self::ProposalExpired => "proposal_expired",
            Self::DigestMismatch => "digest_mismatch",
            Self::StateChanged => "state_changed",
            Self::RateLimited => "rate_limited",
            Self::LimitReached => "limit_reached",
            Self::ConfirmationDeclined => "confirmation_declined",
            Self::ActionFailed => "action_failed",
            Self::PartialApply => "partial_apply",
            Self::VerificationFailed => "verification_failed",
        }
    }
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
        audit_proposal(&proposal, AgentAuditOutcome::Proposed);
        history::record_audit(&proposal, AgentAuditOutcome::Proposed.as_str()).await;
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
        let result = execute_pending(&self.client, app, window, pending.clone(), digest).await;
        let outcome = result
            .as_ref()
            .map(|_| AgentAuditOutcome::Verified)
            .unwrap_or_else(|error| error.audit_outcome());
        audit_proposal(&pending.proposal, outcome);
        history::record_audit(&pending.proposal, outcome.as_str()).await;
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
    fn audit_outcome(&self) -> AgentAuditOutcome {
        match self {
            Self::ActionNotAvailable => AgentAuditOutcome::ActionNotAvailable,
            Self::ProposalNotFound => AgentAuditOutcome::ProposalNotFound,
            Self::ProposalExpired => AgentAuditOutcome::ProposalExpired,
            Self::ProposalDigestMismatch => AgentAuditOutcome::DigestMismatch,
            Self::NetworkStateChanged => AgentAuditOutcome::StateChanged,
            Self::ProposalRateLimited => AgentAuditOutcome::RateLimited,
            Self::ProposalLimitReached => AgentAuditOutcome::LimitReached,
            Self::ConfirmationDeclined => AgentAuditOutcome::ConfirmationDeclined,
            Self::ActionFailed => AgentAuditOutcome::ActionFailed,
            Self::PartialApply => AgentAuditOutcome::PartialApply,
            Self::VerificationFailed => AgentAuditOutcome::VerificationFailed,
        }
    }
}

async fn execute_pending(
    client: &ChimeraClient,
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
    execute_action(client, &current, &proposal.action, &pending.preconditions).await?;
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
        AgentActionRequest::SetTunEnabled { enabled } => plan_tun_enabled(snapshot, *enabled),
        AgentActionRequest::SetSystemProxyEnabled { enabled } => {
            plan_system_proxy_enabled(snapshot, *enabled)
        }
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

fn plan_tun_enabled(snapshot: &AgentNetworkSnapshot, target: bool) -> AgentResult<ActionPlan> {
    if target && !tun_enable_preconditions_satisfied(snapshot) {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    let tun = &snapshot.tun;
    let generated_before = tun
        .generated_runtime_enabled
        .ok_or(AgentCommandError::ActionNotAvailable)?;
    if tun.desired_enabled == target && generated_before == target {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    let mut changes = Vec::with_capacity(2);
    if tun.desired_enabled != target {
        changes.push(AgentStateChange {
            field: "tun_desired".into(),
            before: enabled_label(tun.desired_enabled).into(),
            after: enabled_label(target).into(),
        });
    }
    if generated_before != target {
        changes.push(AgentStateChange {
            field: "tun_runtime".into(),
            before: enabled_label(generated_before).into(),
            after: enabled_label(target).into(),
        });
    }

    Ok(ActionPlan {
        risk: AgentActionRisk::HostNetworkChange,
        impacts: vec![
            AgentImpact::ExistingConnectionsMayChange,
            if target {
                AgentImpact::HostTunEnabled
            } else {
                AgentImpact::HostTunDisabled
            },
        ],
        changes,
        preconditions: ActionPreconditions::SetTunEnabled {
            desired_before: tun.desired_enabled,
            generated_before,
            core_state: snapshot.core.state,
            core_state_changed_at: snapshot.core.state_changed_at,
        },
    })
}

fn tun_enable_preconditions_satisfied(snapshot: &AgentNetworkSnapshot) -> bool {
    if snapshot.os_family != "windows" {
        return true;
    }
    snapshot.core.state == AgentCoreState::Running
        && snapshot.core.run_type == AgentRunType::Service
        && snapshot.service.desired_enabled
        && snapshot.service.state == AgentServiceState::Running
        && snapshot.service.ipc_connected
        && snapshot.service.runtime_compatible == Some(true)
}

fn plan_system_proxy_enabled(
    snapshot: &AgentNetworkSnapshot,
    target: bool,
) -> AgentResult<ActionPlan> {
    let proxy = &snapshot.system_proxy;
    let observed_before = proxy
        .observed_enabled
        .ok_or(AgentCommandError::ActionNotAvailable)?;
    if target && snapshot.core.state != AgentCoreState::Running {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    if observed_before && proxy.matches_expected_endpoint != Some(true) {
        return Err(AgentCommandError::ActionNotAvailable);
    }
    let already_applied = proxy.desired_enabled == target
        && observed_before == target
        && (!target || proxy.matches_expected_endpoint == Some(true));
    if already_applied {
        return Err(AgentCommandError::ActionNotAvailable);
    }

    let mut changes = Vec::with_capacity(2);
    if proxy.desired_enabled != target {
        changes.push(AgentStateChange {
            field: "system_proxy_desired".into(),
            before: enabled_label(proxy.desired_enabled).into(),
            after: enabled_label(target).into(),
        });
    }
    if observed_before != target {
        changes.push(AgentStateChange {
            field: "system_proxy_observed".into(),
            before: enabled_label(observed_before).into(),
            after: enabled_label(target).into(),
        });
    }

    Ok(ActionPlan {
        risk: AgentActionRisk::HostNetworkChange,
        impacts: vec![if target {
            AgentImpact::HostSystemProxyEnabled
        } else {
            AgentImpact::HostSystemProxyDisabled
        }],
        changes,
        preconditions: ActionPreconditions::SetSystemProxyEnabled {
            desired_before: proxy.desired_enabled,
            observed_before,
            observed_host_scope: proxy.observed_host_scope,
            observed_port: proxy.observed_port,
            core_state: snapshot.core.state,
            core_state_changed_at: snapshot.core.state_changed_at,
            expected_port: proxy.expected_mixed_port,
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
        ActionPreconditions::SetTunEnabled {
            desired_before,
            generated_before,
            core_state,
            core_state_changed_at,
        } => {
            current.core.state == *core_state
                && current.core.state_changed_at == *core_state_changed_at
                && current.tun.desired_enabled == *desired_before
                && current.tun.generated_runtime_enabled == Some(*generated_before)
        }
        ActionPreconditions::SetSystemProxyEnabled {
            desired_before,
            observed_before,
            observed_host_scope,
            observed_port,
            core_state,
            core_state_changed_at,
            expected_port,
        } => {
            current.core.state == *core_state
                && current.core.state_changed_at == *core_state_changed_at
                && current.system_proxy.desired_enabled == *desired_before
                && current.system_proxy.observed_enabled == Some(*observed_before)
                && current.system_proxy.observed_host_scope == *observed_host_scope
                && current.system_proxy.observed_port == *observed_port
                && current.system_proxy.expected_mixed_port == *expected_port
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

fn enabled_label(enabled: bool) -> &'static str {
    if enabled { "enabled" } else { "disabled" }
}

async fn execute_action(
    client: &ChimeraClient,
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
    preconditions: &ActionPreconditions,
) -> AgentResult<()> {
    match (action, preconditions) {
        (
            AgentActionRequest::SetRoutingMode { mode },
            ActionPreconditions::SetRoutingMode { before, .. },
        ) => set_routing_mode(client, *before, *mode).await,
        (
            AgentActionRequest::SetTunEnabled { enabled },
            ActionPreconditions::SetTunEnabled { desired_before, .. },
        ) => set_tun_enabled(client, *desired_before, *enabled).await,
        (
            AgentActionRequest::SetSystemProxyEnabled { enabled },
            ActionPreconditions::SetSystemProxyEnabled {
                desired_before,
                expected_port,
                ..
            },
        ) => set_system_proxy_enabled(client, *desired_before, *expected_port, *enabled).await,
        (
            AgentActionRequest::DisableStaleSystemProxy,
            ActionPreconditions::DisableStaleSystemProxy {
                expected_port,
                desired_before,
                ..
            },
        ) => disable_stale_system_proxy(client, snapshot, *expected_port, *desired_before).await,
        _ => Err(AgentCommandError::NetworkStateChanged),
    }
}

async fn set_routing_mode(
    client: &ChimeraClient,
    before: AgentRoutingMode,
    target: AgentRoutingMode,
) -> AgentResult<()> {
    let outcome = apply_routing_mode_transaction(client, target).await?;
    if let Some(error) = routing_transaction_error(&outcome) {
        return Err(error);
    }

    if routing_mode_is_applied_with_timeout(target).await {
        return Ok(());
    }

    let restored = matches!(
        apply_routing_mode_transaction(client, before).await,
        Ok(TransactionOutcome::Committed)
    ) && routing_mode_is_applied_with_timeout(before).await;

    if restored {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

fn routing_transaction_error(outcome: &TransactionOutcome) -> Option<AgentCommandError> {
    match outcome {
        TransactionOutcome::Committed => None,
        TransactionOutcome::Rejected { .. } | TransactionOutcome::RolledBack { .. } => {
            Some(AgentCommandError::ActionFailed)
        }
        TransactionOutcome::RollbackFailed { .. } => Some(AgentCommandError::PartialApply),
    }
}

async fn apply_routing_mode_transaction(
    client: &ChimeraClient,
    mode: AgentRoutingMode,
) -> AgentResult<TransactionOutcome> {
    let overrides = ClashConfigOverrides {
        mode: Some(mode.as_core_value().into()),
        ..ClashConfigOverrides::default()
    };

    Ok(feat::patch_running_clash_overrides(client, overrides).await)
}

async fn routing_mode_is_applied_with_timeout(mode: AgentRoutingMode) -> bool {
    tokio::time::timeout(CORE_ACTION_TIMEOUT, routing_mode_is_applied(mode))
        .await
        .unwrap_or(false)
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

async fn set_tun_enabled(
    client: &ChimeraClient,
    desired_before: bool,
    target: bool,
) -> AgentResult<()> {
    #[cfg(feature = "e2e")]
    if super::e2e::fixture_enabled() {
        super::e2e::set_tun_enabled(target);
        return Ok(());
    }

    if persist_tun_enabled(client, target).await.is_err() {
        return if restore_tun_enabled(client, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    if tun_enabled_is_applied(client, target).await {
        return Ok(());
    }
    if restore_tun_enabled(client, desired_before).await {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

async fn persist_tun_enabled(client: &ChimeraClient, enabled: bool) -> AgentResult<()> {
    client
        .patch_verge(IVerge {
            enable_tun_mode: Some(enabled),
            ..Default::default()
        })
        .await
        .map_err(|_| AgentCommandError::ActionFailed)
}

async fn restore_tun_enabled(client: &ChimeraClient, enabled: bool) -> bool {
    persist_tun_enabled(client, enabled).await.is_ok()
        && tun_enabled_is_applied(client, enabled).await
}

async fn tun_enabled_is_applied(client: &ChimeraClient, enabled: bool) -> bool {
    let desired = client
        .get_clash_config()
        .map(|config| config.enable_tun_mode)
        .ok();
    desired == Some(enabled)
        && generated_tun_enabled() == Some(enabled)
        && core_probe::observed_tun_enabled().await == Ok(enabled)
}

fn generated_tun_enabled() -> Option<bool> {
    crate::config::core::Config::runtime()
        .latest()
        .config
        .as_ref()
        .and_then(|config| config.get("tun"))
        .and_then(serde_yaml::Value::as_mapping)
        .and_then(|tun| tun.get("enable"))
        .and_then(serde_yaml::Value::as_bool)
}

async fn set_system_proxy_enabled(
    client: &ChimeraClient,
    desired_before: bool,
    expected_port: u16,
    target: bool,
) -> AgentResult<()> {
    #[cfg(feature = "e2e")]
    if super::e2e::fixture_enabled() {
        super::e2e::set_system_proxy_enabled(target);
        return Ok(());
    }

    let original = read_system_proxy().await?;
    if persist_system_proxy_desired(client, target).await.is_err() {
        return if rollback_system_proxy(client, original, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    if system_proxy_is_applied(client, target, expected_port).await {
        return Ok(());
    }
    if rollback_system_proxy(client, original, desired_before).await {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

async fn system_proxy_is_applied(
    client: &ChimeraClient,
    enabled: bool,
    expected_port: u16,
) -> bool {
    let desired = client
        .get_app_config()
        .map(|config| config.enable_system_proxy)
        .ok();
    let observed = read_system_proxy().await.ok();
    desired == Some(enabled)
        && observed.is_some_and(|proxy| {
            proxy.enable == enabled
                && (!enabled || is_expected_enabled_proxy(&proxy, expected_port))
        })
}

async fn disable_stale_system_proxy(
    client: &ChimeraClient,
    snapshot: &AgentNetworkSnapshot,
    expected_port: u16,
    desired_before: bool,
) -> AgentResult<()> {
    #[cfg(feature = "e2e")]
    if super::e2e::fixture_enabled() {
        super::e2e::mark_stale_proxy_repaired();
        return Ok(());
    }

    if snapshot.core.state != AgentCoreState::Stopped {
        return Err(AgentCommandError::NetworkStateChanged);
    }
    let original = read_system_proxy().await?;
    if !is_expected_enabled_proxy(&original, expected_port) {
        return Err(AgentCommandError::NetworkStateChanged);
    }
    if persist_system_proxy_desired(client, false).await.is_err() {
        return if rollback_system_proxy(client, original, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    let mut disabled = original.clone();
    disabled.enable = false;
    if write_system_proxy(disabled).await.is_err() {
        return if rollback_system_proxy(client, original, desired_before).await {
            Err(AgentCommandError::ActionFailed)
        } else {
            Err(AgentCommandError::PartialApply)
        };
    }
    let observed = match read_system_proxy().await {
        Ok(observed) => observed,
        Err(_) => {
            return if rollback_system_proxy(client, original, desired_before).await {
                Err(AgentCommandError::VerificationFailed)
            } else {
                Err(AgentCommandError::PartialApply)
            };
        }
    };
    if !observed.enable {
        return Ok(());
    }
    if rollback_system_proxy(client, original, desired_before).await {
        Err(AgentCommandError::VerificationFailed)
    } else {
        Err(AgentCommandError::PartialApply)
    }
}

async fn persist_system_proxy_desired(client: &ChimeraClient, enabled: bool) -> AgentResult<()> {
    client
        .patch_verge(IVerge {
            enable_system_proxy: Some(enabled),
            ..Default::default()
        })
        .await
        .map_err(|_| AgentCommandError::ActionFailed)
}

async fn rollback_system_proxy(
    client: &ChimeraClient,
    original: Sysproxy,
    desired_before: bool,
) -> bool {
    let persisted = persist_system_proxy_desired(client, desired_before)
        .await
        .is_ok();
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
        AgentActionRequest::SetTunEnabled { enabled } => {
            snapshot.tun.desired_enabled == *enabled
                && snapshot.tun.generated_runtime_enabled == Some(*enabled)
                && snapshot.tun.applied_consistency == AgentAppliedState::Consistent
        }
        AgentActionRequest::SetSystemProxyEnabled { enabled } => {
            snapshot.system_proxy.desired_enabled == *enabled
                && snapshot.system_proxy.observed_enabled == Some(*enabled)
                && (!*enabled || snapshot.system_proxy.matches_expected_endpoint == Some(true))
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
    #[cfg(feature = "e2e")]
    if super::e2e::fixture_enabled() {
        return Ok(true);
    }

    let window = window.clone();
    let message = proposal_confirmation_message(proposal);
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

fn proposal_confirmation_message(proposal: &AgentProposal) -> String {
    let mut lines = vec![
        "Confirm Chimera network change".to_owned(),
        format!("risk: {}", action_risk_label(proposal.risk)),
        "changes:".to_owned(),
    ];
    lines.extend(
        proposal
            .changes
            .iter()
            .map(|change| format!("- {}: {} -> {}", change.field, change.before, change.after)),
    );
    if !proposal.impacts.is_empty() {
        lines.push("impacts:".to_owned());
        lines.extend(
            proposal
                .impacts
                .iter()
                .map(|impact| format!("- {}", impact_label(*impact))),
        );
    }
    lines.join("\n")
}

fn action_risk_label(risk: AgentActionRisk) -> &'static str {
    match risk {
        AgentActionRisk::TrafficChange => "traffic_change",
        AgentActionRisk::HostNetworkChange => "host_network_change",
    }
}

fn impact_label(impact: AgentImpact) -> &'static str {
    match impact {
        AgentImpact::ExistingConnectionsMayChange => "existing_connections_may_change",
        AgentImpact::TrafficMayBypassProxy => "traffic_may_bypass_proxy",
        AgentImpact::AllTrafficUsesProxy => "all_traffic_uses_proxy",
        AgentImpact::RestoreRuleRouting => "restore_rule_routing",
        AgentImpact::HostSystemProxyEnabled => "host_system_proxy_enabled",
        AgentImpact::HostSystemProxyDisabled => "host_system_proxy_disabled",
        AgentImpact::HostTunEnabled => "host_tun_enabled",
        AgentImpact::HostTunDisabled => "host_tun_disabled",
    }
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

fn proposal_audit_reference(proposal_id: &str) -> String {
    hex::encode(&sha2::Sha256::digest(proposal_id.as_bytes())[..16])
}

fn audit_proposal(proposal: &AgentProposal, outcome: AgentAuditOutcome) {
    let proposal_reference = proposal_audit_reference(&proposal.id);
    tracing::info!(
        target: "agent_audit",
        proposal_id = %proposal_reference,
        action = ?proposal.action.kind(),
        snapshot_revision = %proposal.snapshot_revision,
        outcome = outcome.as_str(),
        "network action proposal"
    );
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{
        ActionPreconditions, AgentAuditOutcome, PendingProposal, ProposalStore, cleanup_store,
        enforce_store_limits, plan_routing_mode, plan_system_proxy_enabled, plan_tun_enabled,
        proposal_audit_reference, proposal_confirmation_message, proposal_digest,
        routing_transaction_error, verify_action,
    };
    use crate::core::clash::transaction::TransactionOutcome;
    use crate::features::agent::model::{
        AgentActionRequest, AgentActionRisk, AgentAppliedState, AgentConnectorState,
        AgentCoreSnapshot, AgentCoreState, AgentHealth, AgentHostScope, AgentImpact,
        AgentNetworkSnapshot, AgentPrivacyBoundary, AgentProfileSnapshot, AgentProposal,
        AgentRoutingMode, AgentRunType, AgentServiceSnapshot, AgentServiceState, AgentStateChange,
        AgentSystemProxySnapshot, AgentTelemetrySnapshot, AgentTunSnapshot,
    };

    #[test]
    fn audit_outcomes_preserve_stable_log_codes() {
        for (outcome, expected) in [
            (AgentAuditOutcome::Proposed, "proposed"),
            (AgentAuditOutcome::Verified, "verified"),
            (
                AgentAuditOutcome::ActionNotAvailable,
                "action_not_available",
            ),
            (AgentAuditOutcome::ProposalNotFound, "proposal_not_found"),
            (AgentAuditOutcome::ProposalExpired, "proposal_expired"),
            (AgentAuditOutcome::DigestMismatch, "digest_mismatch"),
            (AgentAuditOutcome::StateChanged, "state_changed"),
            (AgentAuditOutcome::RateLimited, "rate_limited"),
            (AgentAuditOutcome::LimitReached, "limit_reached"),
            (
                AgentAuditOutcome::ConfirmationDeclined,
                "confirmation_declined",
            ),
            (AgentAuditOutcome::ActionFailed, "action_failed"),
            (AgentAuditOutcome::PartialApply, "partial_apply"),
            (AgentAuditOutcome::VerificationFailed, "verification_failed"),
        ] {
            assert_eq!(outcome.as_str(), expected);
        }
    }

    #[test]
    fn audit_reference_is_fixed_lower_hex_and_hides_raw_proposal_id() {
        let proposal_id = "proposal-token-canary";
        let reference = proposal_audit_reference(proposal_id);

        assert_eq!(reference.len(), 32);
        assert!(
            reference
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        );
        assert!(!reference.contains(proposal_id));
        assert_eq!(reference, proposal_audit_reference(proposal_id));
    }

    #[test]
    fn confirmation_discloses_risk_all_changes_and_impacts() {
        let proposal = AgentProposal {
            id: "proposal".into(),
            digest: "digest".into(),
            action: AgentActionRequest::SetRoutingMode {
                mode: AgentRoutingMode::Global,
            },
            risk: AgentActionRisk::TrafficChange,
            impacts: vec![
                AgentImpact::ExistingConnectionsMayChange,
                AgentImpact::TrafficMayBypassProxy,
            ],
            changes: vec![
                AgentStateChange {
                    field: "routing_mode".into(),
                    before: "rule".into(),
                    after: "global".into(),
                },
                AgentStateChange {
                    field: "telemetry_connector".into(),
                    before: "connected".into(),
                    after: "disconnected".into(),
                },
            ],
            snapshot_revision: "revision".into(),
            created_at: 0,
            expires_at: 1,
            requires_confirmation: true,
        };

        let message = proposal_confirmation_message(&proposal);
        for expected in [
            "risk: traffic_change",
            "- routing_mode: rule -> global",
            "- telemetry_connector: connected -> disconnected",
            "- existing_connections_may_change",
            "- traffic_may_bypass_proxy",
        ] {
            assert!(message.contains(expected), "missing {expected}: {message}");
        }
    }

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
    fn tun_plan_requires_known_generated_state_and_discloses_host_change() {
        let mut snapshot = snapshot();
        snapshot.tun.generated_runtime_enabled = None;
        assert!(plan_tun_enabled(&snapshot, true).is_err());

        snapshot.tun.generated_runtime_enabled = Some(false);
        let plan = plan_tun_enabled(&snapshot, true).expect("TUN enable should be available");
        assert_eq!(plan.risk, AgentActionRisk::HostNetworkChange);
        assert_eq!(
            plan.impacts,
            vec![
                AgentImpact::ExistingConnectionsMayChange,
                AgentImpact::HostTunEnabled,
            ]
        );
        assert_eq!(plan.changes.len(), 2);
        assert_eq!(plan.changes[0].field, "tun_desired");
        assert_eq!(plan.changes[0].before, "disabled");
        assert_eq!(plan.changes[0].after, "enabled");
        assert_eq!(plan.changes[1].field, "tun_runtime");
    }

    #[test]
    fn windows_tun_enable_requires_ready_service_host_but_disable_does_not() {
        let mut snapshot = snapshot();
        snapshot.os_family = "windows".into();
        assert!(plan_tun_enabled(&snapshot, true).is_err());

        snapshot.core.run_type = AgentRunType::Service;
        snapshot.service.desired_enabled = true;
        snapshot.service.state = AgentServiceState::Running;
        snapshot.service.ipc_connected = true;
        snapshot.service.runtime_compatible = Some(true);
        assert!(plan_tun_enabled(&snapshot, true).is_ok());

        snapshot.tun.desired_enabled = true;
        snapshot.tun.generated_runtime_enabled = Some(true);
        snapshot.service.state = AgentServiceState::Stopped;
        assert!(plan_tun_enabled(&snapshot, false).is_ok());
    }

    #[test]
    fn system_proxy_enable_requires_running_core_but_disable_does_not() {
        let mut snapshot = snapshot();
        snapshot.core.state = AgentCoreState::Stopped;
        assert!(plan_system_proxy_enabled(&snapshot, true).is_err());

        snapshot.system_proxy.desired_enabled = true;
        snapshot.system_proxy.observed_enabled = Some(true);
        snapshot.system_proxy.matches_expected_endpoint = Some(true);
        let plan = plan_system_proxy_enabled(&snapshot, false)
            .expect("system proxy disable should be available while core is stopped");
        assert_eq!(plan.risk, AgentActionRisk::HostNetworkChange);
        assert_eq!(plan.impacts, vec![AgentImpact::HostSystemProxyDisabled]);

        snapshot.core.state = AgentCoreState::Running;
        snapshot.system_proxy.desired_enabled = false;
        snapshot.system_proxy.matches_expected_endpoint = Some(false);
        assert!(plan_system_proxy_enabled(&snapshot, true).is_err());
        assert!(plan_system_proxy_enabled(&snapshot, false).is_err());
    }

    #[test]
    fn new_action_verification_requires_desired_and_observed_state() {
        let mut snapshot = snapshot();
        let tun_action = AgentActionRequest::SetTunEnabled { enabled: true };
        assert!(!verify_action(&snapshot, &tun_action));
        snapshot.tun.desired_enabled = true;
        snapshot.tun.generated_runtime_enabled = Some(true);
        assert!(!verify_action(&snapshot, &tun_action));
        snapshot.tun.observed_active = AgentAppliedState::Consistent;
        snapshot.tun.applied_consistency = AgentAppliedState::Consistent;
        assert!(verify_action(&snapshot, &tun_action));

        let proxy_action = AgentActionRequest::SetSystemProxyEnabled { enabled: true };
        snapshot.system_proxy.desired_enabled = true;
        snapshot.system_proxy.observed_enabled = Some(true);
        snapshot.system_proxy.matches_expected_endpoint = Some(false);
        assert!(!verify_action(&snapshot, &proxy_action));
        snapshot.system_proxy.matches_expected_endpoint = Some(true);
        assert!(verify_action(&snapshot, &proxy_action));
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
    fn routing_transaction_outcomes_preserve_agent_error_semantics() {
        assert!(routing_transaction_error(&TransactionOutcome::Committed).is_none());
        assert_eq!(
            routing_transaction_error(&TransactionOutcome::Rejected {
                primary_error: "read failed".into(),
            })
            .expect("rejected transaction should fail")
            .to_string(),
            "agent_action_failed"
        );
        assert_eq!(
            routing_transaction_error(&TransactionOutcome::RolledBack {
                primary_error: "persist failed".into(),
            })
            .expect("compensated transaction should fail")
            .to_string(),
            "agent_action_failed"
        );
        assert_eq!(
            routing_transaction_error(&TransactionOutcome::RollbackFailed {
                primary_error: "persist failed".into(),
                rollback_error: "restore failed".into(),
            })
            .expect("failed compensation should be partial")
            .to_string(),
            "agent_action_partially_applied"
        );
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
