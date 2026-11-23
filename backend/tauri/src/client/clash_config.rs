//! Clash desired-configuration client boundary.
//!
//! REF owns Clash desired state through a typed config client. Chimera still
//! persists the legacy Clash guard mapping plus typed runtime overrides, so
//! this transitional client keeps those storage semantics while centralizing
//! reads, validation, mutation, and running-core coordination here.

use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context as _, Result, bail};
use camino::Utf8PathBuf;
use chimera_config::clash::config::{
    ClashConfig, ClashConfigPatch,
    clash_strategy::{PortStrategy, PortStrategyKind},
    overrides::ClashGuardOverridesPatch,
};
use chimera_ipc::api::status::CoreState;
use chimera_state_engine::{PersistentStateManagerSetup, StateSnapshot};
use ractor::{Actor, ActorRef, RpcReplyPort, rpc::CallResult};
use serde_yaml::{Mapping, Value};
use struct_patch::Patch;

use crate::{
    config::{clash::ClashInfo, core::Config, runtime::ClashConfigOverrides},
    core::{clash::transaction::TransactionOutcome, handle},
    log_err,
    state::{
        ConditionalReplaceResult,
        clash_config::{
            ClashConfigActor, ClashConfigActorArgs, ClashConfigActorMessage, ClashConfigSnapshot,
        },
        mirror::{ClashLegacyBridge, PreparedTypedReplace},
    },
};

use super::{
    ChimeraClient,
    runtime::{Degradation, DegradationPhase},
};

const CLASH_CONFIG_READ_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct ClashConfigClient {
    state: Arc<ClashConfigStateBackend>,
}

enum ClashConfigStateBackend {
    Actor {
        actor_ref: ActorRef<ClashConfigActorMessage>,
        snapshot: StateSnapshot<ClashConfig>,
    },
    #[cfg(test)]
    Static {
        // The compatibility fixture must expose real monotonic versions or
        // the committed-version fence mistakes a successful write for a race.
        state: parking_lot::RwLock<(ClashConfig, u64)>,
    },
}

#[derive(Debug)]
pub(super) struct ClashCommitReport {
    pub(super) snapshot: ClashConfigSnapshot,
    pub(super) reaches_runtime: bool,
    /// Exact CAS version already notified through ref RuntimeGate::committed.
    /// The caller must not replay the legacy SourceCommitted on this version.
    pub(super) checked_version: Option<u64>,
    pub(super) degradations: Vec<Degradation>,
}

struct ClashPatchPlan {
    mixed_port: Option<u16>,
    mixed_port_changed: bool,
    external_controller_changed: bool,
    mode_changed: bool,
    break_on_mode_change: bool,
}

impl ClashConfigClient {
    #[cfg(test)]
    pub(crate) fn legacy() -> anyhow::Result<Self> {
        Self::legacy_with(ClashConfig::default())
    }

    #[cfg(test)]
    pub(crate) fn legacy_with(config: ClashConfig) -> anyhow::Result<Self> {
        Ok(Self::with_state(ClashConfigStateBackend::Static {
            state: parking_lot::RwLock::new((config, 0)),
        }))
    }

    pub(super) async fn new(
        config_path: Utf8PathBuf,
        seed: ClashConfig,
        bridge: Arc<dyn ClashLegacyBridge>,
    ) -> anyhow::Result<Self> {
        // Ref's typed client fails on malformed config; also keep existing
        // dangling links and inaccessible nodes from becoming default seeds.
        let should_load = crate::core::corrupt::config_node_exists(config_path.as_std_path())
            .with_context(|| format!("failed to inspect clash config {config_path}"))?;
        let setup = PersistentStateManagerSetup::<ClashConfig>::builder()
            .config_path(config_path)
            .assemble();
        let manager = if should_load {
            setup
                .load()
                .await
                .context("failed to load clash persistent state manager")?
        } else {
            setup.from_state(seed)
        };
        let snapshot = manager.snapshot_handle();
        let (actor_ref, _handle) = Actor::spawn(
            None,
            ClashConfigActor,
            ClashConfigActorArgs {
                manager,
                bridge: bridge.clone(),
            },
        )
        .await
        .context("failed to spawn clash config actor")?;

        let client = Self::with_state(ClashConfigStateBackend::Actor {
            actor_ref,
            snapshot,
        });
        bridge
            .prepare(&client.get_typed())
            .context("failed to prepare loaded clash legacy mirror")?
            .apply();
        Ok(client)
    }

    fn with_state(state: ClashConfigStateBackend) -> Self {
        Self {
            state: Arc::new(state),
        }
    }

