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
        self.patch_with_overrides(owner, patch).await
    }

    /// Ref typed Clash source transition: read-only candidate validation,
    /// source CAS and durable publication, then best-effort runtime apply.
    /// Postcommit failures *never* roll back a persisted config version.
    pub(super) async fn patch_typed_config(
        &self,
        owner: &ChimeraClient,
        patch: ClashConfigPatch,
    ) -> Result<ClashCommitReport> {
        let candidate_patch = patch.clone();
        self.patch_typed_config_with(
            patch,
            |candidate| owner.check_clash_runtime_candidate(candidate),
            || async {
                if candidate_patch.enable_tun_mode.is_some() {
                    self.apply_legacy_patch_runtime(owner, &candidate_patch)
                        .await
                } else {
                    update_core_config(owner).await
                }
            },
            || {
                Config::clash().data().save_config()?;
                Config::verge().data().save_file()?;
                Ok(())
            },
        )
        .await
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
            degradations,
        })
    }

    async fn patch_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> Result<ClashCommitReport> {
        let patch = overrides.to_mapping();
        self.patch_with_overrides(owner, patch).await
    }

    /// Follow the source-owned candidate check and commit before reconciling
    /// the running core. A failed apply never rolls back committed config.
    async fn patch_running_overrides(
        &self,
        owner: &ChimeraClient,
        overrides: ClashConfigOverrides,
    ) -> TransactionOutcome {
        match self.patch_overrides(owner, overrides).await {
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
    ) -> Result<ClashCommitReport> {
        let before = self.get_snapshot().await?;
        let current = before.state.clone();
        let next = apply_mapping_to_typed_clash(&current, &patch)?;
        let plan = plan_clash_patch(&patch, &current, &next)?;
        validate_mixed_port_change(&plan, &current)?;
        validate_external_controller_change(owner, &plan, &current, &next).await?;
        let reaches_runtime = crate::state::mutation::impact::reaches_runtime(&current, &next, &[]);
        let mode_changed = plan.mode_changed;
        let break_on_mode_change = plan.break_on_mode_change;
        let report = self
            .commit_candidate_with(
                before,
                next,
                reaches_runtime,
                |candidate| owner.check_clash_runtime_candidate(candidate),
                || async {
                    // RuntimeBuilder consumes the committed typed Clash
                    // source. RuntimeLifecycle publishes the generated
                    // mapping only after the candidate is accepted and applied.
                    update_core_config(owner).await?;
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
    /// Ref CandidateCheck: freeze the currently committed Profile document and
    /// all managed-file content while the candidate Clash document is supplied
    /// independently. The lower Core validator is read-only and does not
    /// acquire the workflow's mutating Core/Service ownership.
    pub(super) async fn check_clash_runtime_candidate(&self, candidate: ClashConfig) -> Result<()> {
        let Some(profiles) = self.inner.typed_profiles.as_ref() else {
            // Compatibility-only fixtures instantiate no Profiles source.
            #[cfg(test)]
            return Ok(());
            #[cfg(not(test))]
            bail!("runtime candidate check requires the typed Profiles source");
        };
        use crate::state::mutation::{DomainChange, RuntimeCandidate, RuntimeGate as _};

        // The ref gate captures the committed Profile document and its files
        // under one fence while checking this proposed Clash source. Reuse
        // its one check implementation; leave postcommit runtime application
        // with the existing CoreLifecycle owner until Clash's actor migrates.
        let checker = super::reconciler::legacy_check::CandidateCheck::new(
            self.inner.core.clone(),
            self.inner.application.clone(),
            self.inner.clash_config.clone(),
            profiles.content_reader(),
            profiles.snapshot_handle(),
            self.inner.core_lifecycle.clone(),
        );
        checker
            .check(RuntimeCandidate {
                change: DomainChange::Clash {
                    candidate: Arc::new(candidate),
                },
                staged: Default::default(),
            })
            .await
            .map_err(anyhow::Error::new)
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
            .patch_overrides(self, overrides)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::mirror::{NoopPreparedLegacyMirror, PreparedLegacyMirror};
    use std::sync::{Arc, Mutex};

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
