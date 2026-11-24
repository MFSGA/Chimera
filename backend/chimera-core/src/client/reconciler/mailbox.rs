//! The single reconciler mailbox's request ownership, history, and scheduling.
//!
//! Like ref, one active round owns execution and only its terminal reply
//! settles the round. Chimera allows Checked admissions during a host await,
//! so the payload is generic and the active worker remains explicitly owned.
use super::{committed::SourceVersions, plan::RunIntent, workflow::RuntimeReconcilerState};
use parking_lot::Mutex;
use ractor::{ActorRef, Message, RpcReplyPort};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

pub type OperationId = u64;
pub const MAX_PENDING: usize = 32;
const COMPLETED_LIMIT: usize = 32;
pub const DIRTY_WINDOW: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Default, serde::Serialize, specta::Type)]
pub struct CoreLifecycleStatus {
    pub active: Option<OperationId>,
    pub queued: Vec<OperationId>,
    pub uncertain: bool,
    pub shutting_down: bool,
    pub completed: Vec<CoreLifecycleOperationResult>,
}

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct CoreLifecycleOperationResult {
    pub id: OperationId,
    pub error: Option<String>,
}

pub struct Response {
    pub id: OperationId,
    pub reply: Option<RpcReplyPort<anyhow::Result<()>>>,
}

pub struct Request<C> {
    pub command: C,
    pub response: Response,
    pub deferred_retry: Option<String>,
}

pub struct ActiveOperation {
    pub response: Response,
    pub task: tokio::task::JoinHandle<()>,
    pub shutdown: bool,
    pub trigger: &'static str,
    pub checked_clash: bool,
    /// Checked Apply already promotes its own exact verified selection.
    /// Legacy startup/reconcile must recover it from the confirmed receipt.
    pub checked_apply: bool,
    pub observed_sources: SourceVersions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    SourceOnly,
    StartupOwner,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionFailure {
    Closing,
    Uncertain,
    StartupUnproven,
    QueueFull,
    ApplicationRecovery,
    TooManyShutdownWaiters,
}

impl AdmissionFailure {
    pub fn message(self) -> &'static str {
        match self {
            Self::Closing => "core lifecycle is shutting down",
            Self::Uncertain => {
                "previous core lifecycle operation has an uncertain outcome; restart the application before further mutations"
            }
            Self::StartupUnproven => {
                "startup has not established the Core owner; retry startup reconciliation before runtime mutations"
            }
            Self::QueueFull => "core lifecycle queue is full",
            Self::ApplicationRecovery => {
                "an application runtime attempt is unresolved; run its explicit recovery before another command"
            }
            Self::TooManyShutdownWaiters => "too many shutdown waiters",
        }
    }
}

pub enum ShutdownAdmission {
    Replay(Option<String>),
    Wait { first: bool },
    Full,
}

pub struct Mailbox<C> {
    pub active: Option<ActiveOperation>,
    pub pending: VecDeque<Request<C>>,
    pub next_id: Arc<AtomicU64>,
    pub status: Arc<Mutex<CoreLifecycleStatus>>,
    pub uncertain: bool,
    pub startup_owner_unproven: bool,
    pub startup_owner_unproven_shared: Arc<AtomicBool>,
    pub shutting_down: bool,
    pub shutdown_result: Option<Option<String>>,
    pub shutdown_waiters: Vec<Response>,
}

impl<C> Mailbox<C> {
    pub fn new(
        next_id: Arc<AtomicU64>,
        status: Arc<Mutex<CoreLifecycleStatus>>,
        startup_owner_unproven_shared: Arc<AtomicBool>,
    ) -> Self {
        Self {
            active: None,
            pending: VecDeque::new(),
            next_id,
            status,
            uncertain: false,
            startup_owner_unproven: false,
            startup_owner_unproven_shared,
            shutting_down: false,
            shutdown_result: None,
            shutdown_waiters: Vec::new(),
        }
    }

    pub fn owner_unproven(&self) -> bool {
        self.startup_owner_unproven || self.startup_owner_unproven_shared.load(Ordering::Acquire)
    }

    pub fn set_owner_unproven(&mut self, unproven: bool) {
        self.startup_owner_unproven = unproven;
        self.startup_owner_unproven_shared
            .store(unproven, Ordering::Release);
    }

    pub fn queue_full(&self) -> bool {
        self.pending.len() >= MAX_PENDING
    }

    pub fn idle(&self) -> bool {
        self.active.is_none() && self.pending.is_empty()
    }