    /// Read-only ref RuntimeGate source handle, shared with the exact
    /// committing actor. An isolated fixture has no backing persisted manager.
    pub(crate) fn committed_snapshot_handle(&self) -> anyhow::Result<StateSnapshot<ClashConfig>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => Ok(snapshot.clone()),
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => {
                anyhow::bail!("test-only Clash fixture has no persistent state handle")
            }
        }
    }

    /// Read the immutable committed config+version pair without a writer
    /// Actor RPC; a queued source mutation may be awaiting CoreLifecycle.
    pub(crate) fn committed_runtime_source(&self) -> (ClashConfig, u64) {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => {
                let observed = snapshot.load();
                (observed.state.clone(), *observed.version)
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => state.read().clone(),
        }
    }

    fn get_typed(&self) -> ClashConfig {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { snapshot, .. } => snapshot.load().state.clone(),
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => state.read().0.clone(),
        }
    }

    pub(crate) fn get(&self) -> Result<ClashConfig> {
        Ok(self.get_typed())
    }

    pub(super) async fn get_snapshot(&self) -> anyhow::Result<ClashConfigSnapshot> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { .. } => {
                self.call(
                    ClashConfigActorMessage::Get,
                    Some(CLASH_CONFIG_READ_TIMEOUT),
                )
                .await
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => {
                let source = state.read();
                Ok(ClashConfigSnapshot {
                    state: source.0.clone(),
                    version: source.1,
                    reaches_runtime: false,
                    operation_id: None,
                })
            }
        }
    }

    pub(super) async fn prepare_replace(
        &self,
        state: ClashConfig,
    ) -> anyhow::Result<PreparedTypedReplace<ClashConfig>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::PrepareReplace { state, reply },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => Ok(PreparedTypedReplace::new(
                state,
                Box::new(crate::state::mirror::NoopPreparedLegacyMirror),
            )),
        }
    }

    pub(super) async fn replace_prepared_if_version(
        &self,
        expected_version: u64,
        prepared: PreparedTypedReplace<ClashConfig>,
    ) -> anyhow::Result<ConditionalReplaceResult<ClashConfigSnapshot>> {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref
                    .call(
                        |reply| ClashConfigActorMessage::ReplacePreparedIfVersion {
                            expected_version,
                            prepared,
                            reply,
                        },
                        None,
                    )
                    .await?
                {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { state } => {
                let mut source = state.write();
                if source.1 != expected_version {
                    return Ok(ConditionalReplaceResult::Conflict {
                        actual_version: source.1,
                    });
                }
                let next_version = source
                    .1
                    .checked_add(1)
                    .context("test Clash version overflow")?;
                let (next, mirror) = prepared.into_parts();
                let reaches_runtime =
                    crate::state::mutation::impact::reaches_runtime(&source.0, &next, &[]);
                source.0 = next.clone();
                source.1 = next_version;
                let version = source.1;
                drop(source);
                mirror.apply();
                Ok(ConditionalReplaceResult::Replaced(ClashConfigSnapshot {
                    state: next,
                    version,
                    reaches_runtime,
                    operation_id: reaches_runtime.then(chimera_core_manager::OperationId::generate),
                }))
            }
        }
    }

    async fn call<F>(
        &self,
        make: F,
        timeout: Option<Duration>,
    ) -> anyhow::Result<ClashConfigSnapshot>
    where
        F: FnOnce(RpcReplyPort<anyhow::Result<ClashConfigSnapshot>>) -> ClashConfigActorMessage,
    {
        match self.state.as_ref() {
            ClashConfigStateBackend::Actor { actor_ref, .. } => {
                match actor_ref.call(make, timeout).await? {
                    CallResult::Success(result) => result,
                    CallResult::SenderError => anyhow::bail!("clash config actor reply dropped"),
                    CallResult::Timeout => anyhow::bail!("clash config actor call timed out"),
                }
            }
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => {
                anyhow::bail!("clash config actor is unavailable in the static test backend")
            }
        }
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
        if let Some(gate) = owner.inner.checked_gate.as_ref() {
            let app = owner.get_app_config()?;
            let service_connected = crate::core::service::ipc::get_ipc_state().is_connected();
            // The same committed source snapshot decides DNS/Hosts ownership
            // and the exact expected CAS version; do not re-read independently.
            let before = self.get_snapshot().await?;
            if checked_clash_patch_eligible(&patch, &before.state, &app, service_connected) {
                return self.patch_typed_config_checked(before, patch, gate).await;
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

    /// Ref source-owner cutover for local config-builder flags and managed
    /// DNS/Hosts. These inputs have no TUN/port/host/mode one-shot effects:
    /// the sole Core owner receives the checked product after durable CAS.
    async fn patch_typed_config_checked(
        &self,
        before: ClashConfigSnapshot,
        patch: ClashConfigPatch,
        gate: &Arc<super::reconciler::check::CandidateCheck>,
    ) -> Result<ClashCommitReport> {
        let mut next = before.state.clone();
        next.apply(patch);
        self.commit_checked_candidate(before, next, gate, false)
            .await
    }

    /// One durable Clash commit implementation for ref-style typed and raw
    /// source transitions. The caller must rule out host/port/controller
    /// effects and explicitly tag any mode-interruption one-shot intent.
    async fn commit_checked_candidate(
        &self,
        before: ClashConfigSnapshot,
        next: ClashConfig,
        gate: &Arc<super::reconciler::check::CandidateCheck>,
        interrupt_mode: bool,
    ) -> Result<ClashCommitReport> {
        use crate::state::mutation::{DomainChange, RuntimeCandidate};
        let prepared = self.prepare_replace(next.clone()).await?;
        let checked_request = gate
            .check_with_receipt_effect(
                RuntimeCandidate {
                    change: DomainChange::Clash {
                        candidate: Arc::new(next),
                    },
                    staged: Default::default(),
                },
                interrupt_mode,
            )
            .await
            .map_err(anyhow::Error::new)?;
        let snapshot = match self
            .replace_prepared_if_version(before.version, prepared)
            .await
        {
            Ok(ConditionalReplaceResult::Replaced(snapshot)) => snapshot,
            Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                gate.abort_with_receipt(
                    chimera_contracts::mutation::ConfigDomain::Clash,
                    checked_request.clone(),
                );
                bail!(
                    "clash config version conflict: expected {}, actual {actual_version}",
                    before.version,
                );
            }
            Err(error) => {
                gate.abort_with_receipt(
                    chimera_contracts::mutation::ConfigDomain::Clash,
                    checked_request.clone(),
                );
                return Err(error.context("checked Clash source CAS did not confirm a commit"));
            }
        };
        // The source is now durable. Notify the only Core owner before any
        // optional mirror write can fail; do not perform a legacy inline apply.
        gate.committed_with_receipt(
            chimera_contracts::mutation::ConfigDomain::Clash,
            chimera_core::state::Version::new(snapshot.version),
            checked_request,
        );
        let mut degradations = Vec::new();
        if let Err(error) = (|| {
            Config::clash().data().save_config()?;
            Config::verge().data().save_file()?;
            Ok::<(), anyhow::Error>(())
        })() {
            degradations.push(Degradation {
                phase: DegradationPhase::LegacyMirror,
                code: "legacy_clash_mirror_write_failed".into(),
                message: format!(
                    "Clash source was saved, but legacy mirror persistence failed: {error:#}"
                ),
                retryable: true,
            });
        }
        Ok(ClashCommitReport {
            checked_version: Some(snapshot.version),
            snapshot,
            reaches_runtime: true,
            degradations,
        })
    }

    async fn patch_typed_config_with<Check, CheckFuture, Apply, ApplyFuture, Save>(
        &self,
        patch: ClashConfigPatch,
        check: Check,
        apply: Apply,
        save_legacy: Save,
    ) -> Result<ClashCommitReport>
    where
        Check: FnOnce(ClashConfig) -> CheckFuture,
        CheckFuture: Future<Output = Result<()>>,
        Apply: FnOnce() -> ApplyFuture,
        ApplyFuture: Future<Output = Result<()>>,
        Save: FnOnce() -> Result<()>,
    {
        let before = self.get_snapshot().await?;
        let mut next = before.state.clone();
        next.apply(patch.clone());
        let reaches_runtime = should_reconcile_clash_patch(&before.state, &next, &patch);
        self.commit_candidate_with(before, next, reaches_runtime, check, apply, save_legacy)
            .await
    }

    async fn commit_candidate_with<Check, CheckFuture, Apply, ApplyFuture, Save>(
        &self,
        before: ClashConfigSnapshot,
        next: ClashConfig,
        reaches_runtime: bool,
        check: Check,
        apply: Apply,
        save_legacy: Save,
    ) -> Result<ClashCommitReport>
    where
        Check: FnOnce(ClashConfig) -> CheckFuture,
        CheckFuture: Future<Output = Result<()>>,
        Apply: FnOnce() -> ApplyFuture,
        ApplyFuture: Future<Output = Result<()>>,
        Save: FnOnce() -> Result<()>,
    {
        let prepared = self.prepare_replace(next.clone()).await?;
        if reaches_runtime {
            // No source file, running Core, or runtime product is mutated
            // until the independent candidate checker accepts this document.
            check(next).await?;
        }
        let snapshot = match self
            .replace_prepared_if_version(before.version, prepared)
            .await?
        {
            ConditionalReplaceResult::Replaced(snapshot) => snapshot,
            ConditionalReplaceResult::Conflict { actual_version } => bail!(
                "clash config version conflict: expected {}, actual {actual_version}",
                before.version,
            ),
        };

        debug_assert!(
            !snapshot.reaches_runtime || reaches_runtime,
            "a durable Clash Runtime input change must be validated before publication"
        );
        let mut degradations = Vec::new();
        if reaches_runtime {
            // The source is durable now. A newer source version prevents an
            // obsolete Core apply, and a concurrent advance during apply
            // remains deferred for the existing lifecycle owner to reconcile.
            if let Some(degradation) = super::reconciler::committed::apply_committed(
                "clash",
                snapshot.version,
                || async { self.get_snapshot().await.map(|latest| latest.version) },
                apply,
            )
            .await
            {
                degradations.push(degradation);
            }
        }
        if let Err(error) = save_legacy() {
            degradations.push(Degradation {
                phase: DegradationPhase::LegacyMirror,
                code: "legacy_clash_mirror_write_failed".into(),
                message: format!(
                    "Clash source was saved, but legacy mirror persistence failed: {error:#}"
                ),
                retryable: true,
            });
        }
        Ok(ClashCommitReport {
            snapshot,
            reaches_runtime,
            checked_version: None,
            degradations,
        })
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
        let reaches_runtime = crate::state::mutation::impact::reaches_runtime(&current, &next, &[]);
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
            && let Some(gate) = owner.inner.checked_gate.as_ref()
        {
            let interrupt_mode = plan.mode_changed && plan.break_on_mode_change;
            let report = self
                .commit_checked_candidate(before, next, gate, interrupt_mode)
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
        owner.inner.core_lifecycle.source_committed(
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

impl Drop for ClashConfigStateBackend {
    fn drop(&mut self) {
        match self {
            ClashConfigStateBackend::Actor { actor_ref, .. } => actor_ref.stop(None),
            #[cfg(test)]
            ClashConfigStateBackend::Static { .. } => {}
        }
    }
}

impl ChimeraClient {
    /// Read the three committed source versions without waiting on a writer
    /// RPC. Reused by both legacy Clash Apply entrypoints; no second runtime
    /// owner, port session or source snapshot store is created.
    fn committed_runtime_source_versions(&self) -> super::reconciler::committed::SourceVersions {
        super::reconciler::committed::SourceVersions::from_committed(
            &self.inner.application,
            &self.inner.clash_config,
            self.inner
                .typed_profiles
                .as_ref()
                .map(|p| *p.snapshot_handle().load().version),
        )
    }

    /// Ref CandidateCheck: freeze the currently committed Profile document and
    /// all managed-file content while the candidate Clash document is supplied
    /// independently. The lower Core validator is read-only and does not
    /// acquire the workflow's mutating Core/Service ownership.
    pub(super) async fn check_clash_runtime_candidate(&self, candidate: ClashConfig) -> Result<()> {
        if self.inner.typed_profiles.is_none() {
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

            let profiles = self.inner.typed_profiles.as_ref().expect("checked above");
            let checker = super::reconciler::legacy_check::CandidateCheck::new(
                self.inner.core.clone(),
                self.inner.application.clone(),
                self.inner.clash_config.clone(),
                profiles.content_reader(),
                profiles.snapshot_handle(),
                self.inner.core_lifecycle.clone(),
            );
            checker.check(proposed).await.map_err(anyhow::Error::new)
        }
    }

    pub(crate) fn get_clash_config(&self) -> Result<ClashConfig> {
        self.inner.clash_config.get()
    }

    pub(crate) fn clash_info(&self) -> ClashInfo {
        self.inner.core.effective_clash_info()
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
                    .streams
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

fn apply_mapping_to_typed_clash(current: &ClashConfig, patch: &Mapping) -> Result<ClashConfig> {
    let mut next = current.clone();

    let mut override_mapping = Mapping::new();
    for key in [
        "allow-lan",
        "ipv6",
        "log-level",
        "mode",
        "secret",
        "unified-delay",
        "tcp-concurrent",
    ] {
        let key_value = Value::String(key.to_string());
        if let Some(value) = patch.get(&key_value) {
            override_mapping.insert(key_value, value.clone());
        }
    }
    let override_patch: ClashGuardOverridesPatch =
        serde_yaml::from_value(Value::Mapping(override_mapping))
            .context("invalid typed Clash override patch")?;
    next.overrides.apply(override_patch);

    if let Some(value) = get_non_null_patch_value(patch, "mixed-port") {
        let port = value
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("mixed-port must be an integer"))?;
        next.mixed_port.start_port =
            u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port"))?;
    }

    if let Some(value) = get_non_null_patch_value(patch, "external-controller") {
        let raw = value
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("external-controller must be a string"))?;
        let normalized = if raw.starts_with(':') {
            format!("127.0.0.1{raw}")
        } else {
            raw.to_string()
        };
        let controller: SocketAddr = normalized
            .parse()
            .context("external-controller must be an IP host:port")?;
        next.external_controller.host = controller.ip();
        next.external_controller.port.start_port = controller.port();
    }

    Ok(next)
}

fn get_non_null_patch_value<'a>(patch: &'a Mapping, key: &str) -> Option<&'a serde_yaml::Value> {
    patch.get(key).filter(|value| !value.is_null())
}

fn patch_changes_mixed_port(patch: &Mapping) -> bool {
    get_non_null_patch_value(patch, "mixed-port").is_some()
}

fn plan_clash_patch(
    patch: &Mapping,
    current: &ClashConfig,
    next: &ClashConfig,
) -> Result<ClashPatchPlan> {
    let mixed_port = get_non_null_patch_value(patch, "mixed-port").and_then(|value| value.as_u64());
    let mixed_port = mixed_port
        .map(|port| u16::try_from(port).map_err(|_| anyhow::anyhow!("invalid mixed-port")))
        .transpose()?;
    let mixed_port_changed = mixed_port
        .map(|port| port != current.mixed_port.start_port)
        .unwrap_or(false);

    let external_controller_requested =
        get_non_null_patch_value(patch, "external-controller").is_some();
    let external_controller_changed = external_controller_requested
        && (current.external_controller.host != next.external_controller.host
            || current.external_controller.port.start_port
                != next.external_controller.port.start_port);

    Ok(ClashPatchPlan {
        mixed_port,
        mixed_port_changed,
        external_controller_changed,
        mode_changed: current.overrides.mode() != next.overrides.mode(),
        break_on_mode_change: current.break_connection.on_mode_change,
    })
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

/// Ref CheckedSourceCommit for local Core-rendered inputs: builder flags
/// and DNS/Hosts ownership. These do not require a separate OS operation.
/// Everything else (ports, TUN, IPC controller, host handoff, mode effects)
/// stays with its existing source-specific workflow.
fn checked_clash_patch_eligible(
    patch: &ClashConfigPatch,
    current: &ClashConfig,
    app: &chimera_config::application::ChimeraAppConfig,
    service_connected: bool,
) -> bool {
    if app.enable_service_mode || service_connected {
        return false;
    }
    let requested = patch.enable_clash_fields.is_some()
        || patch.expand_include_all.is_some()
        || patch.dns_control.is_some()
        || patch.hosts_control.is_some()
        || (patch.dns.is_some() && current.managed_dns().is_some())
        || (patch.hosts.is_some() && current.managed_hosts().is_some());
    if !requested {
        return false;
    }

    // A table is only a checked runtime input when the source already owns
    // it or this patch explicitly changes ownership. Pure profile-owned DNS
    // or Hosts drafts keep their existing document-only semantics instead.
    if patch.dns.is_some() && current.managed_dns().is_none() && patch.dns_control.is_none() {
        return false;
    }
    if patch.hosts.is_some() && current.managed_hosts().is_none() && patch.hosts_control.is_none() {
        return false;
    }

    // Fail closed on every unlisted (including future) typed patch field.
    // Treat even explicit same-value requests as intent, not as no-ops.
    let mut remainder = patch.clone();
    remainder.enable_clash_fields = None;
    remainder.expand_include_all = None;
    remainder.dns_control = None;
    remainder.dns = None;
    remainder.hosts_control = None;
    remainder.hosts = None;
    match (
        serde_yaml::to_value(&remainder),
        serde_yaml::to_value(ClashConfig::new_empty_patch()),
    ) {
        (Ok(remainder), Ok(empty)) => remainder == empty,
        _ => false,
    }
}

/// Raw and ordinary (non-running) override callers share the ref checked
/// Source Gate only for fields with no host/listener/controller effects.
/// An explicit mode change is allowed because the calling source owner
/// attaches its exact connection interruption intent to the Checked receipt.
fn checked_raw_clash_patch_eligible(
    patch: &Mapping,
    app: &chimera_config::application::ChimeraAppConfig,
    service_connected: bool,
) -> bool {
    if app.enable_service_mode || service_connected || patch.is_empty() {
        return false;
    }
    patch.keys().all(|key| {
        matches!(
            key.as_str(),
            Some("mode" | "allow-lan" | "ipv6" | "log-level" | "unified-delay" | "tcp-concurrent")
        )
    })
}

fn should_reconcile_clash_patch(
    before: &ClashConfig,
    after: &ClashConfig,
    patch: &ClashConfigPatch,
) -> bool {
    // ref source-impact projection decides whether built runtime inputs moved.
    // Preserve the legacy explicit-request intent when an unchanged runtime
    // field is resubmitted to retry an outstanding desired configuration.
    crate::state::mutation::impact::reaches_runtime(before, after, &[])
        || crate::client::application_workflow::impact::RequestedRuntimeFields::of_clash(patch)
            .names_any()
        || (patch.dns.is_some() && after.managed_dns().is_some())
        || (patch.hosts.is_some() && after.managed_hosts().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use std::sync::{Arc, Mutex};

    #[test]
    fn checked_clash_route_admits_only_local_builder_flags() {
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = false;
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_clash_fields = Some(true);
        assert!(checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            true
        ));
        app.enable_service_mode = true;
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        app.enable_service_mode = false;

        patch.expand_include_all = Some(false);
        assert!(checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        patch.enable_clash_fields = None;
        assert!(checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        patch.expand_include_all = None;
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));

        patch.enable_clash_fields = Some(true);
        patch.enable_tun_mode = Some(false);
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        patch.enable_tun_mode = None;
        patch.web_ui_list = Some(Vec::new());
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
        patch.web_ui_list = None;
        patch.mixed_port.start_port = Some(chimera_config::clash::config::DEFAULT_MIXED_PORT);
        assert!(!checked_clash_patch_eligible(
            &patch,
            &ClashConfig::default(),
            &app,
            false
        ));
    }

    #[test]
    fn checked_typed_dns_hosts_require_explicit_runtime_ownership() {
        use chimera_config::clash::config::dns::ControlSource;

        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = false;
        let mut before = ClashConfig::default();
        let mut patch = ClashConfig::new_empty_patch();

        // Editing a stored DNS draft while the profile still owns the
        // runtime must not turn a document-only update into a Core apply.
        patch.dns = Some(before.dns.clone());
        assert!(!checked_clash_patch_eligible(&patch, &before, &app, false));

        // Explicitly taking control with the same request is safe: the
        // resulting bytes are built once and admitted by the sole Core owner.
        patch.dns_control = Some(ControlSource::Chimera);
        assert!(checked_clash_patch_eligible(&patch, &before, &app, false));

        patch.hosts = Some(indexmap::IndexMap::new());
        assert!(!checked_clash_patch_eligible(&patch, &before, &app, false));
        patch.hosts_control = Some(ControlSource::Chimera);
        assert!(checked_clash_patch_eligible(&patch, &before, &app, false));

        assert!(!checked_clash_patch_eligible(&patch, &before, &app, true));
        app.enable_service_mode = true;
        assert!(!checked_clash_patch_eligible(&patch, &before, &app, false));
        app.enable_service_mode = false;

        // Listener, TUN and other side effects stay outside this source path,
        // even when mixed into the same DNS/Hosts patch.
        patch.enable_tun_mode = Some(false);
        assert!(!checked_clash_patch_eligible(&patch, &before, &app, false));
        patch.enable_tun_mode = None;
        patch.mixed_port.start_port = Some(7890);
        assert!(!checked_clash_patch_eligible(&patch, &before, &app, false));
        patch.mixed_port.start_port = None;

        // Once an owner is already selected, changing its table alone is a
        // runtime input, including an explicit same-value re-submission.
        before.dns_control = ControlSource::Chimera;
        patch.dns_control = None;
        patch.hosts_control = None;
        patch.hosts = None;
        assert!(checked_clash_patch_eligible(&patch, &before, &app, false));

        // Relinquishing ownership also changes the runtime DNS source.
        patch.dns_control = Some(ControlSource::Profile);
        assert!(checked_clash_patch_eligible(&patch, &before, &app, false));
    }

    #[test]
    fn raw_override_checked_route_admits_only_local_runtime_inputs() {
        let mut app = chimera_config::application::ChimeraAppConfig::default();
        app.enable_service_mode = false;
        let mut patch = Mapping::new();
        for (key, value) in [
            ("allow-lan", Value::Bool(true)),
            ("ipv6", Value::Bool(false)),
            ("log-level", Value::String("debug".into())),
            ("unified-delay", Value::Bool(false)),
            ("tcp-concurrent", Value::Bool(false)),
        ] {
            patch.insert(Value::String(key.into()), value);
        }
        let before = ClashConfig::default();
        let next = apply_mapping_to_typed_clash(&before, &patch).unwrap();
        assert!(checked_raw_clash_patch_eligible(&patch, &app, false));
        assert!(crate::state::mutation::impact::reaches_runtime(
            &before,
            &next,
            &[]
        ));
        assert!(!checked_raw_clash_patch_eligible(&patch, &app, true));
        app.enable_service_mode = true;
        assert!(!checked_raw_clash_patch_eligible(&patch, &app, false));
        app.enable_service_mode = false;

        for (key, value) in [
            ("mixed-port", Value::Number(7890u16.into())),
            (
                "external-controller",
                Value::String("127.0.0.1:9090".into()),
            ),
            ("secret", Value::String("new-control-secret".into())),
            ("unknown-option", Value::Bool(true)),
        ] {
            let mut with_effect = patch.clone();
            with_effect.insert(Value::String(key.into()), value);
            assert!(
                !checked_raw_clash_patch_eligible(&with_effect, &app, false),
                "explicit {key} must retain legacy effects and preflight"
            );
        }
        let mut non_string_key = patch.clone();
        non_string_key.insert(Value::Bool(true), Value::Bool(true));
        assert!(!checked_raw_clash_patch_eligible(
            &non_string_key,
            &app,
            false
        ));
        assert!(!checked_raw_clash_patch_eligible(
            &Mapping::new(),
            &app,
            false
        ));
    }

    #[test]
    fn mode_interruption_intent_follows_source_patch_plan_not_applied_selection() {
        let app = chimera_config::application::ChimeraAppConfig {
            enable_service_mode: false,
            ..Default::default()
        };
        let mut current = ClashConfig::default();
        current.break_connection.on_mode_change = true;
        let mut mode_request = Mapping::new();
        mode_request.insert("mode".into(), "rule".into());
        let same = apply_mapping_to_typed_clash(&current, &mode_request).unwrap();
        let same_plan = plan_clash_patch(&mode_request, &current, &same).unwrap();
        assert!(!same_plan.mode_changed);
        assert!(
            !same_plan.mode_changed
                && same_plan.break_on_mode_change == current.break_connection.on_mode_change
        );
        assert!(checked_raw_clash_patch_eligible(&mode_request, &app, false));
        assert!(!crate::state::mutation::impact::reaches_runtime(
            &current,
            &same,
            &[]
        ));
        mode_request.insert("ipv6".into(), Value::Bool(false));
        assert!(checked_raw_clash_patch_eligible(&mode_request, &app, false));

        mode_request.insert("mode".into(), "global".into());
        let changed = apply_mapping_to_typed_clash(&current, &mode_request).unwrap();
        let changed_plan = plan_clash_patch(&mode_request, &current, &changed).unwrap();
        assert!(changed_plan.mode_changed && changed_plan.break_on_mode_change);
        assert!(checked_raw_clash_patch_eligible(&mode_request, &app, false));
        assert!(crate::state::mutation::impact::reaches_runtime(
            &current,
            &changed,
            &[]
        ));
        // This boolean is attached to the exact checked CAS receipt before
        // publishing the source. No inference from current applied_selection.
        let checked_interruption = changed_plan.mode_changed && changed_plan.break_on_mode_change;
        assert!(checked_interruption);
        current.break_connection.on_mode_change = false;
        let no_interruption = plan_clash_patch(&mode_request, &current, &changed).unwrap();
        assert!(no_interruption.mode_changed);
        assert!(!no_interruption.break_on_mode_change);
        assert!(checked_raw_clash_patch_eligible(&mode_request, &app, false));
    }

    struct RecordingBridge {
        mirrored_tun: Arc<Mutex<Option<bool>>>,
    }

    struct RecordingPreparedMirror {
        value: bool,
        target: Arc<Mutex<Option<bool>>>,
    }

    impl PreparedLegacyMirror for RecordingPreparedMirror {
        fn apply(self: Box<Self>) {
            *self.target.lock().unwrap() = Some(self.value);
        }
    }

    impl ClashLegacyBridge for RecordingBridge {
        fn prepare(&self, snap: &ClashConfig) -> anyhow::Result<Box<dyn PreparedLegacyMirror>> {
            Ok(Box::new(RecordingPreparedMirror {
                value: snap.enable_tun_mode,
                target: self.mirrored_tun.clone(),
            }))
        }

        fn snapshot_legacy(&self) -> anyhow::Result<ClashConfig> {
            Ok(ClashConfig::default())
        }
    }

    #[tokio::test]
    async fn existing_typed_file_wins_over_legacy_seed_on_startup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clash-config.yaml");
        let persisted = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        std::fs::write(&path, serde_yaml::to_string(&persisted).unwrap()).unwrap();

        let mirrored_tun = Arc::new(Mutex::new(None));
        let bridge: Arc<dyn ClashLegacyBridge> = Arc::new(RecordingBridge {
            mirrored_tun: mirrored_tun.clone(),
        });
        let path = Utf8PathBuf::from_path_buf(path).unwrap();
        let client = ClashConfigClient::new(path, ClashConfig::default(), bridge)
            .await
            .unwrap();

        assert!(client.get_typed().enable_tun_mode);
        assert_eq!(*mirrored_tun.lock().unwrap(), Some(true));
    }

    #[tokio::test]
    async fn malformed_clash_config_fails_without_overwriting_or_resetting() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clash-config.yaml");
        let bad = b"enable_tun_mode: [invalid";
        std::fs::write(&path, bad).unwrap();
        let mirrored_tun = Arc::new(Mutex::new(None));
        let result = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(path.clone()).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: mirrored_tun.clone(),
            }),
        )
        .await;
        let Err(error) = result else {
            panic!("invalid clash config must not load defaults");
        };
        assert!(format!("{error:#}").contains("failed to load clash"));
        assert_eq!(std::fs::read(&path).unwrap(), bad);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert_eq!(*mirrored_tun.lock().unwrap(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dangling_clash_link_fails_without_using_default_config() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("clash-config.yaml");
        let target = dir.path().join("missing.yaml");
        symlink(&target, &path).unwrap();
        let mirrored_tun = Arc::new(Mutex::new(None));
        let result = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(path.clone()).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: mirrored_tun.clone(),
            }),
        )
        .await;
        let Err(error) = result else {
            panic!("dangling clash config link must be reported");
        };
        assert!(format!("{error:#}").contains("failed to load clash"));
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        assert!(!target.exists());
        assert_eq!(*mirrored_tun.lock().unwrap(), None);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn valid_clash_link_preserves_existing_config_behavior() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real-clash.yaml");
        let path = dir.path().join("clash-config.yaml");
        let config = ClashConfig {
            enable_tun_mode: true,
            ..ClashConfig::default()
        };
        let original = serde_yaml::to_string(&config).unwrap();
        std::fs::write(&target, &original).unwrap();
        symlink(&target, &path).unwrap();
        let mirrored_tun = Arc::new(Mutex::new(None));
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(path.clone()).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: mirrored_tun.clone(),
            }),
        )
        .await
        .unwrap();
        assert!(client.get_typed().enable_tun_mode);
        assert_eq!(*mirrored_tun.lock().unwrap(), Some(true));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), original);
        assert_eq!(std::fs::read_link(path).unwrap(), target);
    }

    #[test]
    fn raw_mapping_projects_into_typed_saved_state() {
        let current = ClashConfig::default();
        let mut patch = Mapping::new();
        patch.insert("ipv6".into(), true.into());
        patch.insert("mixed-port".into(), 17890.into());
        patch.insert("external-controller".into(), "127.0.0.1:19090".into());

        let next = apply_mapping_to_typed_clash(&current, &patch).unwrap();
        let overrides = serde_yaml::to_value(&next.overrides).unwrap();
        let ipv6 = overrides
            .as_mapping()
            .and_then(|mapping| mapping.get("ipv6"))
            .and_then(Value::as_bool);

        assert_eq!(ipv6, Some(true));
        assert_eq!(next.mixed_port.start_port, 17890);
        assert_eq!(next.external_controller.port.start_port, 19090);
    }

    #[test]
    fn raw_mapping_plan_detects_actual_mode_changes_not_just_field_presence() {
        let current = ClashConfig::default();
        let mut patch = Mapping::new();
        patch.insert("mode".into(), "global".into());
        let next = apply_mapping_to_typed_clash(&current, &patch).unwrap();
        let changed = plan_clash_patch(&patch, &current, &next).unwrap();
        assert!(changed.mode_changed);

        patch.insert("mode".into(), "rule".into());
        let unchanged_next = apply_mapping_to_typed_clash(&current, &patch).unwrap();
        let unchanged = plan_clash_patch(&patch, &current, &unchanged_next).unwrap();
        assert!(!unchanged.mode_changed);
    }

    #[test]
    fn raw_mixed_port_patch_is_guarded_but_absent_or_null_is_not() {
        let mut patch = Mapping::new();
        assert!(!patch_changes_mixed_port(&patch));

        patch.insert("mixed-port".into(), Value::Null);
        assert!(!patch_changes_mixed_port(&patch));

        patch.insert("mixed-port".into(), 17890.into());
        assert!(patch_changes_mixed_port(&patch));
    }

    #[test]
    fn typed_clash_patch_reconciles_runtime_inputs_and_skips_ui_only_fields() {
        let current = ClashConfig::default();

        let mut ui_patch = ClashConfig::new_empty_patch();
        ui_patch.web_ui_list = Some(vec!["https://example.test".into()]);
        let mut ui_only = current.clone();
        ui_only.apply(ui_patch.clone());
        assert!(!should_reconcile_clash_patch(&current, &ui_only, &ui_patch));

        let mut tun_patch = ClashConfig::new_empty_patch();
        tun_patch.enable_tun_mode = Some(current.enable_tun_mode);
        assert!(should_reconcile_clash_patch(&current, &current, &tun_patch));

        let mut tun_auto_route_patch = ClashConfig::new_empty_patch();
        tun_auto_route_patch.tun.auto_route = Some(current.tun.auto_route);
        assert!(should_reconcile_clash_patch(
            &current,
            &current,
            &tun_auto_route_patch
        ));

        let mut runtime_patch = ClashConfig::new_empty_patch();
        let mut expansion_patch = ClashConfig::new_empty_patch();
        expansion_patch.expand_include_all = Some(false);
        let mut expansion_changed = current.clone();
        expansion_changed.apply(expansion_patch.clone());
        assert!(should_reconcile_clash_patch(
            &current,
            &expansion_changed,
            &expansion_patch
        ));

        runtime_patch.enable_clash_fields = Some(!current.enable_clash_fields);
        let mut runtime_changed = current.clone();
        runtime_changed.apply(runtime_patch.clone());
        assert!(should_reconcile_clash_patch(
            &current,
            &runtime_changed,
            &runtime_patch
        ));
    }

    #[tokio::test]
    async fn profile_owned_dns_draft_persists_without_core_check_or_apply() {
        use chimera_config::clash::config::dns::OneOrMany;
        use indexmap::IndexMap;

        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let client = ClashConfigClient::new(
            path.clone(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();

        let before = client.get_snapshot().await.unwrap();
        let mut dns = before.state.dns.clone();
        dns.nameserver = vec!["https://dns.example.test/dns-query".into()];
        let hosts = IndexMap::from([("internal.test".into(), OneOrMany::One("192.0.2.20".into()))]);
        let mut patch = ClashConfig::new_empty_patch();
        patch.dns = Some(dns.clone());
        patch.hosts = Some(hosts.clone());

        let report = client
            .patch_typed_config_with(
                patch,
                |_candidate| async {
                    panic!("an inactive DNS draft must not request a core candidate check");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || async {
                    panic!("an inactive DNS draft must not restart the core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || Ok(()),
            )
            .await
            .unwrap();

        assert!(!report.reaches_runtime);
        assert_eq!(report.snapshot.version, before.version + 1);
        assert_eq!(report.snapshot.state.dns_control, before.state.dns_control);
        assert_eq!(
            report.snapshot.state.hosts_control,
            before.state.hosts_control
        );
        assert_eq!(report.snapshot.state.dns, dns);
        assert_eq!(report.snapshot.state.hosts, hosts);

        let persisted: ClashConfig =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.dns, dns);
        assert_eq!(persisted.hosts, hosts);
    }

    #[tokio::test]
    async fn dns_takeover_checks_full_candidate_before_atomic_source_commit() {
        use chimera_config::clash::config::dns::{ControlSource, OneOrMany};
        use indexmap::IndexMap;

        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let client = ClashConfigClient::new(
            path.clone(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();

        let before = client.get_snapshot().await.unwrap();
        let mut dns = before.state.dns.clone();
        dns.nameserver = vec!["https://one.one.one.one/dns-query".into()];
        let hosts = IndexMap::from([("new.test".into(), OneOrMany::One("192.0.2.123".into()))]);
        let mut patch = ClashConfig::new_empty_patch();
        patch.dns = Some(dns.clone());
        patch.hosts = Some(hosts.clone());
        patch.dns_control = Some(ControlSource::Chimera);
        patch.hosts_control = Some(ControlSource::Chimera);

        // A rejection must leave both tables and both control selectors
        // unchanged, rather than publishing half of the requested takeover.
        let rejected_original = before.state.clone();
        let error = client
            .patch_typed_config_with(
                patch.clone(),
                move |candidate| {
                    let original = rejected_original.clone();
                    async move {
                        assert_eq!(candidate.dns_control, ControlSource::Chimera);
                        assert_eq!(candidate.hosts_control, ControlSource::Chimera);
                        assert_ne!(candidate.dns, original.dns);
                        assert_ne!(candidate.hosts, original.hosts);
                        anyhow::bail!("injected DNS candidate validation failure")
                    }
                },
                || async {
                    panic!("the rejected DNS candidate must never touch the running core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || Ok(()),
            )
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("injected DNS candidate validation failure"));
        let unchanged = client.get_snapshot().await.unwrap();
        assert_eq!(unchanged.version, before.version);
        assert_eq!(unchanged.state.dns_control, ControlSource::Profile);
        assert_eq!(unchanged.state.hosts_control, ControlSource::Profile);
        assert_eq!(unchanged.state.dns, before.state.dns);
        assert_eq!(unchanged.state.hosts, before.state.hosts);

        let preflight = client.clone();
        let applying = client.clone();
        let report = client
            .patch_typed_config_with(
                patch,
                move |candidate| async move {
                    assert_eq!(preflight.get_typed().dns_control, ControlSource::Profile);
                    assert_eq!(candidate.dns_control, ControlSource::Chimera);
                    assert_eq!(candidate.hosts_control, ControlSource::Chimera);
                    assert_eq!(
                        candidate.dns.nameserver[0],
                        "https://one.one.one.one/dns-query"
                    );
                    assert_eq!(
                        candidate.hosts["new.test"],
                        OneOrMany::One("192.0.2.123".into())
                    );
                    Ok(())
                },
                move || async move {
                    let committed = applying.get_typed();
                    assert_eq!(committed.dns_control, ControlSource::Chimera);
                    assert_eq!(committed.hosts_control, ControlSource::Chimera);
                    assert_eq!(
                        committed.dns.nameserver[0],
                        "https://one.one.one.one/dns-query"
                    );
                    assert!(committed.hosts.contains_key("new.test"));
                    Ok(())
                },
                || Ok(()),
            )
            .await
            .unwrap();
        assert!(report.reaches_runtime);
        assert_eq!(report.snapshot.version, before.version + 1);
        let persisted: ClashConfig =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(persisted.dns_control, ControlSource::Chimera);
        assert_eq!(persisted.hosts_control, ControlSource::Chimera);
        assert_eq!(persisted.dns, dns);
        assert_eq!(persisted.hosts, hosts);
    }

    #[tokio::test]
    async fn typed_clash_patch_checks_before_commit_and_preserves_source_on_mirror_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let mirrored_tun = Arc::new(Mutex::new(None));
        let client = ClashConfigClient::new(
            path.clone(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: mirrored_tun.clone(),
            }),
        )
        .await
        .unwrap();
        let observations = Arc::new(Mutex::new(Vec::new()));
        let checking = client.clone();
        let check_events = observations.clone();
        let applying = client.clone();
        let apply_events = observations.clone();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);

        let report = client
            .patch_typed_config_with(
                patch,
                move |candidate| async move {
                    check_events.lock().unwrap().push((
                        "check",
                        candidate.enable_tun_mode,
                        checking.get_typed().enable_tun_mode,
                    ));
                    Ok(())
                },
                move || async move {
                    apply_events.lock().unwrap().push((
                        "apply",
                        true,
                        applying.get_typed().enable_tun_mode,
                    ));
                    Ok(())
                },
                || anyhow::bail!("injected legacy persistence failure"),
            )
            .await
            .unwrap();

        assert_eq!(
            *observations.lock().unwrap(),
            [("check", true, false), ("apply", true, true)]
        );
        assert!(report.reaches_runtime);
        assert_eq!(report.degradations.len(), 1);
        assert_eq!(
            report.degradations[0].code,
            "legacy_clash_mirror_write_failed"
        );
        assert!(client.get_typed().enable_tun_mode);
        assert_eq!(*mirrored_tun.lock().unwrap(), Some(true));
        let saved: ClashConfig = serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(saved.enable_tun_mode);
        assert_eq!(
            serde_yaml::to_value(&report.snapshot.state).unwrap(),
            serde_yaml::to_value(&saved).unwrap()
        );
    }

    #[tokio::test]
    async fn rejected_clash_candidate_keeps_source_and_never_applies_core() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let mirrored_tun = Arc::new(Mutex::new(None));
        let client = ClashConfigClient::new(
            path.clone(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: mirrored_tun.clone(),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let error = client
            .patch_typed_config_with(
                patch,
                |candidate| async move {
                    assert!(candidate.enable_tun_mode);
                    anyhow::bail!("injected candidate rejection")
                },
                || async {
                    panic!("Core must not be applied for rejected candidate");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || panic!("no postcommit mirror write if candidate was rejected"),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("injected candidate rejection"));
        let after = client.get_snapshot().await.unwrap();
        assert_eq!(after.version, before.version);
        assert_eq!(
            serde_yaml::to_value(&after.state).unwrap(),
            serde_yaml::to_value(&before.state).unwrap()
        );
        assert_eq!(*mirrored_tun.lock().unwrap(), Some(false));
    }

    #[tokio::test]
    async fn postcommit_core_failure_is_deferred_and_does_not_roll_back_source() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let client = ClashConfigClient::new(
            path.clone(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let report = client
            .patch_typed_config_with(
                patch,
                |_candidate| async { Ok(()) },
                || async { anyhow::bail!("injected Core refusal after commit") },
                || Ok(()),
            )
            .await
            .unwrap();
        assert!(report.snapshot.state.enable_tun_mode);
        assert!(client.get_typed().enable_tun_mode);
        assert!(report.degradations.iter().any(|d| {
            d.code == "clash_runtime_deferred" && d.message.contains("injected Core refusal")
        }));
        let persisted: ClashConfig =
            serde_yaml::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(persisted.enable_tun_mode);
    }

    #[tokio::test]
    async fn clash_version_conflict_after_preflight_never_applies_stale_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let path = Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap();
        let client = ClashConfigClient::new(
            path,
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let writer = client.clone();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let error = client
            .patch_typed_config_with(
                patch,
                move |_candidate| async move {
                    let mut other = writer.get_typed();
                    other.web_ui_list.push("https://concurrent.example".into());
                    let expected = writer.get_snapshot().await?.version;
                    let prepared = writer.prepare_replace(other).await?;
                    assert!(matches!(
                        writer
                            .replace_prepared_if_version(expected, prepared)
                            .await?,
                        ConditionalReplaceResult::Replaced(_)
                    ));
                    Ok(())
                },
                || async {
                    panic!("stale candidate must not apply Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || panic!("stale candidate must not persist a mirror"),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("clash config version conflict"));
        let latest = client.get_snapshot().await.unwrap();
        assert!(!latest.state.enable_tun_mode);
        assert_eq!(latest.state.web_ui_list, vec!["https://concurrent.example"]);
    }

    #[tokio::test]
    async fn clash_persistence_failure_after_preflight_does_not_apply_core() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("config");
        std::fs::create_dir(&root).unwrap();
        let path = Utf8PathBuf::from_path_buf(root.join("clash-config.yaml")).unwrap();
        let client = ClashConfigClient::new(
            path,
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let moved = dir.path().join("hidden-config");
        std::fs::rename(&root, &moved).unwrap();
        let checks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = checks.clone();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let error = client
            .patch_typed_config_with(
                patch,
                move |candidate| async move {
                    assert!(candidate.enable_tun_mode);
                    observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                },
                || async {
                    panic!("a failed source write cannot apply Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || panic!("legacy mirror save is not owed without a source commit"),
            )
            .await
            .unwrap_err();
        assert!(format!("{error:#}").contains("persist"));
        assert_eq!(checks.load(std::sync::atomic::Ordering::Relaxed), 1);
        let after = client.get_snapshot().await.unwrap();
        assert_eq!(after.version, before.version);
        assert!(!after.state.enable_tun_mode);
        std::fs::rename(moved, root).unwrap();
    }

    #[tokio::test]
    async fn a_newer_clash_source_after_core_apply_requires_latest_reconcile() {
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let writer = client.clone();
        let mut patch = ClashConfig::new_empty_patch();
        patch.enable_tun_mode = Some(true);
        let report = client
            .patch_typed_config_with(
                patch,
                |_candidate| async { Ok(()) },
                move || async move {
                    let latest = writer.get_snapshot().await?;
                    assert!(
                        latest.state.enable_tun_mode,
                        "Core must apply after source publication"
                    );
                    let mut advanced = latest.state;
                    advanced.web_ui_list.push("https://newer.example".into());
                    let prepared = writer.prepare_replace(advanced).await?;
                    assert!(matches!(
                        writer
                            .replace_prepared_if_version(latest.version, prepared)
                            .await?,
                        ConditionalReplaceResult::Replaced(_)
                    ));
                    Ok(())
                },
                || Ok(()),
            )
            .await
            .unwrap();
        assert!(report.reaches_runtime);
        assert!(report.degradations.iter().any(|d| {
            d.code == "clash_runtime_deferred"
                && d.retryable
                && d.message.contains("source advanced")
        }));
        let current = client.get_snapshot().await.unwrap();
        assert!(current.version > report.snapshot.version);
        assert_eq!(current.state.web_ui_list, vec!["https://newer.example"]);
    }

    #[tokio::test]
    async fn raw_mapping_override_checks_before_commit_and_defers_failed_apply_without_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let mut mapping = Mapping::new();
        mapping.insert("mode".into(), "global".into());
        let next = apply_mapping_to_typed_clash(&before.state, &mapping).unwrap();
        let reaches_runtime =
            crate::client::application_workflow::impact::classify_clash(&before.state, &next)
                != crate::client::application_workflow::impact::RuntimeImpact::None;
        assert!(reaches_runtime);
        let check_client = client.clone();
        let apply_client = client.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let check_events = events.clone();
        let apply_events = events.clone();
        let save_events = events.clone();

        let report = client
            .commit_candidate_with(
                before,
                next,
                reaches_runtime,
                move |candidate| async move {
                    assert_eq!(
                        candidate.overrides.mode(),
                        chimera_config::clash::config::overrides::Mode::Global
                    );
                    assert_eq!(
                        check_client.get_typed().overrides.mode(),
                        chimera_config::clash::config::overrides::Mode::Rule
                    );
                    check_events.lock().unwrap().push("check");
                    Ok(())
                },
                move || async move {
                    assert_eq!(
                        apply_client.get_typed().overrides.mode(),
                        chimera_config::clash::config::overrides::Mode::Global
                    );
                    apply_events.lock().unwrap().push("apply");
                    anyhow::bail!("injected raw mapping Core failure")
                },
                move || {
                    save_events.lock().unwrap().push("mirror");
                    Ok(())
                },
            )
            .await
            .unwrap();

        assert_eq!(*events.lock().unwrap(), ["check", "apply", "mirror"]);
        assert!(report.reaches_runtime);
        assert_eq!(
            report.snapshot.state.overrides.mode(),
            chimera_config::clash::config::overrides::Mode::Global
        );
        assert_eq!(
            client.get_typed().overrides.mode(),
            chimera_config::clash::config::overrides::Mode::Global
        );
        assert!(report.degradations.iter().any(|item| {
            item.code == "clash_runtime_deferred"
                && item.message.contains("injected raw mapping Core failure")
        }));
    }

    #[tokio::test]
    async fn raw_mapping_candidate_rejection_preserves_source_and_skips_apply_and_mirror() {
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let mut mapping = Mapping::new();
        mapping.insert("mode".into(), "direct".into());
        let next = apply_mapping_to_typed_clash(&before.state, &mapping).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let check_events = events.clone();
        let apply_events = events.clone();
        let save_events = events.clone();
        let error = client
            .commit_candidate_with(
                before.clone(),
                next,
                true,
                move |_candidate| async move {
                    check_events.lock().unwrap().push("check");
                    anyhow::bail!("injected raw mapping candidate rejection")
                },
                move || async move {
                    apply_events.lock().unwrap().push("apply");
                    Ok(())
                },
                move || {
                    save_events.lock().unwrap().push("mirror");
                    Ok(())
                },
            )
            .await
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("injected raw mapping candidate rejection")
        );
        assert_eq!(*events.lock().unwrap(), ["check"]);
        let after = client.get_snapshot().await.unwrap();
        assert_eq!(after.version, before.version);
        assert_eq!(after.state.overrides.mode(), before.state.overrides.mode());
    }

    #[tokio::test]
    async fn running_override_candidate_must_check_then_commit_then_apply_once() {
        use chimera_config::clash::config::overrides::Mode;
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let mut mapping = Mapping::new();
        mapping.insert("mode".into(), "global".into());
        let next = apply_mapping_to_typed_clash(&before.state, &mapping).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let checking = client.clone();
        let applying = client.clone();
        let check_events = events.clone();
        let apply_events = events.clone();
        let mirror_events = events.clone();
        let report = client
            .commit_candidate_with(
                before,
                next,
                true,
                move |candidate| async move {
                    assert_eq!(candidate.overrides.mode(), Mode::Global);
                    assert_eq!(checking.get_typed().overrides.mode(), Mode::Rule);
                    check_events.lock().unwrap().push("check");
                    Ok(())
                },
                move || async move {
                    assert_eq!(applying.get_typed().overrides.mode(), Mode::Global);
                    apply_events.lock().unwrap().push("apply");
                    Ok(())
                },
                move || {
                    mirror_events.lock().unwrap().push("mirror");
                    Ok(())
                },
            )
            .await
            .unwrap();
        assert_eq!(*events.lock().unwrap(), ["check", "apply", "mirror"]);
        assert!(report.reaches_runtime);
        assert!(report.degradations.is_empty());
        assert_eq!(client.get_typed().overrides.mode(), Mode::Global);
    }

    #[tokio::test]
    async fn unchanged_raw_mapping_override_skips_runtime_check_and_apply() {
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let before = client.get_snapshot().await.unwrap();
        let mut mapping = Mapping::new();
        mapping.insert("mode".into(), "rule".into());
        let next = apply_mapping_to_typed_clash(&before.state, &mapping).unwrap();
        let saves = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let save_counter = saves.clone();
        let report = client
            .commit_candidate_with(
                before,
                next,
                crate::client::application_workflow::impact::classify_clash(
                    &client.get_typed(),
                    &apply_mapping_to_typed_clash(&client.get_typed(), &mapping).unwrap(),
                ) != crate::client::application_workflow::impact::RuntimeImpact::None,
                |_candidate| async {
                    panic!("unchanged override must not check Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || async {
                    panic!("unchanged override must not apply Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                move || {
                    save_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                },
            )
            .await
            .unwrap();
        assert!(!report.reaches_runtime);
        assert!(report.degradations.is_empty());
        assert_eq!(saves.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn source_only_clash_patch_neither_checks_nor_applies_core() {
        let dir = tempfile::tempdir().unwrap();
        let client = ClashConfigClient::new(
            Utf8PathBuf::from_path_buf(dir.path().join("clash-config.yaml")).unwrap(),
            ClashConfig::default(),
            Arc::new(RecordingBridge {
                mirrored_tun: Arc::new(Mutex::new(None)),
            }),
        )
        .await
        .unwrap();
        let mut patch = ClashConfig::new_empty_patch();
        patch.web_ui_list = Some(vec!["https://dashboard.example".into()]);
        let report = client
            .patch_typed_config_with(
                patch,
                |_candidate| async {
                    panic!("source-only update must not check Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || async {
                    panic!("source-only update must not apply Core");
                    #[allow(unreachable_code)]
                    Ok(())
                },
                || Ok(()),
            )
            .await
            .unwrap();
        assert!(!report.reaches_runtime);
        assert!(report.degradations.is_empty());
        assert_eq!(
            client.get_typed().web_ui_list,
            vec!["https://dashboard.example"]
        );
    }

    #[test]
    fn noop_test_mirror_type_remains_constructible() {
        let mirror: Box<dyn PreparedLegacyMirror> = Box::new(NoopPreparedLegacyMirror);
        mirror.apply();
    }
}
