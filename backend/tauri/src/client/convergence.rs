//! Shared vocabulary and bounded retry policy; no infrastructure or mutable globals.
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConvergenceHealth {
    Healthy,
    Pending,
    RetryScheduled,
    WaitingDependency,
    Blocked,
    RecoveryRequired,
}

pub(crate) const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(5),
    Duration::from_secs(30),
];

/// Backoff between startup attempts that are waiting for an absent or
/// unreadable dependency. The final delay repeats, matching ref.
pub(crate) const REESTABLISH_WAIT_DELAYS: [Duration; 5] = [
    Duration::from_secs(5),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutcomeClass {
    Application,
    Dependency,
}

pub(crate) fn next_wait(waits: u8, class: OutcomeClass) -> (Duration, u8) {
    match class {
        OutcomeClass::Application => (Duration::ZERO, 0),
        OutcomeClass::Dependency => {
            let last = REESTABLISH_WAIT_DELAYS.len() - 1;
            let waits = usize::from(waits).min(last);
            (
                REESTABLISH_WAIT_DELAYS[waits],
                u8::try_from((waits + 1).min(last)).expect("the backoff table is short"),
            )
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RetryBudget {
    pub remaining: u8,
    pub attempts: u32,
}
impl Default for RetryBudget {
    fn default() -> Self {
        Self {
            remaining: RETRY_DELAYS.len() as u8,
            attempts: 0,
        }
    }
}
impl RetryBudget {
    pub fn next_delay(&self) -> Option<Duration> {
        (self.remaining > 0).then(|| RETRY_DELAYS[RETRY_DELAYS.len() - usize::from(self.remaining)])
    }

    pub fn record_automatic_attempt(&mut self) -> Option<Duration> {
        self.attempts = self.attempts.saturating_add(1);
        self.remaining = self.remaining.saturating_sub(1);
        self.next_delay()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dependency_waits_back_off_until_an_application_result_resets_them() {
        let mut waits = 0;
        let mut delays = Vec::new();
        for _ in 0..6 {
            let (delay, next) = next_wait(waits, OutcomeClass::Dependency);
            delays.push(delay.as_secs());
            waits = next;
        }
        assert_eq!(delays, [5, 5, 10, 30, 60, 60]);
        assert_eq!(usize::from(waits), REESTABLISH_WAIT_DELAYS.len() - 1);

        let (_, reset) = next_wait(waits, OutcomeClass::Application);
        assert_eq!(reset, 0);
        assert_eq!(next_wait(reset, OutcomeClass::Dependency).0.as_secs(), 5);
    }

    #[test]
    fn deferred_runtime_retry_budget_uses_reference_backoff_and_stops() {
        let mut budget = RetryBudget::default();
        assert_eq!(budget.remaining, 3);
        assert_eq!(
            budget.record_automatic_attempt(),
            Some(Duration::from_secs(5))
        );
        assert_eq!(
            budget.record_automatic_attempt(),
            Some(Duration::from_secs(30))
        );
        assert_eq!(budget.record_automatic_attempt(), None);
        assert_eq!(budget.remaining, 0);
        assert_eq!(budget.attempts, 3);
    }
}
