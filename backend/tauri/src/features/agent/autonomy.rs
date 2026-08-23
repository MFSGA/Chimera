use std::{collections::BTreeSet, sync::Arc};

use parking_lot::Mutex;

use super::model::{
    AgentActionKind, AgentAutonomyPolicyRequest, AgentAutonomyPolicyResult,
    AgentAutonomyPolicySnapshot, AgentAutonomyPolicyStatus, AgentAutonomyScope,
};

pub(crate) const AUTONOMY_POLICY_SCHEMA_VERSION: u16 = 1;
const MAX_AUTONOMY_DURATION_SECONDS: u32 = 30 * 60;
const MAX_AUTONOMY_ACTIONS: u16 = 32;

#[derive(Debug)]
struct ActivePolicy {
    snapshot: AgentAutonomyPolicySnapshot,
    session_nonce: [u8; 32],
}

#[derive(Debug)]
struct State {
    generation: u64,
    session_nonce: [u8; 32],
    terminal_status: AgentAutonomyPolicyStatus,
    active: Option<ActivePolicy>,
    in_flight: BTreeSet<AgentActionKind>,
}

#[derive(Clone, Debug)]
pub(crate) struct AgentAutonomyPolicyStore {
    state: Arc<Mutex<State>>,
}

impl AgentAutonomyPolicyStore {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                generation: 0,
                session_nonce: rand::random(),
                terminal_status: AgentAutonomyPolicyStatus::Disabled,
                active: None,
                in_flight: BTreeSet::new(),
            })),
        }
    }

    pub(crate) fn authorize(
        &self,
        request: AgentAutonomyPolicyRequest,
        now: i64,
    ) -> AgentAutonomyPolicyResult {
        let allowlist = request.allowlist.into_iter().collect::<BTreeSet<_>>();
        let rejected = if request.schema_version != AUTONOMY_POLICY_SCHEMA_VERSION {
            Some(AgentAutonomyPolicyStatus::SchemaVersionMismatch)
        } else if request.scope != AgentAutonomyScope::CurrentDesktopSession {
            Some(AgentAutonomyPolicyStatus::ScopeMismatch)
        } else if allowlist.is_empty() {
            Some(AgentAutonomyPolicyStatus::EmptyAllowlist)
        } else if allowlist
            .iter()
            .any(|action| *action != AgentActionKind::ReconnectTelemetry)
        {
            Some(AgentAutonomyPolicyStatus::ActionNotAllowed)
        } else if request.duration_seconds == 0
            || request.duration_seconds > MAX_AUTONOMY_DURATION_SECONDS
        {
            Some(AgentAutonomyPolicyStatus::DurationOutOfRange)
        } else if request.max_actions == 0 || request.max_actions > MAX_AUTONOMY_ACTIONS {
            Some(AgentAutonomyPolicyStatus::ActionBudgetOutOfRange)
        } else {
            None
        };
        if let Some(reason) = rejected {
            return AgentAutonomyPolicyResult::Rejected { reason };
        }

        let mut state = self.state.lock();
        state.generation = state.generation.saturating_add(1);
        let snapshot = AgentAutonomyPolicySnapshot {
            schema_version: AUTONOMY_POLICY_SCHEMA_VERSION,
            enabled: true,
            scope: AgentAutonomyScope::CurrentDesktopSession,
            allowlist: allowlist.into_iter().collect(),
            issued_at: now,
            expires_at: now.saturating_add(i64::from(request.duration_seconds)),
            max_actions: request.max_actions,
            remaining_actions: request.max_actions,
            generation: state.generation,
            status: AgentAutonomyPolicyStatus::Active,
        };
        state.terminal_status = AgentAutonomyPolicyStatus::Active;
        state.active = Some(ActivePolicy {
            snapshot: snapshot.clone(),
            session_nonce: state.session_nonce,
        });
        audit_policy("authorized", snapshot.generation, snapshot.status);
        AgentAutonomyPolicyResult::Authorized { policy: snapshot }
    }

    pub(crate) fn snapshot(&self, now: i64) -> AgentAutonomyPolicySnapshot {
        let mut state = self.state.lock();
        normalize_invalid(&mut state, now);
        state.active.as_ref().map_or_else(
            || inactive_snapshot(state.generation, state.terminal_status),
            |policy| policy.snapshot.clone(),
        )
    }

    pub(crate) fn revoke(&self, now: i64) -> AgentAutonomyPolicySnapshot {
        let mut state = self.state.lock();
        normalize_invalid(&mut state, now);
        state.generation = state.generation.saturating_add(1);
        state.terminal_status = AgentAutonomyPolicyStatus::Revoked;
        state.active = None;
        state.in_flight.clear();
        let snapshot = inactive_snapshot(state.generation, AgentAutonomyPolicyStatus::Revoked);
        audit_policy("revoked", snapshot.generation, snapshot.status);
        snapshot
    }

    #[allow(dead_code)]
    pub(crate) fn try_acquire(
        &self,
        action: AgentActionKind,
        now: i64,
    ) -> Result<AgentAutonomyLease, AgentAutonomyPolicyStatus> {
        if action != AgentActionKind::ReconnectTelemetry {
            return Err(AgentAutonomyPolicyStatus::ActionNotAllowed);
        }
        let mut state = self.state.lock();
        normalize_invalid(&mut state, now);
        let terminal_status = state.terminal_status;
        let active = state.active.as_ref().ok_or(terminal_status)?;
        if !active.snapshot.allowlist.contains(&action) {
            return Err(AgentAutonomyPolicyStatus::ActionNotAllowed);
        }
        if active.snapshot.remaining_actions == 0 {
            return Err(AgentAutonomyPolicyStatus::ActionBudgetExhausted);
        }
        if !state.in_flight.insert(action) {
            return Err(AgentAutonomyPolicyStatus::ActionInFlight);
        }
        let active = state.active.as_mut().ok_or(terminal_status)?;
        active.snapshot.remaining_actions = active.snapshot.remaining_actions.saturating_sub(1);
        Ok(AgentAutonomyLease {
            store: self.clone(),
            action,
            generation: active.snapshot.generation,
            released: false,
        })
    }

    fn release(&self, action: AgentActionKind, generation: u64) {
        self.state.lock().in_flight.remove(&action);
        audit_policy(
            "lease_released",
            generation,
            AgentAutonomyPolicyStatus::Active,
        );
    }

    #[cfg(test)]
    fn replace_session_nonce(&self, nonce: [u8; 32]) {
        self.state.lock().session_nonce = nonce;
    }
}