    pub fn reject_reason(&self, kind: RequestKind) -> Option<AdmissionFailure> {
        if self.shutting_down {
            return Some(AdmissionFailure::Closing);
        }
        if self.uncertain && kind != RequestKind::SourceOnly {
            return Some(AdmissionFailure::Uncertain);
        }
        if self.owner_unproven() && kind == RequestKind::Other {
            return Some(AdmissionFailure::StartupUnproven);
        }
        None
    }

    pub fn admit_request(&self, kind: RequestKind) -> Result<(), AdmissionFailure> {
        if let Some(reason) = self.reject_reason(kind) {
            return Err(reason);
        }
        if self.queue_full() {
            return Err(AdmissionFailure::QueueFull);
        }
        Ok(())
    }

    /// Recheck the same admission gate when an operation leaves the queue.
    /// Do not apply the capacity gate a second time to already admitted work.
    /// Source-only commits and explicit recovery retain their original order.
    pub fn drain_refused(
        &mut self,
        classify: impl Fn(&C) -> RequestKind,
        recovery_allowed: impl Fn(&C) -> bool,
    ) -> Vec<(Request<C>, AdmissionFailure)> {
        if self.shutting_down {
            return Vec::new();
        }
        let mut retained = VecDeque::new();
        let mut refused = Vec::new();
        while let Some(request) = self.pending.pop_front() {
            let reason = self.reject_reason(classify(&request.command)).or_else(|| {
                (!recovery_allowed(&request.command))
                    .then_some(AdmissionFailure::ApplicationRecovery)
            });
            match reason {
                Some(reason) => refused.push((request, reason)),
                None => retained.push_back(request),
            }
        }
        self.pending = retained;
        // The former three admission passes reported uncertainty before
        // startup-owner refusal, then unresolved application recovery.
        // Retain that order for completion history and callbacks.
        refused.sort_by_key(|(_, reason)| match reason {
            AdmissionFailure::Uncertain => 0,
            AdmissionFailure::StartupUnproven => 1,
            AdmissionFailure::ApplicationRecovery => 2,
            _ => 3,
        });
        refused
    }

    pub fn shutdown_admission(&self) -> ShutdownAdmission {
        if let Some(error) = &self.shutdown_result {
            ShutdownAdmission::Replay(error.clone())
        } else if self.shutdown_waiters.len() >= MAX_PENDING {
            ShutdownAdmission::Full
        } else {
            ShutdownAdmission::Wait {
                first: !self.shutting_down,
            }
        }
    }

    pub fn enqueue_shutdown_waiter(&mut self, response: Response) {
        self.shutdown_waiters.push(response);
    }

    pub fn close(&mut self) -> VecDeque<Request<C>> {
        self.shutting_down = true;
        std::mem::take(&mut self.pending)
    }

    pub fn take_next(&mut self, shutdown: impl FnOnce() -> C) -> Option<Request<C>> {
        if self.active.is_some() {
            return None;
        }
        if self.shutting_down {
            if self.shutdown_result.is_some() {
                return None;
            }
            self.shutdown_waiters.pop().map(|response| Request {
                command: shutdown(),
                response,
                deferred_retry: None,
            })
        } else {
            self.pending.pop_front()
        }
    }

    pub fn complete_shutdown(&mut self, response: Response, result: anyhow::Result<()>) {
        let error = result.as_ref().err().map(ToString::to_string);
        self.shutdown_result = Some(error.clone());
        self.settle(response, result);
        for waiter in std::mem::take(&mut self.shutdown_waiters) {
            self.settle(waiter, replay_shutdown(&error));
        }
    }

    pub fn mark_worker_lost(&mut self) {
        self.uncertain = true;
        self.startup_owner_unproven_shared
            .store(true, Ordering::Release);
        let mut status = self.status.lock();
        status.active = None;
        status.uncertain = true;
        status.shutting_down = true;
    }

    pub fn allocate_operation_id(&self) -> OperationId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn publish(&self) {
        let mut status = self.status.lock();
        status.active = self.active.as_ref().map(|operation| operation.response.id);
        status.queued = self
            .pending
            .iter()
            .map(|request| request.response.id)
            .chain(self.shutdown_waiters.iter().map(|response| response.id))
            .collect();
        status.uncertain = self.uncertain || self.owner_unproven();
        status.shutting_down = self.shutting_down;
    }

    pub fn settle(&self, response: Response, result: anyhow::Result<()>) {
        {
            let mut status = self.status.lock();
            if status.completed.len() == COMPLETED_LIMIT {
                status.completed.remove(0);
            }
            status.completed.push(CoreLifecycleOperationResult {
                id: response.id,
                error: result.as_ref().err().map(ToString::to_string),
            });
        }
        if let Some(reply) = response.reply {
            let _ = reply.send(result);
        } else if let Err(error) = result {
            tracing::warn!(%error, id = response.id, "background core lifecycle operation failed");
        }
    }
}

