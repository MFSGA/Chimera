use std::{
    collections::HashMap,
    future::Future,
    panic::AssertUnwindSafe,
    pin::Pin,
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::FutureExt;
use sha2::Digest;
use subtle::ConstantTimeEq;

use super::{
    client::AgentHistoryClient,
    history::AgentAuditOutcome,
    model::{
        AgentActionRequest, AgentActionResult, AgentCommandError, AgentConnectorState,
        AgentCoreState, AgentNetworkSnapshot, AgentProposal, AgentResult,
    },
    planning::{
        ActionPreconditions, capability_definition, plan_action, validate_preconditions,
        verify_action,
    },
    ports::{AgentConfirmationPort, AgentRuntimePort},
};

#[cfg(test)]
use super::planning::{
    plan_proxy_endpoint_repair, plan_reconnect_telemetry, plan_restart_core, plan_routing_mode,
    plan_service_control, plan_service_mode_change, plan_start_core, plan_system_proxy_change,
    plan_tun_change, recommendations, tun_impacts,
};

const PROPOSAL_TTL: Duration = Duration::from_secs(60);
const MIN_PROPOSAL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_PENDING_PROPOSALS: usize = 24;
const MAX_PENDING_PER_OWNER: usize = 4;
const PROPOSAL_ID_LENGTH: usize = 32;
const PROPOSAL_DIGEST_LENGTH: usize = 64;
const TELEMETRY_STABILIZE_TIMEOUT: Duration = Duration::from_secs(10);
const TELEMETRY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SERVICE_STABILIZE_TIMEOUT: Duration = Duration::from_secs(15);
const SERVICE_POLL_INTERVAL: Duration = Duration::from_millis(500);

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

pub(crate) type AgentProposalExecutionFuture =
    Pin<Box<dyn Future<Output = AgentResult<AgentActionResult>> + Send + 'static>>;

pub(crate) struct AgentProposalState {
    proposals: ProposalStore,
    history: AgentHistoryClient,
}

impl AgentProposalState {
    pub(crate) fn new(history: AgentHistoryClient) -> Self {
        Self {
            proposals: ProposalStore::default(),
            history,
        }
    }

    pub(crate) async fn propose(
        &mut self,
        runtime: &dyn AgentRuntimePort,
        owner_label: &str,
        action: AgentActionRequest,
    ) -> AgentResult<AgentProposal> {
        self.reserve_proposal_slot(owner_label)?;
        let snapshot = runtime.snapshot().await;
        let plan = plan_action(&snapshot, &action)?;
        let capability =
            capability_definition(action.kind()).ok_or(AgentCommandError::ActionNotAvailable)?;
        let created_at = chrono::Utc::now().timestamp_millis();
        let expires_at = created_at + PROPOSAL_TTL.as_millis() as i64;
        let id = hex::encode(rand::random::<[u8; 16]>());
        let digest = proposal_digest(&id, &action, &snapshot.revision, expires_at)?;
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
            requires_confirmation: capability.requires_confirmation,
        };
        self.insert_proposal(
            id,
            PendingProposal {
                proposal: proposal.clone(),
                preconditions: plan.preconditions,
                owner_label: owner_label.to_owned(),
                expires_at: Instant::now() + PROPOSAL_TTL,
            },
        )?;
        audit_proposal(&proposal, AgentAuditOutcome::Proposed);
        Ok(proposal)
    }

    pub(crate) fn history_client(&self) -> AgentHistoryClient {
        self.history.clone()
    }

    pub(crate) fn begin_execute(
        &mut self,
        runtime: Arc<dyn AgentRuntimePort>,
        confirmation: Arc<dyn AgentConfirmationPort>,
        execution_gate: Arc<tokio::sync::Semaphore>,
        owner_label: String,
        proposal_id: String,
        digest: String,
    ) -> AgentResult<AgentProposalExecutionFuture> {
        if !is_fixed_lower_hex(&proposal_id, PROPOSAL_ID_LENGTH) {
            return Err(AgentCommandError::ProposalNotFound);
        }
        if !is_fixed_lower_hex(&digest, PROPOSAL_DIGEST_LENGTH) {
            return Err(AgentCommandError::ProposalDigestMismatch);
        }

        let pending = self.take_proposal(&owner_label, &proposal_id, &digest)?;
        let history = self.history.clone();
        Ok(Box::pin(async move {
            let result = contain_execution_panic(async {
                match execution_gate.acquire_owned().await {
                    Ok(_permit) => {
                        execute_pending(
                            runtime.as_ref(),
                            confirmation.as_ref(),
                            &owner_label,
                            pending.clone(),
                            &digest,
                        )
                        .await
                    }
                    Err(_) => Err(AgentCommandError::ActionFailed),
                }
            })
            .await;
            let outcome = result
                .as_ref()
                .map(|_| AgentAuditOutcome::Verified)
                .unwrap_or_else(|error| error.audit_outcome());
            audit_proposal(&pending.proposal, outcome);
            history
                .record_audit(
                    &pending.proposal.id,
                    &pending.proposal.action,
                    &pending.proposal.snapshot_revision,
                    outcome,
                )
                .await;
            result
        }))
    }

    pub(crate) fn cancel(&mut self, owner_label: &str, proposal_id: &str) -> bool {
        if !is_fixed_lower_hex(proposal_id, PROPOSAL_ID_LENGTH) {
            return false;
        }

        let is_owner = self
            .proposals
            .pending
            .get(proposal_id)
            .is_some_and(|pending| pending.owner_label == owner_label);
        if is_owner {
            self.proposals.pending.remove(proposal_id);
        }
        is_owner
    }

    fn reserve_proposal_slot(&mut self, owner_label: &str) -> AgentResult<()> {
        let now = Instant::now();
        cleanup_store(&mut self.proposals, now);
        if self
            .proposals
            .last_proposed_at
            .get(owner_label)
            .is_some_and(|last| now.duration_since(*last) < MIN_PROPOSAL_INTERVAL)
        {
            return Err(AgentCommandError::ProposalRateLimited);
        }
        enforce_store_limits(&self.proposals, owner_label)?;
        self.proposals
            .last_proposed_at
            .insert(owner_label.to_owned(), now);
        Ok(())
    }

    fn insert_proposal(&mut self, id: String, pending: PendingProposal) -> AgentResult<()> {
        cleanup_store(&mut self.proposals, Instant::now());
        enforce_store_limits(&self.proposals, &pending.owner_label)?;
        self.proposals.pending.insert(id, pending);
        Ok(())
    }

    fn take_proposal(
        &mut self,
        owner_label: &str,
        proposal_id: &str,
        digest: &str,
    ) -> AgentResult<PendingProposal> {
        take_owned_proposal(&mut self.proposals, owner_label, proposal_id, digest)
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
            Self::BridgeStartFailed => AgentAuditOutcome::BridgeStartFailed,
            Self::HistoryClearFailed => AgentAuditOutcome::HistoryClearFailed,
        }
    }
}

