//! Thin Chimera compatibility gate for Tauri's second-instance window callback.
//!
//! Unlike the pinned ref's pre-Tauri singleton lock, the existing Tauri plugin
//! forwards a second launch to the running process. The callback can arrive
//! while that process is still running its application setup. A window should
//! not be opened until the Client, Legacy state and URI listener exist.

use std::sync::Mutex;

#[derive(Default)]
pub(super) struct PendingWindowActivation {
    state: Mutex<ActivationState>,
}

#[derive(Default)]
struct ActivationState {
    ready: bool,
    pending: bool,
}

impl PendingWindowActivation {
    /// Return true when the application is ready to open/focus its window.
    /// Before readiness, coalesce activation requests without opening a window.
    pub(super) fn request(&self) -> bool {
        let mut state = self.state.lock().expect("activation state poisoned");
        if state.ready {
            true
        } else {
            state.pending = true;
            false
        }
    }

    /// Publish readiness and return whether setup must honor a queued wake.
    /// A mutex keeps this transition ordered with any concurrent request.
    pub(super) fn mark_ready(&self) -> bool {
        let mut state = self.state.lock().expect("activation state poisoned");
        state.ready = true;
        std::mem::take(&mut state.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn early_requests_are_coalesced_then_delivered_once() {
        let gate = PendingWindowActivation::default();
        assert!(!gate.request());
        assert!(!gate.request());
        assert!(gate.mark_ready());
        assert!(!gate.mark_ready());
        assert!(gate.request(), "later activations should not be lost");
    }

    #[test]
    fn racing_request_and_ready_do_not_lose_or_double_deliver_the_wake() {
        // The two possible orders must both produce exactly one activation.
        for _ in 0..64 {
            let gate = Arc::new(PendingWindowActivation::default());
            let barrier = Arc::new(Barrier::new(3));

            let requested_gate = gate.clone();
            let request_start = barrier.clone();
            let requester = std::thread::spawn(move || {
                request_start.wait();
                requested_gate.request()
            });

            let ready_gate = gate.clone();
            let ready_start = barrier.clone();
            let ready = std::thread::spawn(move || {
                ready_start.wait();
                ready_gate.mark_ready()
            });

            barrier.wait();
            let immediate = requester.join().unwrap();
            let deferred = ready.join().unwrap();
            assert_ne!(immediate, deferred, "one path must deliver the wake");
            assert!(gate.request(), "gate must stay ready after the race");
            assert!(!gate.mark_ready(), "the queued wake was already consumed");
        }
    }
}
