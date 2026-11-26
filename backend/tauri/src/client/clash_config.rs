//! Clash desired-configuration client boundary.
//!
//! REF owns Clash desired state through a typed config client. Chimera still
//! persists the legacy Clash guard mapping plus typed runtime overrides, so
//! this transitional client keeps those storage semantics while centralizing
//! reads, validation, mutation, and running-core coordination here.

use std::sync::Arc;

use anyhow::{Context as _, Result, bail};
use camino::Utf8PathBuf;
use chimera_config::clash::config::{
    ClashConfig, ClashConfigPatch,
    clash_strategy::{PortStrategy, PortStrategyKind},
};
use chimera_ipc::api::status::CoreState;
use serde_yaml::Mapping;
use struct_patch::Patch;

use crate::{
    config::{core::Config, runtime::ClashConfigOverrides},
    core::{clash::transaction::TransactionOutcome, handle},
    log_err,
    state::{clash_config::ClashConfigSnapshot, mirror::ClashLegacyBridge},
};

use super::ChimeraClient;

/// Desktop runtime orchestration over the shared persistent source owner.
/// No duplicate state, mailbox or persistence.
#[derive(Clone)]
pub(crate) struct ClashConfigClient(chimera_core::client::clash_config::ClashConfigClient);

impl std::ops::Deref for ClashConfigClient {
    type Target = chimera_core::client::clash_config::ClashConfigClient;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ClashConfigClient {
    pub(super) fn from_shared(
        client: chimera_core::client::clash_config::ClashConfigClient,
    ) -> Self {
        Self(client)
    }
    pub(super) async fn new(
        path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> Result<Self> {
        Ok(Self(
            chimera_core::client::clash_config::ClashConfigClient::new(path, seed, bridge).await?,
        ))
    }
    #[cfg(test)]
    pub(crate) fn legacy() -> Result<Self> {
        Ok(Self(
            chimera_core::client::clash_config::ClashConfigClient::legacy()?,
        ))
    }
    #[cfg(test)]
    pub(crate) fn legacy_with(config: ClashConfig) -> Result<Self> {
        Ok(Self(
            chimera_core::client::clash_config::ClashConfigClient::legacy_with(config)?,
        ))
    }
    pub(super) async fn apply_legacy_patch_runtime(
        &self,
        owner: &ChimeraClient,
        patch: &ClashConfigPatch,
    ) -> Result<()> {
        if patch.enable_tun_mode.is_some() {
            log::debug!(target: "app", "toggle tun mode");
            #[cfg(target_os = "linux")]
            {
                use crate::utils::dirs::check_core_permission;
                let current_core = Config::verge().data().clash_core.unwrap_or_default();
                let current_core: chimera_utils::core::CoreType = (&current_core).into();
                let service_state = crate::core::service::ipc::get_ipc_state();
                if !service_state.is_connected()
                    && check_core_permission(&current_core)
                        .inspect_err(|e| {
                            log::error!(target: "app", "clash core is not granted the necessary permissions, grant it: {e:?}");
                        })
                        .is_ok_and(|v| !v)
                {
                    log::debug!(target: "app", "clash core permission is missing, tun toggle will restart core and may still fail");
                };
            }

            update_core_config(owner).await?;
        }

        Ok(())
    }

    async fn patch(&self, owner: &ChimeraClient, patch: Mapping) -> Result<ClashCommitReport> {
        ClashConfigOverrides::from_mapping(&patch)?;
        self.patch_with_overrides(owner, patch, true).await
    }

    /// Ref typed Clash source transition: read-only candidate validation,
    /// source CAS and durable publication, then best-effort runtime apply.
    /// Postcommit failures *never* roll back a persisted config version.
    pub(super) async fn patch_typed_config(
        &self,
        owner: &ChimeraClient,
        patch: ClashConfigPatch,
    ) -> Result<ClashCommitReport> {
        let expected_sources = owner.committed_runtime_source_versions();
        if owner.inner.checked_gate.is_some() {
            let app = owner.get_app_config()?;
            let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
            // The same committed source snapshot decides DNS/Hosts ownership
            // and the exact expected CAS version; do not re-read independently.
            let before = self.get_snapshot().await?;
            let mut next = before.state.clone();
            next.apply(patch.clone());
            let checked =
                checked_clash_patch_eligible(&patch, &before.state, &app, service_connected);
            if checked || !should_reconcile_clash_patch(&before.state, &next, &patch) {
                return self
                    .commit_native_candidate(before, next, checked, false)
                    .await;
            }
        }
        let candidate_patch = patch.clone();
        self.patch_typed_config_with(
            patch,
            |candidate| owner.check_clash_runtime_candidate(candidate),
            || {
                super::reconciler::committed::apply_with_dependency_fence(
                    super::reconciler::committed::SourceDomain::Clash,
                    expected_sources,
                    || async { owner.committed_runtime_source_versions() },
                    || async {
                        if candidate_patch.enable_tun_mode.is_some() {
                            self.apply_legacy_patch_runtime(owner, &candidate_patch)
                                .await
                        } else {
                            update_core_config(owner).await
                        }
                    },
                )
            },
            || {
                Config::clash().data().save_config()?;
                Config::verge().data().save_file()?;
                Ok(())
            },
        )
        .await
    }

    async fn commit_native_candidate(
        &self,
        before: ClashConfigSnapshot,
        next: ClashConfig,
        force_check: bool,
        interrupt_mode: bool,
    ) -> Result<ClashCommitReport> {
        self.0
            .commit_native_candidate_with(before, next, force_check, interrupt_mode, || {
                Config::clash().data().save_config()?;
                Config::verge().data().save_file()?;
                Ok(())
            })
            .await
    }

    async fn patch_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
        allow_checked: bool,
    ) -> Result<ClashCommitReport> {
        let patch = overrides.to_mapping();
        self.patch_with_overrides(owner, patch, allow_checked).await
    }

    /// Follow the source-owned candidate check and commit before reconciling
    /// the running core. A failed apply never rolls back committed config.
    async fn patch_running_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        // TransactionOutcome::Committed historically means the *running*
        // Core apply has finished. Until this API gains a Pending receipt,
        // do not convert it into an asynchronous Checked submission.
        match self.patch_overrides(owner, overrides, false).await {
            Err(error) => TransactionOutcome::Rejected {
                primary_error: format!("{error:#}"),
            },
            Ok(report) => {
                for degradation in &report.degradations {
                    tracing::warn!(
                        code = %degradation.code,
                        message = %degradation.message,
                        "running Clash override committed with degraded follow-up"
                    );
                }
                if let Some(deferred) = report
                    .degradations
                    .iter()
                    .find(|item| item.code == "clash_runtime_deferred")
                {
                    TransactionOutcome::CommittedDeferred {
                        message: deferred.message.clone(),
                    }
                } else {
                    TransactionOutcome::Committed
                }
            }
        }
    }