pub fn replay_shutdown(error: &Option<String>) -> anyhow::Result<()> {
    match error {
        Some(error) => Err(anyhow::anyhow!(error.clone())),
        None => Ok(()),
    }
}

/// Upstream Ractor timer primitives, owned by the mailbox. Retain only live
/// timer handles, and cancel every pending notification on shutdown/drop.
/// Never cancel the actual in-flight host worker here.
pub struct MailboxTimers<M: Message> {
    periodic: tokio::task::JoinHandle<()>,
    closed: bool,
    deadlines: Vec<tokio::task::JoinHandle<Result<(), ractor::MessagingErr<M>>>>,
}

impl<M: Message> MailboxTimers<M> {
    pub fn new(
        actor: &ActorRef<M>,
        interval: Duration,
        tick: impl Fn() -> M + Send + 'static,
    ) -> Self {
        Self {
            periodic: actor.send_interval(interval, tick),
            closed: false,
            deadlines: Vec::new(),
        }
    }

    pub fn after(
        &mut self,
        actor: &ActorRef<M>,
        delay: Duration,
        message: impl FnOnce() -> M + Send + 'static,
    ) {
        if self.closed {
            return;
        }
        self.deadlines.retain(|task| !task.is_finished());
        self.deadlines.push(actor.send_after(delay, message));
    }

    pub fn at(
        &mut self,
        actor: &ActorRef<M>,
        deadline: tokio::time::Instant,
        message: impl FnOnce() -> M + Send + 'static,
    ) {
        self.after(
            actor,
            deadline.saturating_duration_since(tokio::time::Instant::now()),
            message,
        );
    }

    pub fn cancel(&mut self) {
        self.closed = true;
        self.periodic.abort();
        for task in self.deadlines.drain(..) {
            task.abort();
        }
    }
}

impl<M: Message> Drop for MailboxTimers<M> {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirtyTick {
    None,
    Reconcile,
    SourceCommitted,
}

impl RuntimeReconcilerState {
    pub fn request_dirty_tick(&mut self, uncertain: bool, closing: bool) -> bool {
        if self.run_intent == RunIntent::Stopped || uncertain || closing {
            return false;
        }
        self.dirty = true;
        if self.dirty_tick_scheduled {
            return false;
        }
        self.dirty_tick_scheduled = true;
        true
    }

