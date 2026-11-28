//! Shared application candidate admission and runtime effect-port proofs.
//! Host preflight and legacy service handoff remain in the desktop composition.
use super::{ChimeraClient, reconciler::reestablish::KnownRuntimeState};
use crate::state::{application::ApplicationSnapshot, mutation::impact};
use anyhow::Result;
use chimera_config::application::{ChimeraAppConfig, ChimeraAppConfigPatch};
use struct_patch::Patch as _;

use super::mutation_outcome::{Degradation, DegradationPhase};
use chimera_contracts::runtime::{DegradationReason, MutationOutcome};

/// The committed source and its exact publication receipt. Native owner
/// commits have already published their notification; legacy ones have not.
pub struct ApplicationCommitReport {
    pub snapshot: ApplicationSnapshot,
    pub route: ApplicationPatchRoute,
    pub notified_version: Option<u64>,
    pub degradations: Vec<Degradation>,
}

impl ApplicationCommitReport {
    pub fn into_outcome(self) -> MutationOutcome<()> {
        let receipt = application_commit_receipt(
            &self.snapshot,
            self.notified_version,
            self.route.requires_reconcile,
            &self.degradations,
        );
        let degradations = self
            .degradations
            .into_iter()
            .map(|item| chimera_contracts::runtime::Degradation {
                phase: item.phase,
                reason: DegradationReason::LegacyConfigurationFailure {
                    legacy_code: item.code,
                },
                message: item.message,
                retryable: item.retryable,
            })
            .collect();
        MutationOutcome::from_parts((), degradations).with_commit(receipt)
    }
}

impl ChimeraClient {
    pub fn validate_application_candidate(
        &self,
        patch: &ChimeraAppConfigPatch,
    ) -> Result<(ApplicationSnapshot, ChimeraAppConfig)> {
        let before = self.application().snapshot();
        let mut candidate = before.state.clone();
        candidate.apply(patch.clone());
        crate::state::application::validate_network_automation_policy(&candidate)?;
        chimera_config::application::validate_public_ip_probe_sources(
            &candidate.stun_servers,
            &candidate.ip_lookup_providers,
        )
        .map_err(anyhow::Error::msg)?;
        let clash = self.get_clash_config()?;
        super::clash_operations::validate_clash_for_application(&clash, &candidate)?;
        Ok((before, candidate))
    }

    /// The ref facade owns application admission, the commit route, and
    /// publication. The shell provides its native preflight and legacy mirror.
    pub async fn patch_application_validated_with<Preflight, Save>(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
        checked_gate_ready: bool,
        preflight: Preflight,
        save_legacy: Save,
    ) -> Result<ApplicationCommitReport>
    where
        Preflight: FnOnce(&ChimeraAppConfigPatch) -> Result<()>,
        Save: FnOnce() -> Result<()>,
    {
        let (before, candidate) = self.validate_application_candidate(&patch)?;
        preflight(&patch)?;
        #[cfg(windows)]
        validate_windows_service_mode_tun_transition(
            patch.enable_service_mode,
            self.get_clash_config()?.enable_tun_mode,
            None,
        )?;
        let route = application_patch_route(
            &patch,
            &before.state,
            &candidate,
            service_connected,
            checked_gate_ready,
        );
        let reconciler = self
            .reconciler()
            .ok_or_else(|| anyhow::anyhow!("runtime reconciler is not connected"))?;
        let (mut degradations, notified_version, snapshot) = if route.native() {
            let snapshot = self
                .commit_native_application_candidate(before.version, candidate, route, &patch)
                .await?;
            (Vec::new(), Some(snapshot.version), snapshot)
        } else if route.requires_reconcile {
            let report = reconciler.patch_application_config(patch).await?;
            (
                report.degradations,
                report.notified_version,
                self.application().get().await?,
            )
        } else {
            let snapshot = self.application().patch_typed(patch).await?;
            (Vec::new(), None, snapshot)
        };
        if notified_version.is_none() {
            reconciler.source_committed(
                super::reconciler::committed::SourceDomain::Application,
                snapshot.version,
                degradations
                    .iter()
                    .any(|item| item.code == "application_runtime_deferred" && item.retryable),
            );
        }
        self.configure_app_update(&snapshot.state);
        if let Err(error) = save_legacy() {
            degradations.push(Degradation {
                phase: DegradationPhase::LegacyMirror,
                code: "legacy_application_mirror_write_failed".into(),
                message: format!("failed to persist legacy application mirror: {error:#}"),
                retryable: true,
            });
        }
        Ok(ApplicationCommitReport {
            snapshot,
            route,
            notified_version,
            degradations,
        })
    }

