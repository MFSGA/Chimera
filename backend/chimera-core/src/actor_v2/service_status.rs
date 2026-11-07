//! Core-owned ServiceActor watch projection and daemon state transitions.
//! The status DTO and readiness semantics follow ref actor_v2/service_actor.rs.
//! OS-specific runtime ownership is provided as an explicit input.

use super::service_policy::{
    ServicePhase, ServiceRestartPolicySnapshot, phase_for_probe, settled_readiness,
};
use crate::service::compat::ServiceCompat;

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ServiceHostStatus {
    pub name: std::borrow::Cow<'static, str>,
    pub version: std::borrow::Cow<'static, str>,
    pub status: chimera_ipc::types::ServiceStatus,
    pub server: Option<chimera_ipc::api::status::StatusResBody<'static>>,
    pub phase: ServicePhase,
    pub compat: ServiceCompat,
    pub runtime_owned: bool,
    pub restart_attempts: u8,
    /// Internal ref ServiceActor readiness fence; never exposed on the UI wire.
    #[serde(skip)]
    #[specta(skip)]
    pub settled_ready: bool,
    #[serde(skip)]
    #[specta(skip)]
    pub ready_generation: u64,
}

impl ServiceHostStatus {
    pub fn probing() -> Self {
        Self {
            name: std::borrow::Cow::Borrowed("chimera-service"),
            version: std::borrow::Cow::Borrowed(""),
            status: chimera_ipc::types::ServiceStatus::NotInstalled,
            server: None,
            phase: ServicePhase::Probing,
            compat: ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: 0,
            settled_ready: false,
            ready_generation: 0,
        }
    }

    pub fn from_probe(info: chimera_ipc::types::StatusInfo<'static>, runtime_owned: bool) -> Self {
        let compat = ServiceCompat::classify(&info);
        let phase = phase_for_probe(info.status, compat.allows_service_backend(), runtime_owned);
        let (settled_ready, ready_generation) = settled_readiness(false, 0, phase);
        Self {
            name: info.name,
            version: info.version,
            status: info.status,
            server: info.server,
            phase,
            compat,
            runtime_owned,
            restart_attempts: 0,
            settled_ready,
            ready_generation,
        }
    }

    pub fn probe_failed(previous: &Self) -> Self {
        Self {
            name: previous.name.clone(),
            version: previous.version.clone(),
            status: previous.status,
            server: previous.server.clone(),
            phase: ServicePhase::Unknown,
            compat: ServiceCompat::Unknown,
            runtime_owned: false,
            restart_attempts: previous.restart_attempts,
            settled_ready: previous.settled_ready,
            ready_generation: previous.ready_generation,
        }
    }

    /// Apply upstream's settled-generation rule to the last published status.
    /// An unreadable or transitional probe is not evidence of a new daemon.
    pub fn settle_after(mut self, previous: &Self) -> Self {
        (self.settled_ready, self.ready_generation) = settled_readiness(
            previous.settled_ready,
            previous.ready_generation,
            self.phase,
        );
        self
    }

    pub fn with_restart_policy(mut self, policy: ServiceRestartPolicySnapshot) -> Self {
        self.restart_attempts = policy.attempts;
        if policy.exhausted {
            self.phase = ServicePhase::Exhausted;
        }
        self
    }

    /// Publish one real daemon observation. Without a new restart-policy
    /// snapshot, an exhausted latch cannot be cleared by another probe.
    pub fn observe_probe(
        &self,
        info: chimera_ipc::types::StatusInfo<'static>,
        runtime_owned: bool,
        policy: Option<ServiceRestartPolicySnapshot>,
    ) -> Self {
        let mut next = Self::from_probe(info, runtime_owned);
        match policy {
            Some(policy) => next = next.with_restart_policy(policy),
            None => {
                next.restart_attempts = self.restart_attempts;
                if self.phase == ServicePhase::Exhausted {
                    next.phase = ServicePhase::Exhausted;
                }
            }
        }
        next.settle_after(self)
    }

    pub fn with_phase(&self, phase: ServicePhase) -> Self {
        let mut next = self.clone();
        next.phase = phase;
        next.compat = ServiceCompat::Unknown;
        next.settle_after(self)
    }

