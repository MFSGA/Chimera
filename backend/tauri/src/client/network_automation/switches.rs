//! The ref capture-switch adapter, routed through Chimera's existing
//! validated application/Clash commit owner instead of a second OS writer.
//!
//! This adapter is not yet installed in the production setup: the network
//! policy is not persisted and the host observer is not wired to the actor.

use chimera_config::{
    application::{ChimeraAppConfigPatch, MechanismAction, MechanismActions},
    clash::config::{
        ClashConfig, ClashConfigPatch, TransparentProxyMode,
        clash_strategy::{PortStrategy, PortStrategyKind},
    },
};
use chimera_contracts::network_automation::{MechanismOutcome, MechanismOutcomes};

use super::ports::CaptureSwitches;
use crate::client::ChimeraClient;

pub(crate) struct ConfigCaptureSwitches {
    pub(crate) client: ChimeraClient,
}

#[async_trait::async_trait]
impl CaptureSwitches for ConfigCaptureSwitches {
    async fn apply(&self, actions: MechanismActions) -> MechanismOutcomes {
        // Keep ref's order. A proxy preference goes through the same effects
        // owner as a manual settings edit; the two capture switches are one
        // validated Clash source commit, never separate writes.
        let system_proxy = self.apply_system_proxy(actions.system_proxy).await;
        let (tun, transparent_proxy) = self
            .apply_capture(actions.tun, actions.transparent_proxy)
            .await;

        MechanismOutcomes {
            system_proxy,
            tun,
            transparent_proxy,
        }
    }
}

impl ConfigCaptureSwitches {
    async fn apply_system_proxy(&self, action: MechanismAction) -> MechanismOutcome {
        let enabled = match self.client.get_app_config() {
            Ok(config) => config.enable_system_proxy,
            Err(error) => return failed(&error),
        };
        let Some(target) = requested(action).filter(|target| *target != enabled) else {
            return MechanismOutcome::Unchanged;
        };

        let patch = ChimeraAppConfigPatch {
            enable_system_proxy: Some(target),
            ..Default::default()
        };
        // This is the one application owner that performs legacy mirroring,
        // system-proxy effect publication, and Core degradation reporting.
        match self.client.patch_app_config(patch).await {
            Ok(outcome) => mutation_outcome(outcome, target),
            Err(error) => failed(&error),
        }
    }

    /// TUN and transparent capture exclude each other and must be committed
    /// in the same patch. A kept switch may be disabled by the other switch.
    async fn apply_capture(
        &self,
        tun: MechanismAction,
        transparent_proxy: MechanismAction,
    ) -> (MechanismOutcome, MechanismOutcome) {
        let config = match self.client.get_clash_config() {
            Ok(config) => config,
            Err(error) => return (failed(&error), failed(&error)),
        };
        let plan = CapturePlan::new(&config, tun, transparent_proxy);

        let mut refusal = None;
        let mut plan = plan;
        if plan.transparent_target && !plan.transparent_before {
            let app = match self.client.get_app_config() {
                Ok(app) => app,
                Err(error) => return (failed(&error), failed(&error)),
            };
            let supported = if !cfg!(target_os = "linux") || !app.enable_service_mode {
                false
            } else {
                match self.client.get_transparent_proxy_status().await {
                    Ok(status) => status.supported,
                    Err(error) => {
                        refusal = Some(failed(&error));
                        false
                    }
                }
            };
            if !supported {
                refusal.get_or_insert_with(|| MechanismOutcome::Failed {
                    message: "transparent capture requires a supported Linux Service".into(),
                });
                // Ref: a refused transparent request does not suppress the
                // independent TUN action, and cannot disable a working TUN.
                plan = CapturePlan::new(&config, tun, MechanismAction::Keep);
            }
        }

        if !plan.tun_changed() && !plan.transparent_changed() {
            return (
                MechanismOutcome::Unchanged,
                refusal.unwrap_or(MechanismOutcome::Unchanged),
            );
        }

        let patch = plan.patch(&config);
        let result = self.client.patch_clash_config(patch).await;
        let failure = match result {
            Ok(outcome) if outcome.degradations().is_empty() => None,
            Ok(outcome) => Some(uncertain_commit(outcome.degradations())),
            Err(error) => Some(failed(&error)),
        };
        let changed_outcome = |changed: bool, target: bool| match &failure {
            _ if !changed => MechanismOutcome::Unchanged,
            Some(error) => error.clone(),
            None => switched(target),
        };

        (
            changed_outcome(plan.tun_changed(), plan.tun_target),
            refusal.unwrap_or_else(|| {
                changed_outcome(plan.transparent_changed(), plan.transparent_target)
            }),
        )
    }
}

/// Pure ref-aligned capture decision. Keep the plan independent of side
/// effects so the interlock remains testable on every host OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CapturePlan {
    tun_before: bool,
    transparent_before: bool,
    tun_target: bool,
    transparent_target: bool,
}

impl CapturePlan {
    fn new(config: &ClashConfig, tun: MechanismAction, transparent: MechanismAction) -> Self {
        let tun_before = config.enable_tun_mode;
        let transparent_before = config.transparent_proxy.mode != TransparentProxyMode::Disabled;
        let mut tun_target = requested(tun).unwrap_or(tun_before);
        let mut transparent_target = requested(transparent).unwrap_or(transparent_before);
        if tun == MechanismAction::Enable {
            transparent_target = false;
        }
        if transparent == MechanismAction::Enable {
            tun_target = false;
        }
        Self {
            tun_before,
            transparent_before,
            tun_target,
            transparent_target,
        }
    }

    fn tun_changed(self) -> bool {
        self.tun_before != self.tun_target
    }