    /// The existing source actor owns CAS, check admission and committed notices.
    pub async fn commit_native_application_candidate(
        &self,
        expected_version: u64,
        candidate: ChimeraAppConfig,
        route: ApplicationPatchRoute,
        patch: &ChimeraAppConfigPatch,
    ) -> Result<ApplicationSnapshot> {
        anyhow::ensure!(
            route.native(),
            "legacy application transaction requires host workflow"
        );
        self.application()
            .commit_native(
                expected_version,
                candidate,
                route.native_checked,
                false,
                super::effects::plan::requested_owners(patch),
            )
            .await
    }

    pub async fn expected_system_proxy_port(&self) -> anyhow::Result<Option<u16>> {
        let clash = self.get_clash_config()?;
        let receipt = match self.core().observe_runtime_baseline().await? {
            KnownRuntimeState::Applied(receipt) => Some(receipt),
            KnownRuntimeState::Stopped => self.core().last_confirmed_runtime_receipt(),
            KnownRuntimeState::NeverApplied => None,
        };
        Ok(crate::client::ports::effective_mixed_port(
            receipt.as_deref(),
            &clash,
        ))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub async fn active_system_proxy_port(&self) -> anyhow::Result<Option<u16>> {
        let receipt = match self.core().observe_runtime_baseline().await? {
            KnownRuntimeState::Applied(receipt) => receipt,
            KnownRuntimeState::Stopped | KnownRuntimeState::NeverApplied => {
                return Ok(None);
            }
        };
        Ok(crate::client::ports::confirmed_mixed_port(Some(&receipt)))
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub async fn confirmed_effect_port_binding(
        &self,
    ) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
        let receipt = match self.core().active_runtime_receipt().await {
            Ok(receipt) => receipt,
            Err(error) => {
                tracing::debug!(%error, "no active runtime receipt is available for application effects");
                return None;
            }
        };
        // The legacy runtime receipt records a successful prior apply; it
        // alone cannot prove that the ref CoreActor still owns that instance.
        // Fence the port publication against both sides of this async read.
        if let (Some(core), Some(receipt)) = (self.core_api(), receipt.as_deref()) {
            let before = match core.refresh_status().await {
                Ok(status) => status,
                Err(error) => {
                    tracing::debug!(%error, "CoreActor cannot confirm runtime effect ports");
                    return None;
                }
            };
            if !actor_confirms_effect_ports(receipt, &before, &before) {
                return None;
            }
            let after = match core.refresh_status().await {
                Ok(status) => status,
                Err(error) => {
                    tracing::debug!(%error, "CoreActor changed during runtime port verification");
                    return None;
                }
            };
            if !actor_confirms_effect_ports(receipt, &before, &after) {
                return None;
            }
        }
        match resolved_effect_port_binding(receipt.as_deref()) {
            Some(binding) => Some(binding),
            None => {
                tracing::warn!(
                    "the active core has no confirmed mixed port for application effects"
                );
                None
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn actor_confirms_effect_ports(
    receipt: &crate::client::runtime::RuntimeApplyReceipt,
    before: &crate::actor_v2::router::CoreStatusProjection,
    after: &crate::actor_v2::router::CoreStatusProjection,
) -> bool {
    use crate::actor_v2::router::EndpointConnectivity;
    matches!(before.connectivity, EndpointConnectivity::Connected)
        && matches!(after.connectivity, EndpointConnectivity::Connected)
        && before.host == after.host
        && before.generation == after.generation
        && receipt.confirms_projection(before)
        && receipt.confirms_projection(after)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn resolved_effect_port_binding(
    receipt: Option<&crate::client::runtime::RuntimeApplyReceipt>,
) -> Option<chimera_config::runtime::executor::ResolvedPortBindings> {
    receipt
        .map(|receipt| receipt.ports.bindings().mixed_port)
        .filter(|port| *port != 0)
        .map(
            |mixed_port| chimera_config::runtime::executor::ResolvedPortBindings {
                mixed_port,
                ..Default::default()
            },
        )
}

/// Exact route decision for the current checked owner and the legacy handoff.
#[derive(Debug, Clone, Copy)]
pub struct ApplicationPatchRoute {
    pub requires_reconcile: bool,
    native_checked: bool,
    native_source_only: bool,
}

impl ApplicationPatchRoute {
    pub fn native(&self) -> bool {
        self.native_checked || self.native_source_only
    }
}

pub fn application_patch_route(
    patch: &ChimeraAppConfigPatch,
    before: &ChimeraAppConfig,
    candidate: &ChimeraAppConfig,
    service_connected: bool,
    checked_gate_ready: bool,
) -> ApplicationPatchRoute {
    let requires_reconcile = impact::application_patch_requires_reconcile(patch, service_connected)
        || patch.enable_service_mode.is_some();
    ApplicationPatchRoute {
        requires_reconcile,
        native_checked: checked_gate_ready
            && checked_application_patch_eligible(patch, before, service_connected),
        native_source_only: checked_gate_ready
            && !requires_reconcile
            && !impact::reaches_runtime(before, candidate, &[]),
    }
}

/// Use ref checked-source handoff only for an isolated local-host runtime
/// enhancement patch. Core/Service ownership and other application settings
/// may carry separate effects; admitting a combined patch would skip their
/// legacy postcommit workflow after publishing CheckedSourceCommitted.
pub fn checked_application_patch_eligible(
    patch: &ChimeraAppConfigPatch,
    previous: &ChimeraAppConfig,
    service_connected: bool,
) -> bool {
    use struct_patch::Status as _;

    if patch.enable_builtin_enhanced.is_none() || previous.enable_service_mode || service_connected
    {
        return false;
    }
    // Compare every field, including nested patch DTOs, against the empty
    // patch so a future app setting never silently becomes Checked-eligible.
    let mut remainder = patch.clone();
    remainder.enable_builtin_enhanced = None;
    remainder.is_empty()
}

#[cfg(any(target_os = "windows", test, feature = "test-support"))]
pub fn validate_windows_service_mode_tun_transition(
    service_mode: Option<bool>,
    current_tun: bool,
    target_tun: Option<bool>,
) -> Result<()> {
    if service_mode == Some(false) && target_tun.unwrap_or(current_tun) {
        anyhow::bail!("disable TUN before disabling Service Mode on Windows");
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn application_patch_touches_system_proxy(patch: &ChimeraAppConfigPatch) -> bool {
    patch.enable_system_proxy.is_some()
        || patch.enable_proxy_guard.is_some()
        || patch.system_proxy_bypass.is_some()
        || patch.proxy_guard_interval.is_some()
        || patch.pac_url.is_some()
}

#[cfg(test)]
mod checked_application_route_tests {
    use super::*;

    #[test]
    fn only_local_enhancement_requests_may_use_checked_apply() {
        let mut previous = ChimeraAppConfig::default();
        previous.enable_service_mode = false;
        let mut patch = ChimeraAppConfigPatch {
            enable_builtin_enhanced: Some(!previous.enable_builtin_enhanced),
            ..Default::default()
        };
        assert!(checked_application_patch_eligible(&patch, &previous, false));
        assert!(
            !checked_application_patch_eligible(&patch, &previous, true),
            "a connected Service host retains its dedicated handoff"
        );
        previous.enable_service_mode = true;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        previous.enable_service_mode = false;
        patch.core = Some(previous.core);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.core = None;
        patch.enable_service_mode = Some(false);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_service_mode = None;
        patch.enable_builtin_enhanced = None;
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));

        // A combined enhancement/OS-effect patch must stay with the existing
        // owner that handles proxy, launch and network action side effects.
        patch.enable_builtin_enhanced = Some(false);
        patch.enable_system_proxy = Some(true);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_system_proxy = None;
        patch.enable_auto_launch = Some(true);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.enable_auto_launch = None;
        patch.hotkeys = Some(vec!["toggle_system_proxy,Ctrl+Shift+P".into()]);
        assert!(!checked_application_patch_eligible(
            &patch, &previous, false
        ));
        patch.hotkeys = None;
        assert!(checked_application_patch_eligible(&patch, &previous, false));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chimera_config::clash::config::ClashConfig;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn effect_ports_require_current_ref_core_actor_owner_and_applied_revision() {
        use crate::actor_v2::{
            endpoint::{CoreStatusSnapshot, ExecutionHost},
            router::{CoreStatusProjection, EndpointConnectivity},
        };
        use chimera_ipc::api::status::{CoreStateDetail, RevisionIdInfo};

        let mut receipt = crate::client::ports::test_runtime_receipt(Default::default());
        receipt.controller_generation = Some(8);
        let before = CoreStatusProjection {
            host: ExecutionHost::Local,
            generation: 8,
            connectivity: EndpointConnectivity::Connected,
            snapshot: Some(CoreStatusSnapshot {
                instance_id: Some("current-runtime".into()),
                controller: None,
                state: Some(CoreStateDetail::Running {
                    epoch: receipt.applied_revision.epoch,
                    pid: 22,
                }),
                state_changed_at: 7,
                revision: Some(RevisionIdInfo {
                    epoch: receipt.applied_revision.epoch,
                    generation: receipt.applied_revision.generation,
                    effective_hash: receipt.applied_revision.effective_hash.clone(),
                }),
                source_hash: Some(receipt.applied_revision.source_hash.clone()),
                healthy: Some(true),
                applied_kind: None,
            }),
        };
        assert!(actor_confirms_effect_ports(&receipt, &before, &before));

        let mut handoff = before.clone();
        handoff.host = ExecutionHost::Service;
        handoff.generation += 1;
        assert!(!actor_confirms_effect_ports(&receipt, &before, &handoff));

        let mut stopped = before.clone();
        stopped.snapshot.as_mut().unwrap().state = Some(CoreStateDetail::Stopped { reason: None });
        assert!(!actor_confirms_effect_ports(&receipt, &before, &stopped));

        let mut stale_revision = before.clone();
        stale_revision
            .snapshot
            .as_mut()
            .unwrap()
            .revision
            .as_mut()
            .unwrap()
            .generation += 1;
        assert!(!actor_confirms_effect_ports(
            &receipt,
            &before,
            &stale_revision
        ));

        let mut disconnected = before.clone();
        disconnected.connectivity = EndpointConnectivity::Degraded {
            desired: ExecutionHost::Local,
            reason: "connection lost".into(),
        };
        assert!(!actor_confirms_effect_ports(
            &receipt,
            &before,
            &disconnected
        ));
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn effects_receive_the_confirmed_mixed_port_not_the_controller_port() {
        use std::net::TcpListener;

        use crate::client::ports::{
            CandidatePortBindings, SessionPortResolver, test_runtime_receipt,
        };

        assert!(resolved_effect_port_binding(None).is_none());
        let empty_receipt = test_runtime_receipt(CandidatePortBindings::default());
        assert!(resolved_effect_port_binding(Some(&empty_receipt)).is_none());
        let mixed_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let mixed_port = mixed_probe.local_addr().unwrap().port();
        let controller_probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let controller_port = controller_probe.local_addr().unwrap().port();
        drop((mixed_probe, controller_probe));

        let mut clash = ClashConfig::default();
        clash.mixed_port = chimera_config::clash::config::clash_strategy::PortStrategy {
            kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
            start_port: mixed_port,
        };
        clash.external_controller.port =
            chimera_config::clash::config::clash_strategy::PortStrategy {
                kind: chimera_config::clash::config::clash_strategy::PortStrategyKind::Fixed,
                start_port: controller_port,
            };
        let candidate = SessionPortResolver::default()
            .resolve_candidate(&clash)
            .unwrap();
        let receipt = test_runtime_receipt(candidate);
        assert_eq!(
            receipt
                .ports
                .bindings()
                .external_controller
                .as_deref()
                .unwrap()
                .rsplit(':')
                .next()
                .unwrap()
                .parse::<u16>()
                .unwrap(),
            controller_port
        );
        assert_eq!(
            resolved_effect_port_binding(Some(&receipt))
                .expect("confirmed runtime port should be published")
                .mixed_port,
            mixed_port
        );
    }

    #[test]
    fn route_keeps_service_preference_and_combined_changes_with_legacy_owner() {
        let before = ChimeraAppConfig::default();
        let service_patch = ChimeraAppConfigPatch {
            enable_service_mode: Some(true),
            ..Default::default()
        };
        let mut candidate = before.clone();
        candidate.apply(service_patch.clone());
        let route = application_patch_route(&service_patch, &before, &candidate, false, true);
        assert!(route.requires_reconcile);
        assert!(!route.native());
        let source_patch = ChimeraAppConfigPatch {
            release_channel: Some(Some(chimera_config::application::ReleaseChannel::Stable)),
            ..Default::default()
        };
        let mut candidate = before.clone();
        candidate.apply(source_patch.clone());
        assert!(application_patch_route(&source_patch, &before, &candidate, false, true).native());
        assert!(
            !application_patch_route(&source_patch, &before, &candidate, false, false).native()
        );
    }
}

pub fn application_commit_receipt(
    snapshot: &ApplicationSnapshot,
    notified_version: Option<u64>,
    requires_reconcile: bool,
    degradations: &[super::mutation_outcome::Degradation],
) -> super::mutation_outcome::CommitReceipt {
    use super::mutation_outcome::{CommitReceipt, RuntimeCommitStatus};
    CommitReceipt {
        operation_id: snapshot.operation_id.map(|id| id.to_string()),
        domain: "application".into(),
        source_version: notified_version.unwrap_or(snapshot.version),
        runtime: if degradations
            .iter()
            .any(|item| item.code == "application_runtime_deferred")
        {
            RuntimeCommitStatus::Deferred
        } else if snapshot.reaches_runtime || (requires_reconcile && notified_version.is_some()) {
            RuntimeCommitStatus::Pending
        } else {
            RuntimeCommitStatus::Unchanged
        },
    }
}