    async fn patch_with_overrides(
        &self,
        owner: &ChimeraClient,
        patch: Mapping,
        allow_checked: bool,
    ) -> Result<ClashCommitReport> {
        let before = self.get_snapshot().await?;
        let current = before.state.clone();
        let next = apply_mapping_to_typed_clash(&current, &patch)?;
        let plan = plan_clash_patch(&patch, &current, &next)?;
        validate_mixed_port_change(&plan, &current)?;
        validate_external_controller_change(owner, &plan, &current, &next).await?;
        let reaches_runtime =
            chimera_core::state::mutation::impact::reaches_runtime(&current, &next, &[]);
        // The ref source owner can commit the allowed raw runtime inputs
        // without a second Core mutator. The patch plan owns the exact
        // one-shot mode interruption decision, carried with its Checked
        // request through durable CAS and final Core apply. Listener,
        // authorization, Service and host side effects stay on legacy paths.
        if allow_checked
            && reaches_runtime
            && checked_raw_clash_patch_eligible(
                &patch,
                &owner.get_app_config()?,
                crate::core::service::ipc::get_ipc_state().is_connected(),
            )
            && owner.inner.checked_gate.is_some()
        {
            let interrupt_mode = plan.mode_changed && plan.break_on_mode_change;
            let report = self
                .commit_native_candidate(before, next, true, interrupt_mode)
                .await?;
            // The source owner has already sent its exact Checked commit
            // receipt, so do not send legacy SourceCommitted for this version.
            return Ok(report);
        }
        let mode_changed = plan.mode_changed;
        let break_on_mode_change = plan.break_on_mode_change;
        let expected_sources = owner.committed_runtime_source_versions();
        let report = self
            .commit_candidate_with(
                before,
                next,
                reaches_runtime,
                |candidate| owner.check_clash_runtime_candidate(candidate),
                || {
                    super::reconciler::committed::apply_with_dependency_fence(
                        super::reconciler::committed::SourceDomain::Clash,
                        expected_sources.clone(),
                        || async { owner.committed_runtime_source_versions() },
                        || async {
                            // RuntimeBuilder consumes committed typed Clash;
                            // the sole Core owner publishes on confirmed Apply.
                            update_core_config(owner).await?;
                            // A different source may have advanced during
                            // Apply. Do not interrupt the old connection for
                            // a mode change that no longer owns the target.
                            expected_sources.ensure_dependencies_current(
                                super::reconciler::committed::SourceDomain::Clash,
                                &owner.committed_runtime_source_versions(),
                            )?;
                            if mode_changed {
                                let api = owner.clash_api_client().await?;
                                log_err!(
                                    crate::core::connection_interruption::ConnectionInterruptionService::on_mode_change(
                                        &api,
                                        break_on_mode_change,
                                    )
                                    .await,
                                    "failed to interrupt connections after mode change"
                                );
                            }
                            Ok(())
                        },
                    )
                },
                || {
                    // RuntimeLifecycle publishes the confirmed runtime product.
                    // These files are compatibility mirrors, not the
                    // authoritative source; failures cannot roll it back.
                    Config::clash().data().save_config()?;
                    Config::verge().data().save_file()?;
                    Ok(())
                },
            )
            .await?;

        // Raw, override and running-override mutations share one source
        // notification; deferred retries are admitted by CoreLifecycle's
        // existing coalesced mailbox, not by a second runtime writer.
        owner.inner.reconciler().source_committed(
            super::reconciler::committed::SourceDomain::Clash,
            report.snapshot.version,
            report
                .degradations
                .iter()
                .any(|item| item.code == "clash_runtime_deferred" && item.retryable),
        );
        Ok(report)
    }
}