    pub fn dirty_tick(
        &mut self,
        uncertain: bool,
        closing: bool,
        owner_transition: bool,
        queue_full: bool,
    ) -> DirtyTick {
        self.dirty_tick_scheduled = false;
        if self.run_intent == RunIntent::Stopped || uncertain || closing {
            self.dirty = false;
        } else if owner_transition || queue_full {
            // The durable source intent remains latched until a later owner
            // or queue-capacity event can rearm it.
        } else if self.dirty {
            self.dirty = false;
            return if std::mem::take(&mut self.source_dirty) {
                DirtyTick::SourceCommitted
            } else {
                DirtyTick::Reconcile
            };
        }
        DirtyTick::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mailbox() -> Mailbox<RequestKind> {
        Mailbox::new(
            Arc::new(AtomicU64::new(1)),
            Arc::new(Mutex::new(CoreLifecycleStatus::default())),
            Arc::new(AtomicBool::new(false)),
        )
    }
    fn request(id: u64, command: RequestKind) -> Request<RequestKind> {
        Request {
            command,
            response: Response { id, reply: None },
            deferred_retry: None,
        }
    }

    #[test]
    fn capacity_and_owner_fences_are_rechecked_on_queued_work() {
        let mut m = mailbox();
        for id in 0..MAX_PENDING as u64 {
            assert_eq!(m.admit_request(RequestKind::Other), Ok(()));
            m.pending.push_back(request(id, RequestKind::Other));
        }
        assert_eq!(
            m.admit_request(RequestKind::SourceOnly),
            Err(AdmissionFailure::QueueFull)
        );
        m.startup_owner_unproven_shared
            .store(true, Ordering::Release);
        let refused = m.drain_refused(|kind| *kind, |_| true);
        assert_eq!(refused.len(), MAX_PENDING);
        assert!(
            refused
                .iter()
                .all(|(_, reason)| *reason == AdmissionFailure::StartupUnproven)
        );
        assert!(m.idle());
        assert_eq!(m.admit_request(RequestKind::StartupOwner), Ok(()));
        assert_eq!(m.admit_request(RequestKind::SourceOnly), Ok(()));
        m.uncertain = true;
        assert_eq!(
            m.admit_request(RequestKind::StartupOwner),
            Err(AdmissionFailure::Uncertain)
        );
        assert_eq!(m.admit_request(RequestKind::SourceOnly), Ok(()));
    }

    #[test]
    fn revalidation_preserves_admitted_fifo_and_refuses_recovery_incompatible_commands() {
        let mut m = mailbox();
        for (id, kind) in [
            (1, RequestKind::Other),
            (2, RequestKind::SourceOnly),
            (3, RequestKind::StartupOwner),
            (4, RequestKind::SourceOnly),
        ] {
            m.pending.push_back(request(id, kind));
        }
        m.set_owner_unproven(true);
        let refused = m.drain_refused(|kind| *kind, |kind| *kind != RequestKind::StartupOwner);
        assert_eq!(
            refused
                .iter()
                .map(|(r, reason)| (r.response.id, *reason))
                .collect::<Vec<_>>(),
            [
                (1, AdmissionFailure::StartupUnproven),
                (3, AdmissionFailure::ApplicationRecovery)
            ]
        );
        m.publish();
        assert_eq!(m.status.lock().queued, [2, 4]);
        assert_eq!(m.take_next(|| RequestKind::Other).unwrap().response.id, 2);
        assert_eq!(m.take_next(|| RequestKind::Other).unwrap().response.id, 4);
    }

    #[test]
    fn shutdown_drains_pending_and_replays_exact_failure_to_later_callers() {
        let mut m = mailbox();
        m.pending.push_back(request(1, RequestKind::Other));
        assert!(matches!(
            m.shutdown_admission(),
            ShutdownAdmission::Wait { first: true }
        ));
        m.enqueue_shutdown_waiter(Response { id: 2, reply: None });
        assert_eq!(m.close().len(), 1);
        assert_eq!(
            m.admit_request(RequestKind::SourceOnly),
            Err(AdmissionFailure::Closing)
        );
        assert!(matches!(
            m.shutdown_admission(),
            ShutdownAdmission::Wait { first: false }
        ));
        m.enqueue_shutdown_waiter(Response { id: 3, reply: None });
        let next = m.take_next(|| RequestKind::Other).unwrap();
        assert_eq!(next.response.id, 3);
        m.complete_shutdown(next.response, Err(anyhow::anyhow!("stop unconfirmed")));
        assert!(m.take_next(|| RequestKind::Other).is_none());
        assert!(
            matches!(m.shutdown_admission(), ShutdownAdmission::Replay(Some(reason)) if reason == "stop unconfirmed")
        );
        let status = m.status.lock();
        assert_eq!(
            status.completed.iter().map(|r| r.id).collect::<Vec<_>>(),
            [3, 2]
        );
        assert!(
            status
                .completed
                .iter()
                .all(|r| r.error.as_deref() == Some("stop unconfirmed"))
        );
    }

    #[test]
    fn dirty_tick_keeps_latest_source_intent_through_backpressure_and_handoff() {
        let mut runtime = RuntimeReconcilerState::default();
        runtime.source_dirty = true;
        assert!(runtime.request_dirty_tick(false, false));
        assert!(!runtime.request_dirty_tick(false, false));
        assert_eq!(
            runtime.dirty_tick(false, false, true, false),
            DirtyTick::None
        );
        assert!(runtime.dirty && runtime.source_dirty);
        assert!(runtime.request_dirty_tick(false, false));
        assert_eq!(
            runtime.dirty_tick(false, false, false, true),
            DirtyTick::None
        );
        assert!(runtime.dirty && runtime.source_dirty);
        assert!(runtime.request_dirty_tick(false, false));
        assert_eq!(
            runtime.dirty_tick(false, false, false, false),
            DirtyTick::SourceCommitted
        );
        assert!(!runtime.dirty && !runtime.source_dirty);
        assert_eq!(
            runtime.dirty_tick(false, false, false, false),
            DirtyTick::None
        );
    }

    #[test]
    fn history_is_bounded_and_lost_worker_never_publishes_success() {
        let mut m = mailbox();
        for id in 0..(COMPLETED_LIMIT as u64 + 3) {
            m.settle(Response { id, reply: None }, Ok(()));
        }
        assert_eq!(m.status.lock().completed.len(), COMPLETED_LIMIT);
        assert_eq!(m.status.lock().completed[0].id, 3);
        m.mark_worker_lost();
        let status = m.status.lock();
        assert!(status.uncertain && status.shutting_down);
        assert!(status.active.is_none());
        assert_eq!(status.completed.len(), COMPLETED_LIMIT);
    }
}