async fn contain_execution_panic<T, F>(execution: F) -> AgentResult<T>
where
    F: Future<Output = AgentResult<T>>,
{
    AssertUnwindSafe(execution)
        .catch_unwind()
        .await
        .unwrap_or(Err(AgentCommandError::ActionFailed))
}

async fn execute_pending(
    runtime: &dyn AgentRuntimePort,
    confirmation: &dyn AgentConfirmationPort,
    owner_label: &str,
    pending: PendingProposal,
    digest: &str,
) -> AgentResult<AgentActionResult> {
    let proposal = &pending.proposal;
    if proposal.digest != digest {
        return Err(AgentCommandError::ProposalDigestMismatch);
    }
    let confirmation_budget = pending
        .expires_at
        .checked_duration_since(Instant::now())
        .ok_or(AgentCommandError::ProposalExpired)?;
    let confirmed = tokio::time::timeout(
        confirmation_budget,
        confirmation.confirm(owner_label, proposal),
    )
    .await
    .map_err(|_| AgentCommandError::ProposalExpired)??;
    if !confirmed {
        return Err(AgentCommandError::ConfirmationDeclined);
    }
    if pending.expires_at <= Instant::now() {
        return Err(AgentCommandError::ProposalExpired);
    }
    let current = runtime.snapshot().await;
    if pending.expires_at <= Instant::now() {
        return Err(AgentCommandError::ProposalExpired);
    }
    validate_preconditions(&current, &pending.preconditions)?;
    execute_action(runtime, &current, &proposal.action, &pending.preconditions).await?;
    let snapshot = runtime.snapshot().await;
    if !verify_action(&snapshot, &proposal.action, &pending.preconditions) {
        return Err(AgentCommandError::VerificationFailed);
    }
    Ok(AgentActionResult {
        proposal_id: proposal.id.clone(),
        action: proposal.action.kind(),
        verified: true,
        snapshot,
    })
}