impl ChimeraClient {
    /// Ref CandidateCheck: freeze the currently committed Profile document and
    /// all managed-file content while the candidate Clash document is supplied
    /// independently. The lower Core validator is read-only and does not
    /// acquire the workflow's mutating Core/Service ownership.
    pub(super) async fn check_clash_runtime_candidate(&self, candidate: ClashConfig) -> Result<()> {
        if self.inner.shared.typed_profiles().is_none() {
            // Compatibility-only fixtures instantiate no Profiles source.
            #[cfg(test)]
            return Ok(());
            #[cfg(not(test))]
            bail!("runtime candidate check requires the typed Profiles source");
        }
        use crate::state::mutation::{DomainChange, RuntimeCandidate};

        let proposed = RuntimeCandidate {
            change: DomainChange::Clash {
                candidate: Arc::new(candidate),
            },
            staged: Default::default(),
        };
        if let Some(gate) = &self.inner.checked_gate {
            // Share the production Profiles gate's upstream builder, source
            // snapshots and Core validator. Clash still has an inline legacy
            // apply, so never queue Checked bytes for a commit it cannot
            // deliver through CheckedSourceCommitted yet.
            return gate
                .validate_only(proposed)
                .await
                .map_err(anyhow::Error::new);
        }

        // The upstream RuntimeGate is mandatory in production. An absent
        // gate must never silently fall back to an independently constructed
        // CoreLifecycle validator with a different runtime/port session.
        #[cfg(not(test))]
        bail!("Clash candidate validation requires the installed Core-owned RuntimeGate");

        // Isolated legacy fixtures have no canonical Core runtime session.
        #[cfg(test)]
        {
            use crate::state::mutation::RuntimeGate as _;

            let profiles = self
                .inner
                .shared
                .typed_profiles()
                .as_ref()
                .expect("checked above");
            let checker = super::reconciler::legacy_check::CandidateCheck::new(
                self.inner.shared.core().clone(),
                self.inner.shared.application().clone(),
                self.inner.clash_config.clone(),
                profiles.content_reader(),
                profiles.snapshot_handle(),
                self.inner.reconciler().clone(),
            );
            checker.check(proposed).await.map_err(anyhow::Error::new)
        }
    }

