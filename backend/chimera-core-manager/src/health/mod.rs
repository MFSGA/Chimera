//! Health-check policy and transition tracking.

pub(crate) mod driver;
pub mod probe;

use std::{num::NonZeroU32, time::Duration};

use crate::{error::Error, probe::ProbeResult, spec::ResolvedController};

pub(crate) const MAX_LAST_ERROR_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthPolicy {
    interval: Duration,
    timeout: Duration,
    failure_threshold: NonZeroU32,
    success_threshold: NonZeroU32,
    start_period: Duration,
}

/// How many consecutive probe results flip the tracker each way. Both are
/// `NonZeroU32`, so only a name says which direction one belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthThresholds {
    /// Consecutive failures before a healthy instance is called unhealthy.
    pub failure: NonZeroU32,
    /// Consecutive successes before an unhealthy instance is called healthy.
    pub success: NonZeroU32,
}

/// The five inputs of a [`HealthPolicy`]. They were five positional
/// parameters over two types — three `Duration`s and two `NonZeroU32` — so
/// any ordering within either group compiles, and the constructor validates
/// each value on its own rather than against the slot it landed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthPolicySpec {
    /// How often the probe runs.
    pub interval: Duration,
    /// The budget for a single probe run.
    pub timeout: Duration,
    pub thresholds: HealthThresholds,
    /// A grace window after start during which failures do not count.
    pub start_period: Duration,
}

impl HealthPolicy {
    pub fn new(spec: HealthPolicySpec) -> Result<Self, Error> {
        if spec.interval.is_zero() {
            return Err(Error::InvalidHealthPolicy(
                "interval must be greater than zero".into(),
            ));
        }
        if spec.timeout.is_zero() {
            return Err(Error::InvalidHealthPolicy(
                "timeout must be greater than zero".into(),
            ));
        }
        Ok(Self {
            interval: spec.interval,
            timeout: spec.timeout,
            failure_threshold: spec.thresholds.failure,
            success_threshold: spec.thresholds.success,
            start_period: spec.start_period,
        })
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    pub fn failure_threshold(&self) -> NonZeroU32 {
        self.failure_threshold
    }

    pub fn success_threshold(&self) -> NonZeroU32 {
        self.success_threshold
    }

    pub fn start_period(&self) -> Duration {
        self.start_period
    }
}

impl Default for HealthPolicy {
    fn default() -> Self {
        Self {
            interval: Duration::from_millis(250),
            timeout: Duration::from_secs(1),
            failure_threshold: NonZeroU32::new(3).expect("non-zero"),
            success_threshold: NonZeroU32::MIN,
            start_period: Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackerState {
    Starting,
    Healthy,
    Unhealthy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrackerUpdate {
    pub(crate) state: TrackerState,
    pub(crate) transitioned: bool,
    pub(crate) consecutive_failures: u32,
    pub(crate) last_error: Option<String>,
}

pub(crate) struct HealthTracker {
    policy: HealthPolicy,
    started_at: std::time::Instant,
    grace_ended: bool,
    state: TrackerState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    last_error: Option<String>,
}

impl HealthTracker {
    pub(crate) fn new(policy: HealthPolicy, started_at: std::time::Instant) -> Self {
        Self {
            policy,
            started_at,
            grace_ended: false,
            state: TrackerState::Starting,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_error: None,
        }
    }

    pub(crate) fn observe(
        &mut self,
        now: std::time::Instant,
        result: &ProbeResult,
    ) -> TrackerUpdate {
        let previous = self.state;
        match result {
            ProbeResult::Healthy => {
                self.grace_ended = true;
                self.consecutive_failures = 0;
                self.last_error = None;
                if self.state == TrackerState::Healthy {
                    self.consecutive_successes = 0;
                } else {
                    self.consecutive_successes = self.consecutive_successes.saturating_add(1);
                    if self.consecutive_successes >= self.policy.success_threshold.get() {
                        self.state = TrackerState::Healthy;
                        self.consecutive_successes = 0;
                    }
                }
            }
            ProbeResult::Unhealthy { detail } => {
                self.consecutive_successes = 0;
                self.last_error = detail.as_deref().map(cap_detail);
                let grace_active =
                    !self.grace_ended && now < self.started_at + self.policy.start_period;
                if !grace_active {
                    self.consecutive_failures = self.consecutive_failures.saturating_add(1);
                    if self.state != TrackerState::Unhealthy
                        && self.consecutive_failures >= self.policy.failure_threshold.get()
                    {
                        self.state = TrackerState::Unhealthy;
                    }
                }
            }
        }
        TrackerUpdate {
            state: self.state,
            transitioned: previous != self.state,
            consecutive_failures: self.consecutive_failures,
            last_error: self.last_error.clone(),
        }
    }
}

fn cap_detail(detail: &str) -> String {
    if detail.len() <= MAX_LAST_ERROR_BYTES {
        return detail.to_owned();
    }
    let mut end = MAX_LAST_ERROR_BYTES;
    while !detail.is_char_boundary(end) {
        end -= 1;
    }
    detail[..end].to_owned()
}

pub(crate) fn build_control_client(
    controller: &ResolvedController,
    timeout: Duration,
) -> Result<clash_api::Client, Error> {
    let mut builder = clash_api::Client::builder(controller.host.clone())
        .configure_reqwest(|builder| builder.timeout(timeout));
    if let Some(secret) = &controller.secret {
        builder = builder.secret(secret.as_str());
    }
    Ok(builder.build()?)
}