    pub fn on_probe_failure(&self) -> Self {
        let mut next = Self::probe_failed(self);
        if self.phase == ServicePhase::Exhausted {
            next.phase = ServicePhase::Exhausted;
        }
        next.settle_after(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_ipc::types::{ServiceStatus, StatusInfo};
    use std::{borrow::Cow, path::PathBuf};

    fn observed(status: ServiceStatus) -> StatusInfo<'static> {
        StatusInfo {
            name: Cow::Borrowed("chimera-service"),
            version: Cow::Borrowed("1.10.1"),
            status,
            server: Some(chimera_ipc::api::status::StatusResBody {
                version: Cow::Borrowed(crate::service::compat::REQUIRED_SERVICE_MIN),
                core_infos: chimera_ipc::api::status::CoreInfos {
                    instance_id: None,
                    r#type: None,
                    state: chimera_ipc::api::status::CoreState::Stopped(None),
                    state_changed_at: 0,
                    config_path: None,
                    controller: None,
                    health: None,
                    revision: None,
                    detail: None,
                },
                runtime_infos: chimera_ipc::api::status::RuntimeInfos {
                    service_data_dir: Cow::Owned(PathBuf::new()),
                    service_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_config_dir: Cow::Owned(PathBuf::new()),
                    nyanpasu_data_dir: Cow::Owned(PathBuf::new()),
                },
            }),
        }
    }

    #[test]
    fn observation_keeps_generation_until_authoritative_restart() {
        let probing = ServiceHostStatus::probing();
        let ready = probing.observe_probe(observed(ServiceStatus::Running), true, None);
        assert_eq!(ready.phase, ServicePhase::Ready);
        assert_eq!(ready.ready_generation, 1);

        let unknown = ready.on_probe_failure();
        assert!(unknown.settled_ready);
        assert_eq!(unknown.ready_generation, 1);
        assert!(!unknown.runtime_owned);

        let ready_again = unknown.observe_probe(observed(ServiceStatus::Running), true, None);
        assert_eq!(ready_again.ready_generation, 1);
        let stopped = ready_again.observe_probe(observed(ServiceStatus::Stopped), false, None);
        assert!(!stopped.settled_ready);
        let restarted = stopped.observe_probe(observed(ServiceStatus::Running), true, None);
        assert_eq!(restarted.ready_generation, 2);
    }

    #[test]
    fn exhausted_latch_is_only_cleared_by_explicit_policy() {
        let ready = ServiceHostStatus::probing().observe_probe(
            observed(ServiceStatus::Running),
            true,
            None,
        );
        let exhausted = ready.observe_probe(
            observed(ServiceStatus::Stopped),
            false,
            Some(ServiceRestartPolicySnapshot {
                attempts: 3,
                exhausted: true,
            }),
        );
        assert_eq!(exhausted.phase, ServicePhase::Exhausted);
        assert_eq!(exhausted.restart_attempts, 3);
        assert_eq!(exhausted.on_probe_failure().phase, ServicePhase::Exhausted);
        let still_exhausted = exhausted.observe_probe(observed(ServiceStatus::Running), true, None);
        assert_eq!(still_exhausted.phase, ServicePhase::Exhausted);
        let rearmed = still_exhausted.observe_probe(
            observed(ServiceStatus::Running),
            true,
            Some(ServiceRestartPolicySnapshot {
                attempts: 0,
                exhausted: false,
            }),
        );
        assert_eq!(rearmed.phase, ServicePhase::Ready);
        assert_eq!(rearmed.restart_attempts, 0);
    }

    #[test]
    fn foreign_runtime_and_wire_internal_fields_are_not_published_as_ready() {
        let foreign = ServiceHostStatus::probing().observe_probe(
            observed(ServiceStatus::Running),
            false,
            None,
        );
        assert_eq!(foreign.phase, ServicePhase::Incompatible);
        assert!(!foreign.settled_ready);
        let json = serde_json::to_value(foreign).unwrap();
        assert_eq!(json["runtime_owned"], false);
        assert!(!json.as_object().unwrap().contains_key("settled_ready"));
        assert!(!json.as_object().unwrap().contains_key("ready_generation"));
    }

    #[test]
    fn transitional_phase_cannot_erase_settled_readiness() {
        let ready = ServiceHostStatus::probing().observe_probe(
            observed(ServiceStatus::Running),
            true,
            None,
        );
        let pending = ready.with_phase(ServicePhase::Restarting);
        assert!(pending.settled_ready);
        assert_eq!(pending.ready_generation, ready.ready_generation);
        assert!(matches!(pending.compat, ServiceCompat::Unknown));
    }
}