    pub(crate) async fn patch_clash(&self, patch: Mapping) -> Result<()> {
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        let _system_proxy_mutation = if patch_changes_mixed_port(&patch) {
            Some(self.lock_system_proxy_mutation().await)
        } else {
            None
        };
        let report = self.inner.clash_config.patch(self, patch).await?;
        for degradation in &report.degradations {
            tracing::warn!(
                code = %degradation.code,
                message = %degradation.message,
                "Clash patch committed with degraded follow-up"
            );
        }
        self.refresh_stream_log_level().await;
        Ok(())
    }

    pub(crate) async fn patch_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> Result<()> {
        let report = self
            .inner
            .clash_config
            .patch_overrides(self, overrides, true)
            .await?;
        for degradation in &report.degradations {
            tracing::warn!(
                code = %degradation.code,
                message = %degradation.message,
                "Clash overrides committed with degraded follow-up"
            );
        }
        self.refresh_stream_log_level().await;
        Ok(())
    }

    pub(crate) async fn patch_running_clash_overrides(
        &self,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        let outcome = self
            .inner
            .clash_config
            .patch_running_overrides(self, overrides)
            .await;
        self.refresh_stream_log_level().await;
        outcome
    }

    async fn refresh_stream_log_level(&self) {
        match self.get_clash_config() {
            Ok(config) => {
                if let Err(error) = self
                    .inner
                    .shared
                    .streams()
                    .set_log_level(config.overrides.log_level())
                    .await
                {
                    tracing::warn!(%error, "failed to update the Clash logs WebSocket level");
                }
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read Clash log level after config update");
            }
        }
    }
}

fn validate_mixed_port_change(plan: &ClashPatchPlan, current: &ClashConfig) -> Result<()> {
    if plan.mixed_port_changed
        && current.mixed_port.kind != PortStrategyKind::Random
        && let Some(port) = plan.mixed_port
        && !port_scanner::local_port_available(port)
    {
        bail!("port already in use");
    }

    Ok(())
}

async fn validate_external_controller_change(
    client: &ChimeraClient,
    plan: &ClashPatchPlan,
    current: &ClashConfig,
    next: &ClashConfig,
) -> Result<()> {
    if !plan.external_controller_changed {
        return Ok(());
    }

    let strategy = PortStrategy {
        kind: current.external_controller.port.kind.clone(),
        start_port: next.external_controller.port.start_port,
    };
    let core_state = client.core_status().await?;

    if matches!(&core_state.state, CoreState::Running) && strategy.pick_and_try_port().is_err() {
        bail!("can not select fixed: current port is not available.");
    }

    Ok(())
}

async fn update_core_config(client: &ChimeraClient) -> Result<()> {
    match client.rebuild_running_config().await {
        Ok(_) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Ok(())));
            Ok(())
        }
        Err(err) => {
            handle::Handle::notice_message(&handle::Message::SetConfig(Err(format!("{err:?}"))));
            Err(err)
        }
    }
}

use chimera_core::client::clash_operations::{
    ClashCommitReport, ClashPatchPlan, apply_mapping_to_typed_clash, checked_clash_patch_eligible,
    checked_raw_clash_patch_eligible, patch_changes_mixed_port, plan_clash_patch,
    should_reconcile_clash_patch,
};