    fn transparent_changed(self) -> bool {
        self.transparent_before != self.transparent_target
    }

    fn patch(self, config: &ClashConfig) -> ClashConfigPatch {
        let mut patch = ClashConfigPatch::default();
        patch.enable_tun_mode = self.tun_changed().then_some(self.tun_target);
        if self.transparent_changed() {
            patch.transparent_proxy.mode = Some(if self.transparent_target {
                TransparentProxyMode::Tproxy
            } else {
                TransparentProxyMode::Disabled
            });
            if self.transparent_target && config.tproxy_port.is_none() {
                patch.tproxy_port = Some(Some(PortStrategy {
                    kind: PortStrategyKind::Fixed,
                    start_port: 7894,
                }));
            }
        }
        patch
    }
}

fn requested(action: MechanismAction) -> Option<bool> {
    match action {
        MechanismAction::Keep => None,
        MechanismAction::Enable => Some(true),
        MechanismAction::Disable => Some(false),
    }
}

fn switched(enabled: bool) -> MechanismOutcome {
    if enabled {
        MechanismOutcome::Enabled
    } else {
        MechanismOutcome::Disabled
    }
}

fn failed(error: impl std::fmt::Display) -> MechanismOutcome {
    MechanismOutcome::Failed {
        message: error.to_string(),
    }
}

fn uncertain_commit(degradations: &[crate::client::runtime::Degradation]) -> MechanismOutcome {
    MechanismOutcome::Failed {
        message: format!(
            "configuration committed, but runtime/effect application is degraded: {}",
            degradations
                .iter()
                .map(|item| item.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ),
    }
}

fn mutation_outcome(outcome: crate::client::MutationOutcome<()>, target: bool) -> MechanismOutcome {
    if outcome.degradations().is_empty() {
        switched(target)
    } else {
        uncertain_commit(outcome.degradations())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use struct_patch::Patch as _;

    #[test]
    fn tun_enable_turns_kept_transparent_proxy_off_in_one_patch() {
        let mut config = ClashConfig::default();
        config.transparent_proxy.mode = TransparentProxyMode::Tproxy;
        config.tproxy_port = Some(PortStrategy::new_allow_fallback(7894));

        let plan = CapturePlan::new(&config, MechanismAction::Enable, MechanismAction::Keep);
        assert!(plan.tun_changed());
        assert!(plan.transparent_changed());
        let patch = plan.patch(&config);
        assert_eq!(patch.enable_tun_mode, Some(true));
        assert_eq!(
            patch.transparent_proxy.mode,
            Some(TransparentProxyMode::Disabled)
        );
        let mut candidate = config.clone();
        candidate.apply(patch);
        assert!(candidate.enable_tun_mode);
        assert_eq!(
            candidate.transparent_proxy.mode,
            TransparentProxyMode::Disabled
        );
    }

    #[test]
    fn enabling_transparent_capture_disables_kept_tun_and_adds_port() {
        let mut config = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        let plan = CapturePlan::new(&config, MechanismAction::Keep, MechanismAction::Enable);
        let patch = plan.patch(&config);
        assert_eq!(patch.enable_tun_mode, Some(false));
        assert_eq!(
            patch.transparent_proxy.mode,
            Some(TransparentProxyMode::Tproxy)
        );
        assert_eq!(
            patch
                .tproxy_port
                .as_ref()
                .and_then(|port| port.as_ref())
                .map(|p| p.start_port),
            Some(7894)
        );
        config.apply(patch);
        assert!(!config.enable_tun_mode);
        assert_eq!(config.transparent_proxy.mode, TransparentProxyMode::Tproxy);
    }

    #[test]
    fn transparent_disable_keeps_explicit_redir_listener_configuration() {
        let mut config = ClashConfig::default();
        config.transparent_proxy.mode = TransparentProxyMode::Redir;
        config.redir_port = Some(PortStrategy::new_allow_fallback(7893));
        let patch = CapturePlan::new(&config, MechanismAction::Keep, MechanismAction::Disable)
            .patch(&config);
        config.apply(patch);
        assert_eq!(
            config.transparent_proxy.mode,
            TransparentProxyMode::Disabled
        );
        assert_eq!(config.redir_port.as_ref().map(|p| p.start_port), Some(7893));
    }

    #[test]
    fn keep_actions_do_not_write_unrelated_state() {
        let config = ClashConfig::default();
        let plan = CapturePlan::new(&config, MechanismAction::Keep, MechanismAction::Keep);
        assert!(!plan.tun_changed());
        assert!(!plan.transparent_changed());
        let patch = plan.patch(&config);
        assert_eq!(patch.enable_tun_mode, None);
        assert_eq!(patch.transparent_proxy.mode, None);
        assert_eq!(patch.tproxy_port, None);
    }

    #[test]
    fn contradictory_enable_actions_fail_closed_like_ref() {
        // A valid policy rejects this combination before reaching the port.
        // Ref applies both interlocks even for an invalid direct port call.
        let config = ClashConfig::default();
        let plan = CapturePlan::new(&config, MechanismAction::Enable, MechanismAction::Enable);
        assert!(!plan.tun_target);
        assert!(!plan.transparent_target);
    }

    #[test]
    fn a_degraded_commit_is_not_reported_as_an_applied_os_switch() {
        let degraded = crate::client::MutationOutcome::from_parts(
            (),
            vec![crate::client::runtime::Degradation {
                phase: crate::client::runtime::DegradationPhase::LegacyMirror,
                code: "legacy_mirror_unavailable".into(),
                message: "injected mirror error".into(),
                retryable: true,
            }],
        );
        assert!(matches!(
            mutation_outcome(degraded, true),
            MechanismOutcome::Failed { .. }
        ));
    }
}