#[allow(dead_code)]
pub(crate) struct AgentAutonomyLease {
    store: AgentAutonomyPolicyStore,
    action: AgentActionKind,
    generation: u64,
    released: bool,
}

impl AgentAutonomyLease {
    #[cfg(test)]
    fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    fn release(mut self) {
        self.store.release(self.action, self.generation);
        self.released = true;
    }
}

impl Drop for AgentAutonomyLease {
    fn drop(&mut self) {
        if !self.released {
            self.store.release(self.action, self.generation);
            self.released = true;
        }
    }
}

fn normalize_invalid(state: &mut State, now: i64) {
    let status = state.active.as_ref().and_then(|policy| {
        if policy.session_nonce != state.session_nonce {
            Some(AgentAutonomyPolicyStatus::SessionMismatch)
        } else if policy.snapshot.expires_at <= now {
            Some(AgentAutonomyPolicyStatus::Expired)
        } else {
            None
        }
    });
    if let Some(status) = status {
        state.active = None;
        state.in_flight.clear();
        state.generation = state.generation.saturating_add(1);
        state.terminal_status = status;
        audit_policy("invalidated", state.generation, status);
    }
}

fn inactive_snapshot(
    generation: u64,
    status: AgentAutonomyPolicyStatus,
) -> AgentAutonomyPolicySnapshot {
    AgentAutonomyPolicySnapshot {
        schema_version: AUTONOMY_POLICY_SCHEMA_VERSION,
        enabled: false,
        scope: AgentAutonomyScope::CurrentDesktopSession,
        allowlist: Vec::new(),
        issued_at: 0,
        expires_at: 0,
        max_actions: 0,
        remaining_actions: 0,
        generation,
        status,
    }
}

fn audit_policy(event: &'static str, generation: u64, status: AgentAutonomyPolicyStatus) {
    tracing::info!(
        target: "agent_audit",
        event,
        generation,
        status = ?status,
        "agent autonomy policy changed"
    );
}

#[cfg(test)]
mod tests {
    use super::{AUTONOMY_POLICY_SCHEMA_VERSION, AgentAutonomyPolicyStore};
    use crate::features::agent::model::{
        AgentActionKind, AgentAutonomyPolicyRequest, AgentAutonomyPolicyResult,
        AgentAutonomyPolicyStatus, AgentAutonomyScope,
    };