fn is_fixed_lower_hex(value: &str, expected_length: usize) -> bool {
    value.len() == expected_length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn cleanup_store(store: &mut ProposalStore, now: Instant) {
    store.pending.retain(|_, pending| pending.expires_at > now);
    store
        .last_proposed_at
        .retain(|_, last| now.duration_since(*last) < PROPOSAL_TTL);
}

fn take_owned_proposal(
    store: &mut ProposalStore,
    owner_label: &str,
    proposal_id: &str,
    digest: &str,
) -> AgentResult<PendingProposal> {
    let Some(pending) = store.pending.get(proposal_id) else {
        return Err(AgentCommandError::ProposalNotFound);
    };
    if pending.owner_label != owner_label {
        return Err(AgentCommandError::ProposalNotFound);
    }
    if !bool::from(pending.proposal.digest.as_bytes().ct_eq(digest.as_bytes())) {
        return Err(AgentCommandError::ProposalDigestMismatch);
    }
    store
        .pending
        .remove(proposal_id)
        .ok_or(AgentCommandError::ProposalNotFound)
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

async fn execute_action(
    runtime: &dyn AgentRuntimePort,
    snapshot: &AgentNetworkSnapshot,
    action: &AgentActionRequest,
    preconditions: &ActionPreconditions,
) -> AgentResult<()> {
    let definition =
        capability_definition(action.kind()).ok_or(AgentCommandError::NetworkStateChanged)?;
    if definition.executor.action_kind() != action.kind() {
        return Err(AgentCommandError::NetworkStateChanged);
    }

    match (action, preconditions) {
        (
            AgentActionRequest::SetRoutingMode { mode },
            ActionPreconditions::SetRoutingMode { before, .. },
        ) => runtime.set_routing_mode(*before, *mode).await,
        (
            AgentActionRequest::SetTunEnabled { enabled },
            ActionPreconditions::SetTunEnabled { desired_before, .. },
        ) => runtime.set_tun_enabled(*desired_before, *enabled).await,
        (
            AgentActionRequest::SetSystemProxyEnabled { enabled },
            ActionPreconditions::SetSystemProxyEnabled { desired_before, .. },
        ) => {
            runtime
                .set_system_proxy_enabled(*desired_before, *enabled)
                .await
        }
        (
            AgentActionRequest::SetServiceMode { enabled },
            ActionPreconditions::SetServiceMode { desired_before, .. },
        ) => runtime.set_service_mode(*desired_before, *enabled).await,
        (AgentActionRequest::StartCore, ActionPreconditions::StartCore { .. }) => {
            runtime.ensure_core_running().await
        }
        (AgentActionRequest::RestartCore, ActionPreconditions::RestartCore { .. }) => {
            runtime.restart_core().await
        }
        (
            AgentActionRequest::ReconnectTelemetry,
            ActionPreconditions::ReconnectTelemetry { .. },
        ) => reconnect_telemetry(runtime).await,
        (
            AgentActionRequest::StartService
            | AgentActionRequest::StopService
            | AgentActionRequest::RestartService,
            ActionPreconditions::ControlService { .. },
        ) => control_service(runtime, action, preconditions).await,
        (
            AgentActionRequest::RepairSystemProxyEndpoint,
            ActionPreconditions::RepairSystemProxyEndpoint {
                expected_port,
                desired_before,
                ..
            },
        ) => {
            runtime
                .repair_system_proxy_endpoint(snapshot, *expected_port, *desired_before)
                .await
        }
        (
            AgentActionRequest::DisableStaleSystemProxy,
            ActionPreconditions::DisableStaleSystemProxy {
                expected_port,
                desired_before,
                ..
            },
        ) => {
            runtime
                .disable_stale_system_proxy(snapshot, *expected_port, *desired_before)
                .await
        }
        _ => Err(AgentCommandError::NetworkStateChanged),
    }
}

async fn reconnect_telemetry(runtime: &dyn AgentRuntimePort) -> AgentResult<()> {
    runtime.reconnect_telemetry().await?;
    let deadline = Instant::now() + TELEMETRY_STABILIZE_TIMEOUT;
    loop {
        let snapshot = runtime.snapshot().await;
        if snapshot.telemetry.state == AgentConnectorState::Connected {
            return Ok(());
        }
        if snapshot.core.state != AgentCoreState::Running || Instant::now() >= deadline {
            return Err(AgentCommandError::VerificationFailed);
        }
        tokio::time::sleep(TELEMETRY_POLL_INTERVAL).await;
    }
}

async fn control_service(
    runtime: &dyn AgentRuntimePort,
    action: &AgentActionRequest,
    preconditions: &ActionPreconditions,
) -> AgentResult<()> {
    runtime.control_service(action).await?;
    let deadline = Instant::now() + SERVICE_STABILIZE_TIMEOUT;
    loop {
        let snapshot = runtime.snapshot().await;
        if verify_action(&snapshot, action, preconditions) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(AgentCommandError::PartialApply);
        }
        tokio::time::sleep(SERVICE_POLL_INTERVAL).await;
    }
}

fn proposal_digest(
    id: &str,
    action: &AgentActionRequest,
    revision: &str,
    expires_at: i64,
) -> AgentResult<String> {
    let material = serde_json::to_vec(&(id, action, revision, expires_at))
        .map_err(|_| AgentCommandError::ActionFailed)?;
    Ok(hex::encode(sha2::Sha256::digest(material)))
}

fn audit_proposal(proposal: &AgentProposal, outcome: AgentAuditOutcome) {
    tracing::info!(
        target: "agent_audit",
        action = ?proposal.action.kind(),
        outcome = outcome.as_str(),
        "network action proposal"
    );
}

#[cfg(test)]
#[path = "actions_tests.rs"]
mod tests;