    fn request() -> AgentAutonomyPolicyRequest {
        AgentAutonomyPolicyRequest {
            schema_version: AUTONOMY_POLICY_SCHEMA_VERSION,
            scope: AgentAutonomyScope::CurrentDesktopSession,
            allowlist: vec![AgentActionKind::ReconnectTelemetry],
            duration_seconds: 60,
            max_actions: 2,
        }
    }

    #[test]
    fn policy_is_memory_only_expiring_and_revocable() {
        let store = AgentAutonomyPolicyStore::new();
        let AgentAutonomyPolicyResult::Authorized { policy } = store.authorize(request(), 100)
        else {
            panic!("expected authorization");
        };
        assert_eq!(policy.expires_at, 160);
        assert_eq!(
            store.snapshot(161).status,
            AgentAutonomyPolicyStatus::Expired
        );
        let _ = store.authorize(request(), 200);
        assert_eq!(store.revoke(201).status, AgentAutonomyPolicyStatus::Revoked);
        assert_eq!(
            store.snapshot(202).status,
            AgentAutonomyPolicyStatus::Revoked
        );
    }

    #[test]
    fn invalid_requests_fail_closed() {
        let store = AgentAutonomyPolicyStore::new();
        for (request, reason) in [
            (
                AgentAutonomyPolicyRequest {
                    schema_version: 2,
                    ..request()
                },
                AgentAutonomyPolicyStatus::SchemaVersionMismatch,
            ),
            (
                AgentAutonomyPolicyRequest {
                    allowlist: vec![],
                    ..request()
                },
                AgentAutonomyPolicyStatus::EmptyAllowlist,
            ),
            (
                AgentAutonomyPolicyRequest {
                    allowlist: vec![AgentActionKind::StartCore],
                    ..request()
                },
                AgentAutonomyPolicyStatus::ActionNotAllowed,
            ),
            (
                AgentAutonomyPolicyRequest {
                    duration_seconds: 1_801,
                    ..request()
                },
                AgentAutonomyPolicyStatus::DurationOutOfRange,
            ),
            (
                AgentAutonomyPolicyRequest {
                    max_actions: 33,
                    ..request()
                },
                AgentAutonomyPolicyStatus::ActionBudgetOutOfRange,
            ),
        ] {
            assert_eq!(
                store.authorize(request, 100),
                AgentAutonomyPolicyResult::Rejected { reason }
            );
        }
    }

    #[test]
    fn request_rejects_unknown_fields() {
        let error = serde_json::from_str::<AgentAutonomyPolicyRequest>(
            r#"{"schema_version":1,"scope":"current_desktop_session","allowlist":["reconnect_telemetry"],"duration_seconds":60,"max_actions":1,"token":"canary"}"#,
        )
        .expect_err("unknown fields must fail closed");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn budget_and_single_flight_fail_closed() {
        let store = AgentAutonomyPolicyStore::new();
        let _ = store.authorize(request(), 100);
        let first = store
            .try_acquire(AgentActionKind::ReconnectTelemetry, 101)
            .expect("first lease");
        assert_eq!(first.generation(), 1);
        assert!(matches!(
            store.try_acquire(AgentActionKind::ReconnectTelemetry, 101),
            Err(AgentAutonomyPolicyStatus::ActionInFlight)
        ));
        first.release();
        let second = store
            .try_acquire(AgentActionKind::ReconnectTelemetry, 102)
            .expect("second lease");
        drop(second);
        assert!(matches!(
            store.try_acquire(AgentActionKind::ReconnectTelemetry, 103),
            Err(AgentAutonomyPolicyStatus::ActionBudgetExhausted)
        ));
    }

    #[test]
    fn session_change_invalidates_the_policy() {
        let store = AgentAutonomyPolicyStore::new();
        let _ = store.authorize(request(), 100);
        store.replace_session_nonce([7; 32]);
        assert_eq!(
            store.snapshot(101).status,
            AgentAutonomyPolicyStatus::SessionMismatch
        );
        assert!(matches!(
            store.try_acquire(AgentActionKind::ReconnectTelemetry, 101),
            Err(AgentAutonomyPolicyStatus::SessionMismatch)
        ));
    }
}
