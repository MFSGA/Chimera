//! Chimera runtime generation and snapshot adapter shared by reference-aligned
//! Local and Service control endpoints.
//!
//! App config conversion and the legacy application projection stay at the
//! Tauri boundary; process, epoch, health, and config transactions belong to
//! the selected endpoint and `chimera-core-manager`.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{collections::HashMap, fmt, future::Future};

use anyhow::{Context, bail};
use chimera_config::clash::config::ClashConfig;
use chimera_config::{
    profile::{ProfileDefinition, Profiles, ScriptRuntime, TransformDefinition},
    runtime::{
        executor::{StepLogLevel, TransformFailure},
        snapshot::OperatorTag,
    },
};
use chimera_core_manager::{
    ConfigInput, CoreCommand, CoreCommandEnvelope, CoreControl, CoreState, InstanceOptions,
    OperationId, OperationOutput, ReconcileRequest, spec::LocalIpcSettings,
};
use chimera_core_manager::{CoreErrorKind, RevisionId};
use chimera_ipc::api::core::v2::{
    OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo, ReconcileOutcomeKind,
};
use chimera_ipc::api::status::CoreStateDetail;
use parking_lot::RwLock;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    client::{
        RuntimePaths,
        runtime::{
            RuntimeApplyReceipt, RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransformFailure,
        },
    },
    config::{chimera::ClashCore, clash::ClashInfo, core::Config},
    core::{
        actor_v2::{
            CoreStatusSnapshot, control_endpoint::CheckSubmission, control_endpoint::CheckSupport,
            control_endpoint::ControlEndpoint, control_endpoint::CoreSubmission,
            control_endpoint::ExecutionHost, local_host,
        },
        clash::core::RunType,
    },
    enhance::PostProcessingOutput,
    utils::task::spawn_tracked_on_tauri_runtime,
};

fn publish_transform_failure(
    lifecycle: &crate::client::runtime::RuntimeLifecycle,
    error: &anyhow::Error,
    profiles: &Profiles,
    attempt_revision: crate::client::runtime::RuntimeRevision,
) {
    if let Some(transform) = error.downcast_ref::<crate::enhance::TransformFailureError>() {
        lifecycle.publish_transform_failure(RuntimeTransformFailure {
            attempt_revision,
            transform_uid: transform.transform_uid.clone(),
            scope_uid: transform.scope_uid.clone(),
            script_type: transform.script_type,
            message: transform.message(),
        });
        return;
    }

    let Some(crate::enhance::RuntimeBuildError::TransformsFailed { failures, logs }) =
        error.downcast_ref::<crate::enhance::RuntimeBuildError>()
    else {
        return;
    };
    let Some(transform_uid) = failures.iter().find_map(|failure| match failure {
        TransformFailure::Profile { id } => Some(id),
        TransformFailure::Builtin { .. } => None,
    }) else {
        return;
    };
    let Some(log) = logs
        .iter()
        .find(|log| transform_uid_of(&log.tag) == Some(transform_uid.as_str()))
    else {
        return;
    };
    let scope_uid = match &log.tag {
        OperatorTag::ScopedTransform {
            host_profile_id, ..
        } => Some(host_profile_id.0.clone()),
        OperatorTag::GlobalTransform { .. } => None,
        _ => return,
    };
    let script_type = profiles
        .items
        .values()
        .find(|item| item.uid.0 == *transform_uid)
        .and_then(|item| match &item.definition {
            ProfileDefinition::Transform {
                transform: TransformDefinition::Script(script),
            } => Some(match script.runtime {
                ScriptRuntime::JavaScript => {
                    crate::config::profile::item_type::ScriptType::JavaScript
                }
                ScriptRuntime::Lua => crate::config::profile::item_type::ScriptType::Lua,
            }),
            _ => None,
        });
    let message = log
        .entries
        .iter()
        .rev()
        .find(|entry| matches!(entry.level, StepLogLevel::Error | StepLogLevel::Warn))
        .map(|entry| entry.message.clone())
        .unwrap_or_else(|| format!("transform {transform_uid} failed"));
    lifecycle.publish_transform_failure(RuntimeTransformFailure {
        attempt_revision,
        transform_uid: transform_uid.clone(),
        scope_uid,
        script_type,
        message,
    });
}

fn transform_uid_of(tag: &OperatorTag) -> Option<&str> {
    match tag {
        OperatorTag::ScopedTransform {
            transform_profile_id,
            ..
        }
        | OperatorTag::GlobalTransform {
            transform_profile_id,
            ..
        } => Some(transform_profile_id.0.as_str()),
        _ => None,
    }
}

/// Owns Chimera's runtime generation and snapshot adapter around the shared
/// endpoint control plane.
///
/// `control` backs the Local endpoint; the selected endpoint owns the active
/// process lifecycle. `lifecycle` keeps the application-facing transformed
/// snapshot, and `ports` preserves the session's existing port-pick behavior
/// while config compilation remains in the Chimera runtime pipeline.
pub(crate) struct LocalRuntimeHost {
    control: CoreControl,
    runtime_paths: RuntimePaths,
    ports: Arc<crate::client::SessionPortResolver>,
    lifecycle: Arc<crate::client::runtime::RuntimeLifecycle>,
    pending_profile_candidates:
        tokio::sync::Mutex<HashMap<OperationId, crate::client::runtime::CandidateFile>>,
    run_type: RwLock<RunType>,
    recovery_notify: Arc<tokio::sync::Notify>,
    outcome_uncertain: AtomicBool,
    closed: AtomicBool,
}

impl fmt::Debug for LocalRuntimeHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalRuntimeHost")
            .field("is_local", &self.is_local())
            .field("outcome_uncertain", &self.outcome_uncertain())
            .field("closed", &self.closed.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl LocalRuntimeHost {
    #[cfg(test)]
    pub(crate) fn new(control: CoreControl, runtime_paths: RuntimePaths) -> Self {
        Self::new_with_shutdown(
            control,
            runtime_paths,
            CancellationToken::new(),
            &TaskTracker::new(),
        )
    }

    pub(crate) fn new_with_shutdown(
        control: CoreControl,
        runtime_paths: RuntimePaths,
        shutdown: CancellationToken,
        tasks: &TaskTracker,
    ) -> Self {
        let recovery_notify = Arc::new(tokio::sync::Notify::new());
        let mut status = control.subscribe();
        let recovery_signal = recovery_notify.clone();
        let status_shutdown = shutdown.child_token();
        spawn_tracked_on_tauri_runtime(tasks, async move {
            let mut previous = status.borrow().state.clone();
            loop {
                let changed = tokio::select! {
                    _ = status_shutdown.cancelled() => break,
                    changed = status.changed() => changed,
                };
                if changed.is_err() {
                    break;
                }
                let current = status.borrow_and_update().state.clone();
                if matches!(previous, CoreState::Restarting { .. })
                    && matches!(current, CoreState::Running { .. })
                {
                    recovery_signal.notify_one();
                }
                previous = current;
            }
        });
        let lifecycle = Arc::new(crate::client::runtime::RuntimeLifecycle::default());
        Self {
            control,
            runtime_paths,
            ports: Arc::new(crate::client::SessionPortResolver::new(lifecycle.clone())),
            lifecycle,
            pending_profile_candidates: tokio::sync::Mutex::new(HashMap::new()),
            run_type: RwLock::new(RunType::Normal),
            recovery_notify,
            outcome_uncertain: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        }
    }

    pub(crate) fn is_local(&self) -> bool {
        *self.run_type.read() == RunType::Normal
    }

    pub(crate) fn run_type(&self) -> RunType {
        *self.run_type.read()
    }

    /// Select the endpoint established by startup S2. This does not start,
    /// stop, or reconcile a Core; callers must first prove the source host is
    /// retired and then verify the destination owner before applying config.
    pub(crate) fn select_execution_host(
        &self,
        host: super::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        let run_type = match host {
            super::control_endpoint::ExecutionHost::Local => RunType::Normal,
            super::control_endpoint::ExecutionHost::Service => RunType::Service,
        };
        let mut selected = self.run_type.write();
        if *selected != run_type {
            self.ports.invalidate();
            *selected = run_type;
        }
        Ok(())
    }

    pub(crate) fn local_endpoint(&self) -> super::control_endpoint::LocalEndpoint {
        super::control_endpoint::LocalEndpoint::new(self.control.clone())
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) fn confirmed_runtime_receipt(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        self.lifecycle.confirmed_receipt()
    }

    pub(crate) fn accepted_runtime_receipt(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        self.lifecycle.accepted_confirmed_receipt()
    }

    /// Returns the real Core owner session, not a parallel preparation cache.
    /// Port candidate reads and confirmed Core publications use this exact
    /// resolver/RuntimeLifecycle instance on both sides of the Actor.
    pub(crate) fn preparation_session(&self) -> crate::client::ports::RuntimePreparationSession {
        crate::client::ports::RuntimePreparationSession {
            revisions: self.lifecycle.revision_allocator(),
            ports: self.ports.clone(),
        }
    }

    pub(crate) fn invalidate_ports(&self) {
        self.ports.invalidate();
    }

    /// Submit the *exact* checked document through Chimera's existing single
    /// Core host, bypassing a second config build. This is the port required
    /// by pinned ref `RuntimeReconciler::submit_prepared`; enabling the new
    /// production gate additionally requires serializing SourceCommitted and
    /// host ownership in the outer CoreLifecycle Actor.
    pub(crate) async fn reconcile_prepared(
        &self,
        prepared: &crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        endpoint: &dyn ControlEndpoint,
        expected_owner: (ExecutionHost, u64),
    ) -> anyhow::Result<()> {
        self.reconcile_prepared_outcome(prepared, core_spec, endpoint, expected_owner)
            .await
            .map(|_| ())
    }

    /// Return the Core's *confirmed* replacement kind, not a guess based on
    /// differences between persisted Clash documents. Source-instance
    /// interruption can only use this evidence after a successful apply.
    pub(crate) async fn reconcile_prepared_outcome(
        &self,
        prepared: &crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        endpoint: &dyn ControlEndpoint,
        expected_owner: (ExecutionHost, u64),
    ) -> anyhow::Result<bool> {
        let settled = self
            .reconcile_prepared_settlement(prepared, core_spec, endpoint, expected_owner)
            .await?
            .require_published()?;
        settled
            .replaced
            .ok_or_else(|| anyhow::anyhow!("the Core did not provide a replacement verdict"))
    }

    /// Submit and confirm the Core *once*. A failure to publish the derived
    /// file or snapshot is not a Core refusal or a reason to resubmit bytes.
    /// Keep the accepted receipt and product for receipt-fenced repair.
    pub(crate) async fn reconcile_prepared_settlement(
        &self,
        prepared: &crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        endpoint: &dyn ControlEndpoint,
        expected_owner: (ExecutionHost, u64),
    ) -> anyhow::Result<crate::client::core_lifecycle::ports::PreparedCoreSettlement> {
        use crate::client::core_lifecycle::ports::{PreparedRolledBack, PreparedSubmission};
        match self
            .reconcile_prepared_submission(prepared, core_spec, endpoint, expected_owner)
            .await?
        {
            PreparedSubmission::Applied(applied) => Ok(applied),
            PreparedSubmission::RolledBack(report) => {
                let reason = report
                    .failed_apply
                    .clone()
                    .unwrap_or_else(|| "the host returned no rollback detail".into());
                Err(PreparedRolledBack { report, reason }.into())
            }
            PreparedSubmission::Unknown(error) => Err(error.into()),
        }
    }

    pub(crate) async fn reconcile_prepared_submission(
        &self,
        prepared: &crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        endpoint: &dyn ControlEndpoint,
        expected_owner: (ExecutionHost, u64),
    ) -> anyhow::Result<crate::client::core_lifecycle::ports::PreparedSubmission> {
        anyhow::ensure!(
            !self.closed.load(Ordering::Acquire),
            "the local core control plane is shutting down"
        );
        anyhow::ensure!(
            !self.outcome_uncertain(),
            "the local core runtime outcome is uncertain; recover before applying"
        );
        anyhow::ensure!(
            expected_owner.0 == endpoint.host(),
            "the checked Core owner no longer matches the selected endpoint"
        );
        let target_core = prepared.snapshot.target_core;
        let bytes = prepared.snapshot.product_bytes();
        anyhow::ensure!(
            prepared.target.is_some(),
            "checked runtime has no source target identity"
        );
        anyhow::ensure!(
            prepared.intent.config_text.as_bytes() == bytes,
            "checked runtime intent does not match the built product bytes"
        );
        anyhow::ensure!(
            chimera_core_manager::payload_digest(bytes) == prepared.intent.digest,
            "checked runtime digest does not match its exact product bytes"
        );
        anyhow::ensure!(
            prepared.intent.core_type == (&target_core).into(),
            "checked runtime core identity does not match its product"
        );
        anyhow::ensure!(
            core_spec.kind == chimera_core::client::core_spec::core_kind(target_core),
            "checked runtime CoreSpec selects a different core"
        );

        let candidate = self.runtime_paths.create_candidate(bytes).await?;
        // An actor-validated candidate already has an authoritative check.
        // The host still verifies the content digest on submit; re-running
        // transforms or serializing YAML here would violate ref byte identity.
        let status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not read checked runtime baseline").await;
                return Err(error.into());
            }
        };
        let expected_applied = match expected_applied_revision(&status) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "checked runtime baseline is not authoritative").await;
                return Err(error);
            }
        };
        let operation_id = OperationId::generate();
        let submission = CoreSubmission {
            expected_owner: Some(expected_owner),
            envelope: CoreCommandEnvelope {
                operation_id,
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: core_spec.clone(),
                    config: ConfigInput::Inline {
                        bytes: bytes.to_vec(),
                        expected_digest: Some(prepared.intent.digest.clone()),
                    },
                    options: InstanceOptions {
                        local_ipc: Some(prepared.intent.local_ipc),
                        ..InstanceOptions::default()
                    },
                    expected_applied,
                })),
            },
            core_type: Some(prepared.intent.core_type.clone()),
        };
        let outcome = match self
            .submit_and_wait_terminal(endpoint, submission, operation_id.clone())
            .await
        {
            Ok(ObservedReconcile::Applied(outcome)) => outcome,
            Ok(ObservedReconcile::RolledBack(report)) => {
                cleanup_candidate(candidate, "prepared Core restored its previous runtime").await;
                return Ok(
                    crate::client::core_lifecycle::ports::PreparedSubmission::RolledBack(report),
                );
            }
            Err(error) => {
                cleanup_candidate(candidate, "checked runtime operation failed").await;
                if self.outcome_uncertain() {
                    let cause = error
                        .downcast_ref::<chimera_core_manager::CoreError>()
                        .cloned()
                        .unwrap_or(chimera_core_manager::CoreError {
                            kind: None,
                            message: format!("{error:#}"),
                            retryable: false,
                            operation_id: None,
                        })
                        .with_operation(operation_id);
                    return Ok(
                        crate::client::core_lifecycle::ports::PreparedSubmission::Unknown(cause),
                    );
                }
                return Err(error);
            }
        };
        let applied_status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not confirm checked runtime").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(anyhow::Error::from(error).context(
                    "core accepted checked runtime, but its applied status could not be confirmed",
                ));
            }
        };
        let applied_revision = match confirmed_applied_revision(
            endpoint.host(),
            &applied_status,
            &outcome.revision,
            &core_spec,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "checked runtime identity not confirmed").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(error)
                    .context("checked runtime was submitted but could not be confirmed");
            }
        };

        let receipt = Arc::new(RuntimeApplyReceipt {
            revision: prepared.snapshot.revision,
            config_text: prepared.intent.config_text.clone(),
            config_digest: prepared.intent.digest.clone(),
            target_core,
            core_spec,
            host: endpoint.host(),
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: prepared.intent.local_ipc,
            applied_revision,
            instance_id: applied_status.instance_id.clone(),
            controller_generation: Some(expected_owner.1),
            target: prepared.target.clone(),
            ports: prepared.ports.clone(),
            artifact: Some(prepared.snapshot.clone()),
        });
        *self.run_type.write() = match endpoint.host() {
            ExecutionHost::Local => RunType::Normal,
            ExecutionHost::Service => RunType::Service,
        };
        // Publish only the verified Core outcome. `publish_profile_candidate`
        // handles exact-file promotion and applied snapshot identity checks.
        self.lifecycle.publish_confirmed(receipt.clone());
        let publication = match self
            .publish_profile_candidate(candidate, receipt.clone(), prepared.snapshot.clone())
            .await
        {
            Ok(()) => crate::client::core_lifecycle::ports::PreparedProductPublication::Published,
            Err(error) => {
                tracing::warn!(%error, "Core applied the prepared runtime, but product publication needs repair");
                crate::client::core_lifecycle::ports::PreparedProductPublication::Pending {
                    reason: format!("{error:#}"),
                }
            }
        };
        Ok(
            crate::client::core_lifecycle::ports::PreparedSubmission::Applied(
                crate::client::core_lifecycle::ports::PreparedCoreSettlement {
                    report: None,
                    outcome: Some(outcome.clone()),
                    replaced: Some(reconcile_replaced(&outcome)),
                    confirmed: Some(receipt),
                    product: Some(prepared.snapshot.clone()),
                    publication,
                },
            ),
        )
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(
            Some(clash),
            target_core,
            endpoint,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn reconcile_with_profiles(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(
            Some(clash),
            target_core,
            endpoint,
            Some((profiles, app, staged_content)),
            target,
            None,
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn prepare_profile_runtime_prepared(
        &self,
        prepared: crate::client::core_lifecycle::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        endpoint: &dyn ControlEndpoint,
        operation_id: OperationId,
        expected_owner: Option<(ExecutionHost, u64)>,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        let core = prepared.snapshot.target_core;
        self.reconcile_with_profile_source(
            None,
            core,
            endpoint,
            None,
            prepared.target.clone(),
            Some(operation_id),
            Some((prepared, core_spec)),
            expected_owner,
        )
        .await?
        .context("prepared Profile TCC Try returned no held candidate")
    }

    pub(crate) async fn validate_profile_runtime(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        if self.closed.load(Ordering::Acquire) {
            bail!("the local core control plane is shutting down");
        }
        if self.outcome_uncertain() {
            bail!(
                "the local core runtime outcome is uncertain; restart Chimera before checking a Profile"
            );
        }

        let candidate_ports = self.ports.resolve_candidate(&clash)?;
        let revision = self.lifecycle.allocate_revision()?;
        let diagnostic_profiles = profiles.clone();
        let output = match Config::generate_runtime_output_from_profiles(
            &clash,
            target_core,
            app,
            profiles,
            candidate_ports.bindings().clone(),
            staged_content,
            true,
        )
        .await
        {
            Ok(output) => output,
            Err(error) => {
                publish_transform_failure(&self.lifecycle, &error, &diagnostic_profiles, revision);
                return Err(error);
            }
        };
        self.lifecycle.clear_transform_failure();

        let config_bytes = Config::render_runtime_bytes(&output.config)?;
        let digest = chimera_core_manager::payload_digest(&config_bytes);
        let core_spec = local_host::core_spec(&target_core)?;
        let candidate = self
            .runtime_paths
            .create_candidate(&config_bytes)
            .await
            .map_err(|error| {
                crate::client::application_workflow::mutation::RuntimeCheckFailure::candidate_unavailable(
                    format!("could not stage Profile config for checking: {error}"),
                )
            })?;
        let staged_config = if endpoint.host() == ExecutionHost::Service {
            match camino::Utf8PathBuf::from_path_buf(candidate.path().to_path_buf()) {
                Ok(path) => Some(path),
                Err(path) => {
                    cleanup_candidate(candidate, "service check path is not UTF-8").await;
                    return Err(
                        crate::client::application_workflow::mutation::RuntimeCheckFailure::candidate_unavailable(
                            format!(
                                "service config check path is not valid UTF-8: {}",
                                path.to_string_lossy()
                            ),
                        )
                        .into(),
                    );
                }
            }
        } else {
            None
        };
        let check = endpoint
            .check_config(CheckSubmission {
                core_spec: core_spec.clone(),
                core_type: (&target_core).into(),
                config_bytes,
                digest,
                staged_config,
            })
            .await;
        if let Err(error) = candidate.cleanup().await {
            tracing::warn!(%error, "failed to clean a checked Profile runtime candidate");
        }
        profile_runtime_check_outcome(endpoint.host(), check)
            .map(|outcome| outcome.check_record())
            .map_err(anyhow::Error::new)
    }

    async fn reconcile_with_profile_source(
        &self,
        clash: Option<ClashConfig>,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profile_source: Option<(
            Arc<chimera_config::profile::Profiles>,
            chimera_config::application::ChimeraAppConfig,
            std::collections::BTreeMap<String, String>,
        )>,
        target: Option<String>,
        profile_operation_id: Option<OperationId>,
        prepared: Option<(
            crate::client::core_lifecycle::ports::PreparedRuntime,
            chimera_core_manager::CoreSpec,
        )>,
        expected_owner: Option<(ExecutionHost, u64)>,
    ) -> anyhow::Result<
        Option<(
            crate::client::application_workflow::mutation::AppliedCandidate,
            crate::client::application_workflow::mutation::CheckRecord,
        )>,
    > {
        if self.closed.load(Ordering::Acquire) {
            bail!("the local core control plane is shutting down");
        }
        if self.outcome_uncertain() {
            bail!(
                "the local core runtime outcome is uncertain; restart Chimera before applying another runtime"
            );
        }
        if let Some(operation_id) = profile_operation_id.as_ref() {
            anyhow::ensure!(
                !self
                    .pending_profile_candidates
                    .lock()
                    .await
                    .contains_key(operation_id),
                "Profile operation already owns a pending runtime candidate"
            );
        }

        // Use ref's canonical strict candidate directly for Profile TCC.
        // No second runtime build, port reservation or revision allocation
        // can change its exact advisory-checked and Core-submitted bytes.
        let (
            candidate_ports,
            revision,
            config_bytes,
            core_spec,
            local_ipc,
            digest,
            config_text,
            snapshot,
        ) = if let Some((prepared, core_spec)) = prepared {
            anyhow::ensure!(
                profile_operation_id.is_some(),
                "a precommit Profile product requires a TCC operation owner"
            );
            anyhow::ensure!(
                target.is_some() && target == prepared.target,
                "the precommit Profile target does not match the prepared source"
            );
            anyhow::ensure!(
                target_core == prepared.snapshot.target_core
                    && prepared.intent.core_type == (&target_core).into(),
                "prepared Profile candidate uses a different Core type"
            );
            anyhow::ensure!(
                prepared.intent.config_text.as_bytes() == prepared.snapshot.product_bytes()
                    && chimera_core_manager::payload_digest(prepared.snapshot.product_bytes())
                        == prepared.intent.digest,
                "Profile check bytes, snapshot and Core intent disagree"
            );
            self.lifecycle.clear_transform_failure();
            (
                prepared.ports,
                prepared.snapshot.revision,
                prepared.snapshot.product_bytes().to_vec(),
                core_spec,
                prepared.intent.local_ipc,
                prepared.intent.digest.clone(),
                prepared.intent.config_text.clone(),
                prepared.snapshot,
            )
        } else {
            // A canonical PreparedRuntime does not supply or require default
            // Clash values; the legacy generator uses its actual source.
            let clash = clash.context("legacy Profile build requires its original Clash source")?;
            let candidate_ports = self.ports.resolve_candidate(&clash)?;
            let revision = self.lifecycle.allocate_revision()?;
            let mut diagnostic_profiles = None;
            let generated = match profile_source {
                Some((profiles, app, staged_content)) => {
                    diagnostic_profiles = Some(profiles.clone());
                    Config::generate_runtime_output_from_profiles(
                        &clash,
                        target_core,
                        app,
                        profiles,
                        candidate_ports.bindings().clone(),
                        staged_content,
                        true,
                    )
                    .await
                }
                None => {
                    Config::generate_runtime_output_with_ports(
                        &clash,
                        target_core,
                        candidate_ports.bindings().clone(),
                    )
                    .await
                }
            };
            let (config, exists_keys, transform_output, inspection) = match generated {
                Ok(output) => (
                    output.config,
                    output.exists_keys,
                    output.postprocessing_output,
                    output.inspection,
                ),
                Err(error) => {
                    if let Some(profiles) = diagnostic_profiles.as_ref() {
                        publish_transform_failure(&self.lifecycle, &error, profiles, revision);
                    } else if let Some(transform) =
                        error.downcast_ref::<crate::enhance::TransformFailureError>()
                    {
                        self.lifecycle
                            .publish_transform_failure(RuntimeTransformFailure {
                                attempt_revision: revision,
                                transform_uid: transform.transform_uid.clone(),
                                scope_uid: transform.scope_uid.clone(),
                                script_type: transform.script_type,
                                message: transform.message(),
                            });
                    }
                    return Err(error);
                }
            };
            self.lifecycle.clear_transform_failure();

            let config_bytes = Config::render_runtime_bytes(&config)?;
            let core_spec = local_host::core_spec(&target_core)?;
            let local_ipc = LocalIpcSettings {
                policy: chimera_core_manager::LocalIpcPolicy::Disable,
                keep_http_controller: true,
            };
            let config_text: Arc<str> = String::from_utf8(config_bytes.clone())?.into();
            // Use the ref's pure core-owned intent builder for the actual
            // reconcile. Its digest and bytes are one immutable candidate.
            let intent = chimera_core::actor_v2::intent::RuntimeIntentBuilder::build(
                (&target_core).into(),
                config_text,
                local_ipc,
            );
            let digest = intent.digest.clone();
            let config_text = intent.config_text.clone();
            let snapshot = Arc::new(RuntimeSnapshot::from_data(
                revision,
                target_core,
                config_bytes.clone().into(),
                RuntimeSnapshotData {
                    config,
                    exists_keys,
                    postprocessing_output: transform_output,
                    inspection: Arc::new(inspection),
                },
            ));
            (
                candidate_ports,
                revision,
                config_bytes,
                core_spec,
                local_ipc,
                digest,
                config_text,
                snapshot,
            )
        };
        let candidate = self
            .runtime_paths
            .create_candidate(&config_bytes)
            .await
            .map_err(|error| {
                crate::client::application_workflow::mutation::RuntimeCheckFailure::candidate_unavailable(
                    format!("could not stage Profile config for checking: {error}"),
                )
            })?;
        let staged_config = if endpoint.host() == ExecutionHost::Service {
            match camino::Utf8PathBuf::from_path_buf(candidate.path().to_path_buf()) {
                Ok(path) => Some(path),
                Err(path) => {
                    cleanup_candidate(candidate, "service check path is not UTF-8").await;
                    return Err(
                        crate::client::application_workflow::mutation::RuntimeCheckFailure::candidate_unavailable(
                            format!(
                                "service config check path is not valid UTF-8: {}",
                                path.to_string_lossy()
                            ),
                        )
                        .into(),
                    );
                }
            }
        } else {
            None
        };
        let check = endpoint
            .check_config(CheckSubmission {
                core_spec: core_spec.clone(),
                core_type: (&target_core).into(),
                config_bytes: config_bytes.clone(),
                digest: digest.clone(),
                staged_config,
            })
            .await;
        let check_outcome = match profile_runtime_check_outcome(endpoint.host(), check) {
            Ok(outcome) => outcome,
            Err(error) => {
                cleanup_candidate(candidate, "advisory check failed").await;
                return Err(anyhow::Error::new(error));
            }
        };
        if let crate::client::application_workflow::ports::RuntimeCheckOutcome::Unavailable(
            crate::client::application_workflow::ports::RuntimeCheckUnavailable::HostUnsupported {
                reason,
                ..
            },
        ) = &check_outcome
        {
            tracing::debug!(%reason, "core host does not support advisory config checks");
        }
        let check_record = check_outcome.check_record();

        let status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not read reconcile baseline").await;
                return Err(error.into());
            }
        };
        let expected_applied = match expected_applied_revision(&status) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "reconcile baseline is not authoritative").await;
                return Err(error);
            }
        };

        let operation_id = OperationId::generate();
        let submission = CoreSubmission {
            expected_owner,
            envelope: CoreCommandEnvelope {
                operation_id,
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: core_spec.clone(),
                    config: ConfigInput::Inline {
                        bytes: config_bytes,
                        expected_digest: Some(digest.clone()),
                    },
                    options: InstanceOptions {
                        local_ipc: Some(local_ipc),
                        ..InstanceOptions::default()
                    },
                    expected_applied,
                })),
            },
            core_type: Some((&target_core).into()),
        };
        let reconcile_outcome = match self
            .submit_and_wait(endpoint, submission, operation_id)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                cleanup_candidate(candidate, "runtime operation failed").await;
                return Err(error);
            }
        };

        let applied_status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not confirm applied runtime").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(anyhow::Error::from(error).context(
                    "the core accepted the runtime operation, but its applied status could not be confirmed",
                ));
            }
        };
        let applied_revision = match confirmed_applied_revision(
            endpoint.host(),
            &applied_status,
            &reconcile_outcome.revision,
            &core_spec,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "applied runtime identity was not confirmed").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(anyhow::anyhow!(
                    "the core accepted the runtime operation, but status confirmation failed: {error:#}"
                ));
            }
        };
        let receipt = Arc::new(RuntimeApplyReceipt {
            revision,
            config_text,
            config_digest: digest,
            target_core,
            core_spec,
            host: endpoint.host(),
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc,
            applied_revision,
            instance_id: applied_status.instance_id.clone(),
            controller_generation: expected_owner.map(|(_, generation)| generation),
            target,
            ports: candidate_ports,
            artifact: Some(snapshot.clone()),
        });

        *self.run_type.write() = match endpoint.host() {
            ExecutionHost::Local => RunType::Normal,
            ExecutionHost::Service => RunType::Service,
        };
        // The runtime host has confirmed this apply even if the Profile
        // source transaction has not reached Confirm. Keep the receipt and
        // ports bound to what is actually running; Cancel restores its saved
        // baseline receipt through the same confirmed path.
        self.lifecycle.publish_confirmed(receipt.clone());

        if let Some(operation_id) = profile_operation_id {
            let replaced = reconcile_replaced(&reconcile_outcome);
            let mut pending = self.pending_profile_candidates.lock().await;
            anyhow::ensure!(
                !pending.contains_key(&operation_id),
                "Profile operation already owns a pending runtime candidate"
            );
            pending.insert(operation_id, candidate);
            return Ok(Some((
                crate::client::application_workflow::mutation::AppliedCandidate {
                    replaced,
                    receipt,
                    product: snapshot,
                },
                check_record,
            )));
        }
        self.publish_profile_candidate(candidate, receipt, snapshot)
            .await?;
        Ok(None)
    }

    pub(crate) async fn confirm_profile_runtime(
        &self,
        operation_id: &OperationId,
        candidate: crate::client::application_workflow::mutation::AppliedCandidate,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            candidate.product.revision == candidate.receipt.revision
                && candidate.product.target_core == candidate.receipt.target_core
                && chimera_core_manager::payload_digest(candidate.product.product_bytes())
                    == candidate.receipt.config_digest
                && candidate
                    .receipt
                    .artifact
                    .as_ref()
                    .is_some_and(|artifact| { Arc::ptr_eq(artifact, &candidate.product) }),
            "committed Profile product is not the exact candidate accepted by the Core"
        );
        let confirmed = self
            .lifecycle
            .accepted_confirmed_receipt()
            .context("the prepared Profile runtime receipt is no longer confirmed")?;
        anyhow::ensure!(
            confirmed.revision == candidate.receipt.revision
                && confirmed.config_digest == candidate.receipt.config_digest
                && confirmed.target_core == candidate.receipt.target_core
                && confirmed.host == candidate.receipt.host,
            "the confirmed runtime no longer matches the committed Profile candidate"
        );

        let candidate_file = match self
            .pending_profile_candidates
            .lock()
            .await
            .remove(operation_id)
        {
            Some(candidate_file) => candidate_file,
            None => {
                // A prior Confirm may have consumed its staged file before
                // interruption. Recreate the same verified product so replay
                // can safely finish promotion and snapshot publication.
                self.runtime_paths
                    .create_candidate(candidate.product.product_bytes())
                    .await?
            }
        };
        // Verify the *held* precommit file before touching the publicly
        // applied runtime product. A stale/misassociated TCC operation must
        // not overwrite the product even briefly and then report mismatch.
        let staged_bytes = candidate_file.read_verified().await?;
        anyhow::ensure!(
            staged_bytes.as_slice() == candidate.product.product_bytes(),
            "held Profile TCC candidate differs from the accepted Core product"
        );
        self.publish_profile_candidate(candidate_file, candidate.receipt, candidate.product)
            .await
    }

    pub(crate) async fn discard_profile_runtime(&self, operation_id: &OperationId) {
        self.pending_profile_candidates
            .lock()
            .await
            .remove(operation_id);
    }

    async fn publish_profile_candidate(
        &self,
        candidate_file: crate::client::runtime::CandidateFile,
        receipt: Arc<RuntimeApplyReceipt>,
        product: Arc<RuntimeSnapshot>,
    ) -> anyhow::Result<()> {
        self.lifecycle.publish_confirmed(receipt);
        let promoted = crate::client::runtime::promote_candidate(
            &candidate_file,
            self.runtime_paths.product(),
        )
        .await;
        let cleanup = candidate_file.cleanup().await;
        let product_bytes = match promoted {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(error).context(
                    "the core applied the committed Profile runtime, but Chimera could not promote its runtime product",
                );
            }
        };
        if let Err(error) = cleanup {
            tracing::warn!(%error, "failed to clean a promoted Profile runtime candidate");
        }

        if product_bytes.as_slice() != product.product_bytes() {
            bail!("promoted runtime product differs from the confirmed runtime receipt");
        }
        self.lifecycle.publish_promoted(product.clone());
        if let Err(error) = self.lifecycle.publish_applied(product.clone()) {
            return Err(error).context(
                "the core applied the committed Profile runtime, but Chimera could not publish its applied snapshot",
            );
        }
        Config::publish_applied_runtime_legacy(&product.config);
        Ok(())
    }

    #[cfg(test)]
    pub(crate) async fn restore_runtime_receipt(
        &self,
        endpoint: &dyn ControlEndpoint,
        receipt: Arc<RuntimeApplyReceipt>,
    ) -> anyhow::Result<()> {
        self.restore_runtime_receipt_with_owner(endpoint, receipt, None)
            .await
    }

    pub(crate) async fn restore_runtime_receipt_with_owner(
        &self,
        endpoint: &dyn ControlEndpoint,
        receipt: Arc<RuntimeApplyReceipt>,
        expected_owner: Option<(ExecutionHost, u64)>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.closed.load(Ordering::Acquire),
            "the local core control plane is shutting down"
        );
        anyhow::ensure!(
            !self.outcome_uncertain(),
            "the local core runtime outcome is uncertain; restart Chimera before restoring a runtime"
        );
        anyhow::ensure!(
            endpoint.host() == receipt.host,
            "runtime receipt belongs to a different execution host"
        );
        anyhow::ensure!(
            matches!(
                receipt.run_intent,
                crate::client::application_workflow::policy::CoreRunIntent::Running
            ),
            "a stopped runtime receipt cannot be restored by applying a config"
        );
        anyhow::ensure!(
            expected_owner.is_none_or(|(host, _)| host == endpoint.host()),
            "the restore receipt belongs to a different routed host"
        );
        let artifact = receipt
            .artifact
            .as_ref()
            .context("runtime receipt has no Chimera snapshot to restore")?;
        let revision = self.lifecycle.allocate_revision()?;
        let snapshot = Arc::new(artifact.with_revision(revision));
        let config_bytes = receipt.config_text.as_bytes().to_vec();
        let digest = chimera_core_manager::payload_digest(&config_bytes);
        anyhow::ensure!(
            digest == receipt.config_digest,
            "runtime receipt bytes do not match their recorded digest"
        );
        let candidate = self.runtime_paths.create_candidate(&config_bytes).await?;
        let staged_config = if endpoint.host() == ExecutionHost::Service {
            match camino::Utf8PathBuf::from_path_buf(candidate.path().to_path_buf()) {
                Ok(path) => Some(path),
                Err(path) => {
                    cleanup_candidate(candidate, "service restore path is not UTF-8").await;
                    bail!(
                        "service runtime restore path is not valid UTF-8: {}",
                        path.to_string_lossy()
                    );
                }
            }
        } else {
            None
        };
        match endpoint
            .check_config(CheckSubmission {
                core_spec: receipt.core_spec.clone(),
                core_type: (&receipt.target_core).into(),
                config_bytes: config_bytes.clone(),
                digest: digest.clone(),
                staged_config,
            })
            .await
        {
            CheckSupport::Ran(Ok(())) => {}
            CheckSupport::Ran(Err(error)) => {
                cleanup_candidate(candidate, "runtime restore check failed").await;
                return Err(error.into());
            }
            CheckSupport::Unsupported { reason } => {
                tracing::debug!(%reason, "core host does not support advisory restore checks");
            }
        }

        let status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not read runtime restore baseline").await;
                return Err(error.into());
            }
        };
        let expected_applied = match expected_applied_revision(&status) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "runtime restore baseline is not authoritative").await;
                return Err(error);
            }
        };
        let operation_id = OperationId::generate();
        let submission = CoreSubmission {
            expected_owner,
            envelope: CoreCommandEnvelope {
                operation_id,
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: receipt.core_spec.clone(),
                    config: ConfigInput::Inline {
                        bytes: config_bytes,
                        expected_digest: Some(digest.clone()),
                    },
                    options: InstanceOptions {
                        local_ipc: Some(receipt.local_ipc),
                        ..InstanceOptions::default()
                    },
                    expected_applied,
                })),
            },
            core_type: Some((&receipt.target_core).into()),
        };
        let reconcile_outcome = match self
            .submit_and_wait(endpoint, submission, operation_id)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                cleanup_candidate(candidate, "runtime restore operation failed").await;
                self.ports.invalidate();
                return Err(error);
            }
        };

        let applied_status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not confirm restored runtime").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(anyhow::Error::from(error).context(
                    "the runtime restore was accepted, but its applied status could not be confirmed",
                ));
            }
        };
        let applied_revision = match confirmed_applied_revision(
            endpoint.host(),
            &applied_status,
            &reconcile_outcome.revision,
            &receipt.core_spec,
        ) {
            Ok(revision) => revision,
            Err(error) => {
                cleanup_candidate(candidate, "restored runtime identity was not confirmed").await;
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(anyhow::anyhow!(
                    "the runtime restore was accepted, but status confirmation failed: {error:#}"
                ));
            }
        };
        let restored_receipt = Arc::new(RuntimeApplyReceipt {
            revision,
            config_text: receipt.config_text.clone(),
            config_digest: digest,
            target_core: receipt.target_core,
            core_spec: receipt.core_spec.clone(),
            host: receipt.host,
            run_intent: receipt.run_intent,
            local_ipc: receipt.local_ipc,
            applied_revision,
            instance_id: applied_status.instance_id.clone(),
            controller_generation: expected_owner.map(|(_, generation)| generation),
            target: receipt.target.clone(),
            ports: receipt.ports.clone(),
            artifact: Some(snapshot.clone()),
        });
        self.lifecycle.publish_confirmed(restored_receipt);
        *self.run_type.write() = match endpoint.host() {
            ExecutionHost::Local => RunType::Normal,
            ExecutionHost::Service => RunType::Service,
        };

        let promoted =
            crate::client::runtime::promote_candidate(&candidate, self.runtime_paths.product())
                .await;
        let cleanup = candidate.cleanup().await;
        let product_bytes = match promoted {
            Ok(bytes) => bytes,
            Err(error) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(error).context(
                    "the core restored the runtime, but Chimera could not promote its runtime product",
                );
            }
        };
        if let Err(error) = cleanup {
            tracing::warn!(%error, "failed to clean a restored runtime candidate");
        }
        if product_bytes.as_slice() != snapshot.product_bytes() {
            self.outcome_uncertain.store(true, Ordering::Release);
            bail!("restored runtime product differs from its confirmed receipt");
        }
        self.lifecycle.publish_promoted(snapshot.clone());
        if let Err(error) = self.lifecycle.publish_applied(snapshot.clone()) {
            self.outcome_uncertain.store(true, Ordering::Release);
            return Err(error).context(
                "the core restored the runtime, but Chimera could not publish its applied snapshot",
            );
        }
        Config::publish_applied_runtime_legacy(&snapshot.config);
        Ok(())
    }

    async fn submit_and_wait(
        &self,
        endpoint: &dyn ControlEndpoint,
        submission: CoreSubmission,
        operation_id: OperationId,
    ) -> anyhow::Result<ReconcileOutcomeInfo> {
        match self
            .submit_and_wait_terminal(endpoint, submission, operation_id)
            .await?
        {
            ObservedReconcile::Applied(outcome) => Ok(outcome),
            ObservedReconcile::RolledBack(report) => {
                accept_reconcile_outcome(&report).map(|_| report)
            }
        }
    }

    /// Keep the original terminal ReconcileOutcomeInfo intact for ref-style
    /// Applied/RolledBack classification. The compatibility method above
    /// continues to report rollback as an error to old Profile transactions.
    async fn submit_and_wait_terminal(
        &self,
        endpoint: &dyn ControlEndpoint,
        submission: CoreSubmission,
        operation_id: OperationId,
    ) -> anyhow::Result<ObservedReconcile> {
        let mut operation = match endpoint.submit(submission).await {
            Ok(operation) => operation,
            Err(error) => {
                // An accepted local or Service submit can outlive its
                // waiting caller. The ref CoreActor serializes admission but
                // the envelope id is not retained by this application-layer
                // transaction after an error. Never automatically generate
                // another id and risk duplicate application.
                if error.retryable
                    || error.kind.is_some_and(|kind| {
                        matches!(
                            kind,
                            CoreErrorKind::BackendUnavailable | CoreErrorKind::Internal
                        )
                    })
                {
                    self.ports.invalidate();
                    self.outcome_uncertain.store(true, Ordering::Release);
                }
                return Err(error.into());
            }
        };

        if operation.id != operation_id.to_string() {
            self.ports.invalidate();
            self.outcome_uncertain.store(true, Ordering::Release);
            bail!("core host returned a different reconcile operation id");
        }
        if matches!(
            operation.phase,
            OperationPhase::Queued | OperationPhase::Running
        ) {
            operation = match endpoint
                .wait_operation(operation_id, std::time::Duration::from_secs(60))
                .await
            {
                Some(operation) => operation,
                None => {
                    self.ports.invalidate();
                    self.outcome_uncertain.store(true, Ordering::Release);
                    bail!("core reconcile outcome could not be observed");
                }
            };
        }
        if operation.id != operation_id.to_string() {
            self.ports.invalidate();
            self.outcome_uncertain.store(true, Ordering::Release);
            bail!("core host returned a different reconcile operation id");
        }

        match (operation.phase, operation.output) {
            (OperationPhase::Succeeded, Some(OperationOutputInfo::Reconciled(outcome))) => {
                if outcome.outcome == ReconcileOutcomeKind::RolledBack {
                    Ok(ObservedReconcile::RolledBack(outcome))
                } else {
                    accept_reconcile_outcome(&outcome)?;
                    Ok(ObservedReconcile::Applied(outcome))
                }
            }
            (OperationPhase::Failed, _) => bail!(
                "core reconcile failed: {}",
                operation
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "core host returned no failure detail".into())
            ),
            (OperationPhase::Queued | OperationPhase::Running, _) => {
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                bail!("core reconcile is still running; its outcome is uncertain");
            }
            (OperationPhase::Succeeded, output) => {
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                bail!("core reconcile completed with an unexpected result: {output:?}");
            }
        }
    }

    pub(crate) async fn shutdown(&self) -> anyhow::Result<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.control.shutdown().await.map_err(anyhow::Error::from)
    }

    /// Stop the active core while keeping the control executor available for
    /// the next start or runtime update. Application shutdown and stopping a
    /// core are distinct commands in `CoreControl`.
    pub(crate) async fn stop_core(&self) -> anyhow::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            bail!("the local core control plane is shutting down");
        }
        if matches!(self.control.status().state, CoreState::Stopped { .. }) {
            self.ports.invalidate();
            return Ok(());
        }
        let envelope = CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Stop,
        };
        let handle = match self.control.submit(envelope) {
            Ok(handle) => handle,
            Err(error) => return Err(error.into()),
        };
        match handle.wait().await {
            Ok(OperationOutput::Stopped) => {
                self.ports.invalidate();
                Ok(())
            }
            Ok(output) => {
                self.ports.invalidate();
                self.outcome_uncertain.store(true, Ordering::Release);
                bail!("core stop returned unexpected output: {output:?}");
            }
            Err(error) => {
                if error.kind.is_some_and(|kind| {
                    matches!(
                        kind,
                        chimera_core_manager::CoreErrorKind::StopUnconfirmed
                            | chimera_core_manager::CoreErrorKind::Quarantined
                    )
                }) || self.control.executor_is_closed()
                {
                    self.ports.invalidate();
                    self.outcome_uncertain.store(true, Ordering::Release);
                }
                Err(error.into())
            }
        }
    }

    pub(crate) async fn status(&self) -> CoreStatusSnapshot {
        let status = self.control.status();
        let state = match status.state {
            CoreState::Running { .. } => chimera_ipc::api::status::CoreState::Running,
            CoreState::Stopped { ref reason } => chimera_ipc::api::status::CoreState::Stopped(
                reason.as_ref().map(ToString::to_string),
            ),
            CoreState::Starting { .. }
            | CoreState::Restarting { .. }
            | CoreState::Switching { .. }
            | CoreState::Stopping { .. } => chimera_ipc::api::status::CoreState::Stopped(None),
            _ => chimera_ipc::api::status::CoreState::Stopped(None),
        };
        CoreStatusSnapshot {
            instance_id: status
                .instance_id
                .map(|instance_id| instance_id.to_string()),
            state,
            state_changed_at: status.changed_at,
            run_type: *self.run_type.read(),
        }
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        self.lifecycle.snapshot().applied.map(|snapshot| {
            (
                snapshot.revision.get(),
                snapshot.postprocessing_output.clone(),
            )
        })
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.lifecycle.snapshot().promoted
    }

    pub(crate) fn pending_runtime_product_target(&self) -> Option<String> {
        let (receipt, _) = self.lifecycle.pending_confirmed_product()?;
        receipt.target.clone()
    }

    /// Repair only the derived snapshot publication for a still-active Core
    /// receipt. The caller first checks the selected endpoint's live revision;
    /// this method does not re-submit runtime bytes or touch Profile sources.
    pub(crate) async fn republish_pending_runtime_product(
        &self,
        receipt: &Arc<RuntimeApplyReceipt>,
    ) -> anyhow::Result<bool> {
        if self.closed.load(Ordering::Acquire) || self.outcome_uncertain() {
            return Ok(false);
        }
        let Some((pending_receipt, product)) = self.lifecycle.pending_confirmed_product() else {
            return Ok(false);
        };
        if !Arc::ptr_eq(&pending_receipt, receipt) {
            return Ok(false);
        }
        let promoted = self
            .lifecycle
            .snapshot()
            .promoted
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, &product));
        if !promoted {
            // Core already accepted these exact bytes. Repair only the
            // derived file; do not call Reconcile/prepare another candidate.
            crate::client::runtime::restore_product(
                self.runtime_paths.product(),
                product.product_bytes(),
            )
            .await?;
            if !self
                .lifecycle
                .accepted_confirmed_receipt()
                .is_some_and(|active| Arc::ptr_eq(&active, receipt))
            {
                // A newer Core receipt superseded this file while it was
                // being promoted; it must never become the Applied snapshot.
                return Ok(false);
            }
            self.lifecycle.publish_promoted(product.clone());
        }
        let Some(published) = self.lifecycle.republish_confirmed_product(receipt)? else {
            return Ok(false);
        };
        Config::publish_applied_runtime_legacy(&published.config);
        Ok(true)
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.lifecycle.snapshot().last_transform_failure
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        self.lifecycle
            .snapshot()
            .applied
            .map(|snapshot| snapshot.clash_info())
            .unwrap_or_else(|| Config::clash().latest().get_client_info())
    }

    pub(crate) async fn active_clash_info(&self) -> anyhow::Result<ClashInfo> {
        let connection = self
            .control
            .api_connection()
            .await
            .context("no running local core exposes an API connection")?;
        let mut info = self.effective_clash_info();

        match connection.controller.host {
            chimera_core_manager::Host::Http(url) => {
                if url.scheme() != "http" {
                    bail!("the local core API uses an unsupported URL scheme");
                }
                let host = match url.host().context("local core API URL has no host")? {
                    url::Host::Ipv6(address) => format!("[{address}]"),
                    host => host.to_string(),
                };
                let port = url
                    .port_or_known_default()
                    .context("local core API URL has no usable port")?;
                info.server = format!("{host}:{port}");
                info.port = port;
            }
            chimera_core_manager::Host::NamedPipe(_)
            | chimera_core_manager::Host::UnixSocket(_) => {
                bail!("the local core API uses a non-HTTP controller transport");
            }
            _ => bail!("the local core API uses an unsupported controller transport"),
        }
        info.secret = connection.controller.secret;
        Ok(info)
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.recovery_notify.clone()
    }
}

#[derive(Debug, Clone)]
enum ObservedReconcile {
    Applied(ReconcileOutcomeInfo),
    RolledBack(ReconcileOutcomeInfo),
}

async fn cleanup_candidate(candidate: crate::client::runtime::CandidateFile, reason: &str) {
    if let Err(error) = candidate.cleanup().await {
        tracing::warn!(%error, reason, "failed to clean a rejected runtime candidate");
    }
}

fn expected_applied_revision(
    status: &super::control_endpoint::CoreStatusSnapshot,
) -> anyhow::Result<Option<RevisionId>> {
    let Some(state) = status.state.as_ref() else {
        bail!("core host did not publish an authoritative runtime state");
    };
    match state {
        CoreStateDetail::Stopped { .. } => {}
        CoreStateDetail::Running { .. } if status.revision.is_some() => {}
        CoreStateDetail::Running { .. } => {
            bail!("running core host did not publish its applied revision");
        }
        CoreStateDetail::Starting { .. }
        | CoreStateDetail::Restarting { .. }
        | CoreStateDetail::Switching { .. }
        | CoreStateDetail::Stopping { .. } => {
            bail!("core host is changing state; reconcile must be retried after it settles");
        }
    }

    status
        .revision
        .as_ref()
        .map(|revision| {
            let epoch = chimera_core_manager::Epoch::new(revision.epoch)
                .ok_or_else(|| anyhow::anyhow!("the applied revision reported epoch 0"))?;
            Ok(RevisionId {
                epoch,
                generation: revision.generation,
                effective_hash: revision.effective_hash.clone(),
            })
        })
        .transpose()
}

fn confirmed_applied_revision(
    _host: ExecutionHost,
    status: &super::control_endpoint::CoreStatusSnapshot,
    expected: &chimera_ipc::api::status::ConfigRevisionInfo,
    core_spec: &chimera_core_manager::CoreSpec,
) -> anyhow::Result<chimera_ipc::api::status::ConfigRevisionInfo> {
    anyhow::ensure!(
        matches!(status.state, Some(CoreStateDetail::Running { epoch, .. }) if epoch == expected.epoch),
        "core host did not confirm the runtime epoch returned by reconcile"
    );
    let applied_revision = status
        .revision
        .as_ref()
        .context("core host did not report the applied runtime revision")?;
    anyhow::ensure!(
        status.source_hash.as_deref() == Some(expected.source_hash.as_str()),
        "core host source hash does not match the reconcile result"
    );
    anyhow::ensure!(
        applied_revision.epoch == expected.epoch
            && applied_revision.generation == expected.generation
            && applied_revision.effective_hash == expected.effective_hash,
        "core host revision identity does not match the reconcile result"
    );
    anyhow::ensure!(
        status
            .applied_kind
            .is_none_or(|kind| kind == core_spec.kind),
        "core host applied a different core than the submitted runtime"
    );
    Ok(chimera_ipc::api::status::ConfigRevisionInfo {
        epoch: applied_revision.epoch,
        generation: applied_revision.generation,
        source_hash: expected.source_hash.clone(),
        effective_hash: applied_revision.effective_hash.clone(),
    })
}

fn accept_reconcile_outcome(outcome: &ReconcileOutcomeInfo) -> anyhow::Result<()> {
    if outcome.outcome == ReconcileOutcomeKind::RolledBack {
        bail!(
            "core runtime apply was rolled back: {}",
            outcome
                .failed_apply
                .as_deref()
                .unwrap_or("the host returned no rollback detail")
        );
    }
    if let Some(warning) = &outcome.warning {
        tracing::warn!(%warning, "core runtime applied with uncertain disk durability");
    }
    Ok(())
}

fn reconcile_replaced(outcome: &ReconcileOutcomeInfo) -> bool {
    matches!(
        outcome.outcome,
        ReconcileOutcomeKind::Started
            | ReconcileOutcomeKind::Restarted
            | ReconcileOutcomeKind::Switched
    )
}

fn profile_runtime_check_outcome(
    host: ExecutionHost,
    support: CheckSupport,
) -> Result<
    crate::client::application_workflow::ports::RuntimeCheckOutcome,
    crate::client::application_workflow::mutation::RuntimeCheckFailure,
> {
    use crate::client::application_workflow::ports::{
        RuntimeCheckOutcome, RuntimeCheckUnavailable,
    };

    match support {
        CheckSupport::Ran(Ok(())) => Ok(RuntimeCheckOutcome::Passed),
        CheckSupport::Ran(Err(error)) => Err(
            crate::client::application_workflow::mutation::RuntimeCheckFailure::from_core_error(
                error,
            ),
        ),
        CheckSupport::Unsupported { reason } => Ok(RuntimeCheckOutcome::Unavailable(
            RuntimeCheckUnavailable::HostUnsupported { host, reason },
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Mutex as StdMutex, time::Duration};

    use chimera_core_manager::{CoreKind, LocalIpcPolicy};
    use chimera_ipc::api::{
        core::v2::{
            OperationInfo, OperationOutputInfo, OperationPhase, ReconcileOutcomeInfo,
            ReconcileOutcomeKind,
        },
        status::{ConfigRevisionInfo, RevisionIdInfo},
    };

    async fn build_local_control(
        paths: &crate::utils::path::PathResolver,
    ) -> chimera_core_manager::CoreControl {
        local_host::build(
            paths,
            &paths.app_config_dir().join("helper.exe"),
            Arc::new(chimera_utils::process::AtomicStopStrategy::new(
                chimera_utils::process::StopStrategy::default(),
            )),
        )
        .await
        .unwrap()
    }

    // Contract: with an isolated app root and a newly built local host, status
    // must come from CoreControl as stopped/normal; a host that silently kept
    // reading the unused legacy manager would not know its manager's timestamp.
    // Stopping an already stopped core is idempotent and leaves the executor
    // open for a later reconcile.
    #[tokio::test]
    async fn status_projects_the_new_control_plane_before_first_start() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            root.path().join("config"),
            root.path().join("data"),
        );
        let control = build_local_control(&paths).await;
        let runtime_paths = RuntimePaths::new(
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/clash.yaml"))
                .unwrap(),
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/candidates"))
                .unwrap(),
        );
        let host = LocalRuntimeHost::new(control, runtime_paths);

        let status = host.status().await;

        assert!(matches!(
            status.state,
            chimera_ipc::api::status::CoreState::Stopped(None)
        ));
        assert_eq!(status.run_type, RunType::Normal);
        assert!(status.state_changed_at > 0);
        host.stop_core().await.unwrap();
        assert!(!host.control.executor_is_closed());
        assert!(matches!(
            host.status().await.state,
            chimera_ipc::api::status::CoreState::Stopped(_)
        ));
        host.shutdown().await.unwrap();
    }

    struct RecordingEndpoint {
        status: StdMutex<super::super::control_endpoint::CoreStatusSnapshot>,
        submitted: StdMutex<
            Vec<(
                Vec<u8>,
                Option<LocalIpcSettings>,
                Option<(ExecutionHost, u64)>,
            )>,
        >,
        apply_submission: bool,
        /// Check replacement semantics independently from runtime receipt.
        patched: bool,
        /// A terminal Core rollback is a success for the host operation, but
        /// a refusal for the candidate source that was not applied.
        rolled_back: bool,
        /// A registered command that remains Running after bounded wait.
        lose_result: bool,
    }

    #[async_trait::async_trait]
    impl ControlEndpoint for RecordingEndpoint {
        fn host(&self) -> ExecutionHost {
            ExecutionHost::Local
        }

        async fn check_config(&self, _submission: CheckSubmission) -> CheckSupport {
            CheckSupport::Ran(Ok(()))
        }

        async fn submit(
            &self,
            submission: CoreSubmission,
        ) -> Result<OperationInfo, chimera_core_manager::CoreError> {
            let chimera_core_manager::CoreCommandEnvelope {
                operation_id,
                command,
            } = submission.envelope;
            let CoreCommand::Reconcile(request) = command else {
                unreachable!("restore should submit a reconcile")
            };
            let ConfigInput::Inline {
                bytes,
                expected_digest,
            } = request.config;
            let digest = chimera_core_manager::payload_digest(&bytes);
            assert_eq!(expected_digest.as_deref(), Some(digest.as_str()));
            self.submitted
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((bytes, request.options.local_ipc, submission.expected_owner));

            let generation = if self.apply_submission && !self.rolled_back {
                let mut status = self
                    .status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let generation = status
                    .revision
                    .as_ref()
                    .map_or(1, |revision| revision.generation + 1);
                let revision = ConfigRevisionInfo {
                    epoch: 1,
                    generation,
                    source_hash: digest.clone(),
                    effective_hash: format!("effective-{generation}"),
                };
                status.state = Some(CoreStateDetail::Running { epoch: 1, pid: 42 });
                status.revision = Some(revision.id());
                status.source_hash = Some(digest.clone());
                status.applied_kind = Some(request.core.kind);
                generation
            } else {
                self.status
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .revision
                    .as_ref()
                    .map_or(1, |revision| revision.generation + 1)
            };
            let revision = ConfigRevisionInfo {
                epoch: 1,
                generation,
                source_hash: digest,
                effective_hash: format!("effective-{generation}"),
            };
            let rollback_revision = if self.rolled_back {
                let status = self.status.lock().unwrap();
                let id = status.revision.as_ref().expect("old Core revision");
                ConfigRevisionInfo {
                    epoch: id.epoch,
                    generation: id.generation,
                    source_hash: status.source_hash.clone().unwrap_or_default(),
                    effective_hash: id.effective_hash.clone(),
                }
            } else {
                revision
            };
            Ok(OperationInfo {
                id: operation_id.to_string(),
                phase: if self.lose_result {
                    OperationPhase::Running
                } else {
                    OperationPhase::Succeeded
                },
                output: (!self.lose_result).then_some(OperationOutputInfo::Reconciled(
                    ReconcileOutcomeInfo {
                        outcome: if self.rolled_back {
                            ReconcileOutcomeKind::RolledBack
                        } else if self.patched {
                            ReconcileOutcomeKind::Patched
                        } else {
                            ReconcileOutcomeKind::Started
                        },
                        revision: rollback_revision,
                        warning: None,
                        failed_apply: self
                            .rolled_back
                            .then(|| "candidate rejected; previous runtime restored".into()),
                    },
                )),
                error: None,
            })
        }

        async fn wait_operation(
            &self,
            _id: OperationId,
            _timeout: Duration,
        ) -> Option<OperationInfo> {
            None
        }

        async fn status(
            &self,
        ) -> Result<
            super::super::control_endpoint::CoreStatusSnapshot,
            chimera_core_manager::CoreError,
        > {
            Ok(self
                .status
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone())
        }
    }

    async fn runtime_host_fixture() -> (tempfile::TempDir, LocalRuntimeHost, RuntimePaths) {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            root.path().join("config"),
            root.path().join("data"),
        );
        let control = build_local_control(&paths).await;
        let runtime_paths = RuntimePaths::new(
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/clash.yaml"))
                .unwrap(),
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/candidates"))
                .unwrap(),
        );
        let host = LocalRuntimeHost::new(control, runtime_paths.clone());
        (root, host, runtime_paths)
    }

    fn recording_endpoint(apply_submission: bool) -> RecordingEndpoint {
        RecordingEndpoint {
            status: StdMutex::new(super::super::control_endpoint::CoreStatusSnapshot {
                instance_id: None,
                controller: None,
                state: Some(CoreStateDetail::Running { epoch: 1, pid: 42 }),
                state_changed_at: 1,
                revision: Some(RevisionIdInfo {
                    epoch: 1,
                    generation: 2,
                    effective_hash: "before-restore".into(),
                }),
                source_hash: Some("candidate-digest".into()),
                healthy: Some(true),
                applied_kind: Some(CoreKind::Mihomo),
            }),
            submitted: StdMutex::new(Vec::new()),
            apply_submission,
            patched: false,
            rolled_back: false,
            lose_result: false,
        }
    }

    fn restore_receipt(config_text: Arc<str>) -> Arc<RuntimeApplyReceipt> {
        let target_core = ClashCore::Mihomo;
        let runtime_revision = crate::client::runtime::RuntimeRevisionAllocator::default()
            .allocate()
            .unwrap();
        let artifact = Arc::new(RuntimeSnapshot::new_with_transform_output(
            runtime_revision,
            target_core,
            config_text.as_bytes().to_vec(),
            serde_yaml::Mapping::new(),
            PostProcessingOutput::default(),
        ));
        Arc::new(RuntimeApplyReceipt {
            revision: runtime_revision,
            config_digest: chimera_core_manager::payload_digest(config_text.as_bytes()),
            config_text,
            target_core,
            core_spec: chimera_core_manager::CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("/test/mihomo"),
                version: None,
                features: Vec::new(),
            },
            host: ExecutionHost::Local,
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: LocalIpcSettings {
                policy: LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            instance_id: None,
            controller_generation: None,
            applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "baseline-digest".into(),
                effective_hash: "old-effective".into(),
            },
            target: None,
            ports: Default::default(),
            artifact: Some(artifact),
        })
    }

    fn prepared_runtime_fixture(
        bytes: Arc<str>,
    ) -> (
        crate::client::core_lifecycle::ports::PreparedRuntime,
        chimera_core_manager::CoreSpec,
    ) {
        use chimera_core::actor_v2::intent::RuntimeIntentBuilder;
        let receipt = restore_receipt(bytes.clone());
        let snapshot = receipt.artifact.as_ref().unwrap().clone();
        let prepared = crate::client::core_lifecycle::ports::PreparedRuntime {
            snapshot,
            intent: Arc::new(RuntimeIntentBuilder::build(
                (&receipt.target_core).into(),
                bytes,
                receipt.local_ipc,
            )),
            ports: receipt.ports.clone(),
            target: Some("committed-source-42".into()),
        };
        (prepared, receipt.core_spec.clone())
    }

    #[tokio::test]
    async fn prepared_profile_tcc_try_holds_exact_checked_bytes_until_confirm() {
        use chimera_core::actor_v2::intent::RuntimeIntentBuilder;

        let (_root, host, paths) = runtime_host_fixture().await;
        let session = host.preparation_session();
        let (mut prepared, spec) =
            prepared_runtime_fixture(Arc::from("mode: rule\n# exact checked source\n"));
        let revision = session.revisions.allocate().unwrap();
        prepared.snapshot = Arc::new(prepared.snapshot.with_revision(revision));
        prepared.ports = session
            .ports
            .resolve_candidate(&ClashConfig::default())
            .unwrap();
        // Ref uses the selected source's control-channel policy, not the
        // legacy Profile path's unconditional local IPC Disable setting.
        let settings = LocalIpcSettings {
            policy: LocalIpcPolicy::Prefer,
            keep_http_controller: false,
        };
        prepared.intent = Arc::new(RuntimeIntentBuilder::build(
            (&prepared.snapshot.target_core).into(),
            prepared.intent.config_text.clone(),
            settings,
        ));
        let original_snapshot = prepared.snapshot.clone();
        let exact_bytes = prepared.snapshot.product_bytes().to_vec();
        let target = prepared.target.clone();
        let endpoint = recording_endpoint(true);
        let operation_id = OperationId::generate();

        let (candidate, check) = host
            .prepare_profile_runtime_prepared(
                prepared,
                spec,
                &endpoint,
                operation_id.clone(),
                Some((ExecutionHost::Local, 3)),
            )
            .await
            .expect("TCC Try submits only the canonical strictly prepared candidate");
        assert_eq!(
            check,
            crate::client::application_workflow::mutation::CheckRecord::Passed
        );
        assert!(Arc::ptr_eq(&candidate.product, &original_snapshot));
        assert_eq!(candidate.receipt.revision, revision);
        assert_eq!(candidate.receipt.target, target);
        assert_eq!(candidate.receipt.local_ipc, settings);
        assert!(Arc::ptr_eq(
            candidate.receipt.artifact.as_ref().unwrap(),
            &original_snapshot
        ));
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        {
            let submitted = endpoint.submitted.lock().unwrap();
            assert_eq!(
                submitted[0].0, exact_bytes,
                "check and Core Try use the same input bytes"
            );
            assert_eq!(submitted[0].1, Some(settings));
            assert_eq!(
                submitted[0].2,
                Some((ExecutionHost::Local, 3)),
                "the Profile Try must fence the current router owner generation"
            );
        }
        assert!(
            host.pending_profile_candidates
                .lock()
                .await
                .contains_key(&operation_id)
        );
        assert!(host.lifecycle.snapshot().promoted.is_none());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert!(
            !paths.product().exists(),
            "the Profile source has not committed; no runtime product is public"
        );

        host.confirm_profile_runtime(&operation_id, candidate.clone())
            .await
            .expect("only the source TCC Confirm may promote the exact product");
        assert_eq!(tokio::fs::read(paths.product()).await.unwrap(), exact_bytes);
        assert!(Arc::ptr_eq(
            &host.lifecycle.snapshot().applied.unwrap(),
            &original_snapshot
        ));
        assert!(host.pending_profile_candidates.lock().await.is_empty());
        assert_eq!(
            endpoint.submitted.lock().unwrap().len(),
            1,
            "Confirm must not reapply Core"
        );

        // A duplicate Confirm after the candidate file is consumed must
        // promote identical receipt bytes, still without resubmitting Core.
        host.confirm_profile_runtime(&operation_id, candidate)
            .await
            .unwrap();
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn profile_tcc_confirm_rejects_wrong_held_file_before_touching_public_product() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let endpoint = recording_endpoint(true);
        let operation_id = OperationId::generate();
        let (accepted, _) = host
            .prepare_profile_runtime_prepared(prepared, spec, &endpoint, operation_id.clone(), None)
            .await
            .unwrap();
        let substituted = paths.create_candidate(b"mode: global\n").await.unwrap();
        host.pending_profile_candidates
            .lock()
            .await
            .insert(operation_id.clone(), substituted);
        let error = host
            .confirm_profile_runtime(&operation_id, accepted.clone())
            .await
            .expect_err("a mismatched held file may never overwrite the public product");
        assert!(
            error
                .to_string()
                .contains("held Profile TCC candidate differs")
        );
        assert!(!paths.product().exists());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);

        // Once the bad file is discarded, the original accepted Core receipt
        // may reconstruct only its exact bytes; no Core command is replayed.
        host.confirm_profile_runtime(&operation_id, accepted.clone())
            .await
            .unwrap();
        assert_eq!(
            tokio::fs::read(paths.product()).await.unwrap(),
            accepted.product.product_bytes()
        );
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert!(Arc::ptr_eq(
            &host.lifecycle.snapshot().applied.unwrap(),
            &accepted.product,
        ));
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn profile_tcc_confirm_rejects_forged_product_identity_before_file_promotion() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let endpoint = recording_endpoint(true);
        let operation_id = OperationId::generate();
        let (accepted, _) = host
            .prepare_profile_runtime_prepared(prepared, spec, &endpoint, operation_id.clone(), None)
            .await
            .unwrap();
        let forged = crate::client::application_workflow::mutation::AppliedCandidate {
            replaced: accepted.replaced,
            receipt: accepted.receipt.clone(),
            product: Arc::new((*accepted.product).clone()),
        };
        assert!(
            host.confirm_profile_runtime(&operation_id, forged)
                .await
                .is_err(),
            "copying a product is not the exact accepted TCC artifact"
        );
        assert!(!paths.product().exists());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert!(
            host.pending_profile_candidates
                .lock()
                .await
                .contains_key(&operation_id),
            "a forged product must not consume the legitimate TCC candidate"
        );
        host.discard_profile_runtime(&operation_id).await;
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_profile_tcc_cancel_discards_held_file_without_publication() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let endpoint = recording_endpoint(true);
        let operation_id = OperationId::generate();
        let (candidate, _) = host
            .prepare_profile_runtime_prepared(prepared, spec, &endpoint, operation_id.clone(), None)
            .await
            .unwrap();
        assert!(
            host.pending_profile_candidates
                .lock()
                .await
                .contains_key(&operation_id)
        );
        assert!(!paths.product().exists());
        assert!(host.lifecycle.snapshot().applied.is_none());

        // The higher Profile TCC coordinator restores the prior Core baseline
        // before calling Discard on an Aborted source. Discard itself must be
        // strictly a staged-file cleanup and never a publication/reapply.
        host.discard_profile_runtime(&operation_id).await;
        assert!(host.pending_profile_candidates.lock().await.is_empty());
        assert!(!paths.product().exists());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert!(
            host.accepted_runtime_receipt().is_some(),
            "Discard does not falsely claim a restored Core; TCC must restore baseline"
        );
        drop(candidate);
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_profile_tcc_try_rejects_mismatched_source_before_core_submit() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let (mut prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let endpoint = recording_endpoint(true);
        let operation_id = OperationId::generate();
        prepared.target = Some("source-changed-after-prepare".into());
        let result = host
            .reconcile_with_profile_source(
                None,
                prepared.snapshot.target_core,
                &endpoint,
                None,
                Some("older-source".into()),
                Some(operation_id),
                Some((prepared, spec)),
                None,
            )
            .await;
        assert!(result.unwrap_err().to_string().contains("target"));
        assert!(endpoint.submitted.lock().unwrap().is_empty());
        assert!(host.pending_profile_candidates.lock().await.is_empty());
        assert!(!paths.product().exists());
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn checked_preparation_and_core_owner_share_revision_and_confirmed_port_session() {
        use chimera_config::clash::config::ClashConfig;
        let (_root, host, _paths) = runtime_host_fixture().await;
        let checked_session = host.preparation_session();
        let clash = ClashConfig::default();
        let before = checked_session.ports.resolve_candidate(&clash).unwrap();
        assert!(
            host.accepted_runtime_receipt().is_none(),
            "a candidate must remain inert"
        );
        let revision = checked_session.revisions.allocate().unwrap();
        let (mut prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        prepared.snapshot = Arc::new(prepared.snapshot.with_revision(revision));
        prepared.ports = before.clone();
        let endpoint = recording_endpoint(true);
        host.reconcile_prepared(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .unwrap();

        // The Core receipt must contain the exact prepared picks and revision,
        // rather than a second generator's independently allocated values.
        let receipt = host.accepted_runtime_receipt().unwrap();
        assert_eq!(receipt.revision, revision);
        assert_eq!(receipt.ports, before);
        assert!(Arc::ptr_eq(
            receipt.artifact.as_ref().unwrap(),
            &prepared.snapshot
        ));
        let future_session = host.preparation_session();
        assert!(
            Arc::ptr_eq(&checked_session.ports, &future_session.ports),
            "the checked builder must share the actual Core host resolver Arc"
        );
        let next = future_session.ports.resolve_candidate(&clash).unwrap();
        assert_eq!(
            next, before,
            "unchanged strategy must reuse the confirmed running listeners"
        );
        let revision_two = future_session.revisions.allocate().unwrap();
        assert_eq!(revision_two.get(), revision.get() + 1);
        let revision_three = host.lifecycle.allocate_revision().unwrap();
        assert_eq!(
            revision_three.get(),
            revision_two.get() + 1,
            "Core-host allocations and checked builder allocations share one counter"
        );
        // A confirmed listener baseline belongs to the previous Core host.
        // After S2 owner switching it must be invalidated for *all* session
        // borrowers, without creating a new resolver or resetting revisions.
        host.select_execution_host(ExecutionHost::Service).unwrap();
        assert!(host.accepted_runtime_receipt().is_none());
        assert!(Arc::ptr_eq(
            &host.preparation_session().ports,
            &checked_session.ports
        ));
        assert_eq!(
            checked_session.revisions.allocate().unwrap().get(),
            revision_three.get() + 1
        );
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn checked_runtime_submits_exact_prevalidated_bytes_and_publishes_same_snapshot() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let bytes: Arc<str> = "mode: rule\nexternal-controller: 127.0.0.1:9090\n".into();
        let (prepared, spec) = prepared_runtime_fixture(bytes.clone());
        let snapshot = prepared.snapshot.clone();
        let expected_intent = prepared.intent.clone();
        let endpoint = recording_endpoint(true);
        assert!(host.confirmed_runtime_receipt().is_none());

        let replaced = host
            .reconcile_prepared_outcome(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .unwrap();
        assert!(replaced, "Started is a confirmed Core replacement");

        let submits = endpoint.submitted.lock().unwrap();
        assert_eq!(submits.len(), 1);
        assert_eq!(submits[0].0, expected_intent.config_text.as_bytes());
        assert_eq!(submits[0].1, Some(expected_intent.local_ipc));
        assert_eq!(submits[0].2, Some((ExecutionHost::Local, 3)));
        drop(submits);
        let receipt = host.confirmed_runtime_receipt().unwrap();
        assert_eq!(receipt.config_digest, expected_intent.digest);
        assert_eq!(receipt.target.as_deref(), Some("committed-source-42"));
        assert!(Arc::ptr_eq(receipt.artifact.as_ref().unwrap(), &snapshot));
        assert!(Arc::ptr_eq(
            host.lifecycle.snapshot().applied.as_ref().unwrap(),
            &snapshot
        ));
        assert_eq!(
            tokio::fs::read(paths.product()).await.unwrap(),
            bytes.as_bytes()
        );
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_core_rollback_stays_typed_and_never_publishes_candidate() {
        use crate::client::core_lifecycle::ports::{PreparedRolledBack, PreparedSubmission};

        let (_root, host, _paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let mut endpoint = recording_endpoint(true);
        endpoint.rolled_back = true;

        let terminal = host
            .reconcile_prepared_submission(
                &prepared,
                spec.clone(),
                &endpoint,
                (ExecutionHost::Local, 3),
            )
            .await
            .expect("rollback is an observed Core terminal, not an uncertain error");
        let PreparedSubmission::RolledBack(report) = terminal else {
            panic!("the Core restored its old revision, never accepted the candidate");
        };
        assert_eq!(report.outcome, ReconcileOutcomeKind::RolledBack);
        assert_eq!(
            report.failed_apply.as_deref(),
            Some("candidate rejected; previous runtime restored")
        );
        assert_eq!(
            report.revision.generation, 2,
            "the restored revision remains running"
        );
        assert!(host.lifecycle.confirmed_receipt().is_none());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert!(host.pending_runtime_product_target().is_none());
        assert!(!host.outcome_uncertain());
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);

        let rejected = host
            .reconcile_prepared_settlement(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .expect_err("the legacy single-result caller must refuse an observed rollback");
        let typed = rejected.downcast_ref::<PreparedRolledBack>().unwrap();
        assert_eq!(typed.report.outcome, ReconcileOutcomeKind::RolledBack);
        assert!(typed.reason.contains("previous runtime restored"));
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn prepared_core_unknown_retains_operation_identity_and_isolates_owner() {
        use crate::client::core_lifecycle::ports::PreparedSubmission;

        let (_root, host, _paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture(Arc::from("mode: rule\n"));
        let mut endpoint = recording_endpoint(false);
        endpoint.lose_result = true;
        let terminal = host
            .reconcile_prepared_submission(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .expect("the unobserved terminal is represented explicitly");
        let PreparedSubmission::Unknown(cause) = terminal else {
            panic!("a Running operation that never returns cannot be a refusal");
        };
        assert!(cause.operation_id.is_some());
        assert!(cause.message.contains("outcome could not be observed"));
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert!(host.outcome_uncertain());
        assert!(host.lifecycle.confirmed_receipt().is_none());
        assert!(host.pending_runtime_product_target().is_none());
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn confirmed_prepared_core_survives_product_promotion_failure_and_repairs_without_resubmit()
     {
        use crate::client::core_lifecycle::ports::PreparedProductPublication;

        let (_root, host, paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture("mode: rule\n".into());
        let expected_product = prepared.snapshot.clone();
        let endpoint = recording_endpoint(true);

        // A directory in place of the generated YAML product makes atomic
        // file promotion fail *after* the real Core endpoint accepted it.
        tokio::fs::create_dir_all(paths.product()).await.unwrap();
        let settled = host
            .reconcile_prepared_settlement(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .expect("the Core applied, even though publication failed");
        assert_eq!(settled.replaced, Some(true));
        settled.validate_identity("committed-source-42").unwrap();
        assert!(settled.validate_identity("an-obsolete-source").is_err());
        let mut forged = settled.clone();
        forged.product = Some(Arc::new((*expected_product).clone()));
        assert!(
            forged.validate_identity("committed-source-42").is_err(),
            "a copied product is not the exact accepted Core artifact"
        );
        assert!(matches!(
            settled.publication,
            PreparedProductPublication::Pending { .. }
        ));
        let accepted = settled
            .confirmed
            .expect("accepted Core receipt is not a publish receipt");
        assert!(Arc::ptr_eq(
            &accepted,
            &host.accepted_runtime_receipt().unwrap()
        ));
        assert!(Arc::ptr_eq(
            settled
                .product
                .as_ref()
                .expect("confirmed bytes are retained"),
            &expected_product
        ));
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert_eq!(
            host.pending_runtime_product_target().as_deref(),
            Some("committed-source-42")
        );
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert!(host.lifecycle.snapshot().promoted.is_none());

        tokio::fs::remove_dir_all(paths.product()).await.unwrap();
        assert!(
            host.republish_pending_runtime_product(&accepted)
                .await
                .unwrap()
        );
        assert_eq!(
            tokio::fs::read(paths.product()).await.unwrap(),
            expected_product.product_bytes()
        );
        assert_eq!(
            endpoint.submitted.lock().unwrap().len(),
            1,
            "publish recovery must not resubmit Core"
        );
        assert!(host.pending_runtime_product_target().is_none());
        assert!(Arc::ptr_eq(
            &host.lifecycle.snapshot().applied.unwrap(),
            &expected_product
        ));
        assert!(
            !host
                .republish_pending_runtime_product(&accepted)
                .await
                .unwrap()
        );
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn checked_runtime_patched_core_reports_no_instance_replacement() {
        let (_root, host, _paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture("mode: global\n".into());
        let mut endpoint = recording_endpoint(true);
        endpoint.patched = true;
        let replaced = host
            .reconcile_prepared_outcome(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .unwrap();
        assert!(!replaced, "Patched must retain the prior Core instance");
        assert!(host.confirmed_runtime_receipt().is_some());
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn checked_runtime_refuses_mismatched_intent_before_any_core_submission() {
        use chimera_core::actor_v2::intent::RuntimeIntentBuilder;
        let (_root, host, paths) = runtime_host_fixture().await;
        let (mut prepared, spec) = prepared_runtime_fixture("mode: rule\n".into());
        prepared.intent = Arc::new(RuntimeIntentBuilder::build(
            prepared.intent.core_type.clone(),
            Arc::from("mode: global\n"),
            prepared.intent.local_ipc,
        ));
        let endpoint = recording_endpoint(true);
        let error = host
            .reconcile_prepared(
                &prepared,
                spec.clone(),
                &endpoint,
                (ExecutionHost::Local, 3),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("intent does not match"));
        assert!(endpoint.submitted.lock().unwrap().is_empty());
        assert!(host.confirmed_runtime_receipt().is_none());
        assert!(!paths.product().exists());
        prepared.intent = Arc::new(RuntimeIntentBuilder::build(
            (&prepared.snapshot.target_core).into(),
            Arc::from("mode: rule\n"),
            prepared.intent.local_ipc,
        ));
        prepared.target = Some("committed-source-42".into());
        assert!(
            host.reconcile_prepared(
                &prepared,
                spec.clone(),
                &endpoint,
                (ExecutionHost::Service, 3)
            )
            .await
            .is_err(),
            "wrong Core host must fail before submitting"
        );
        prepared.target = None;
        assert!(
            host.reconcile_prepared(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
                .await
                .is_err()
        );
        assert!(endpoint.submitted.lock().unwrap().is_empty());
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn checked_runtime_requires_fresh_status_confirmation_not_just_submit_reply() {
        let (_root, host, paths) = runtime_host_fixture().await;
        let (prepared, spec) = prepared_runtime_fixture("mode: rule\n".into());
        let endpoint = recording_endpoint(false);
        let error = host
            .reconcile_prepared(&prepared, spec, &endpoint, (ExecutionHost::Local, 3))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("could not be confirmed"));
        assert_eq!(endpoint.submitted.lock().unwrap().len(), 1);
        assert!(host.lifecycle.accepted_confirmed_receipt().is_none());
        assert!(!paths.product().exists());
        assert!(host.outcome_uncertain());
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn restore_runtime_receipt_reapplies_the_exact_confirmed_bytes_and_settings() {
        let (_root, host, runtime_paths) = runtime_host_fixture().await;
        let config_text: Arc<str> = "mode: rule\nexternal-controller: 127.0.0.1:9090\n".into();
        let endpoint = recording_endpoint(true);
        let receipt = restore_receipt(config_text.clone());
        host.ports.confirm(&receipt.ports);

        host.restore_runtime_receipt(&endpoint, receipt.clone())
            .await
            .unwrap();

        let submitted = endpoint
            .submitted
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].0, config_text.as_bytes());
        assert_eq!(submitted[0].1, Some(receipt.local_ipc));
        drop(submitted);
        assert_eq!(
            host.ports.confirmed(),
            Some(receipt.ports.bindings().clone()),
            "only the verified restore may confirm its port binding"
        );
        assert_eq!(
            tokio::fs::read(runtime_paths.product()).await.unwrap(),
            config_text.as_bytes()
        );
        assert_eq!(
            host.confirmed_runtime_receipt().unwrap().config_digest,
            receipt.config_digest
        );
        host.shutdown().await.unwrap();
    }

    #[tokio::test]
    // Contract: a restore submission succeeds but the fresh status still
    // describes the candidate. The method must fail and make its old binding
    // unavailable; trusting the submission response alone fails this check.
    async fn restore_with_unconfirmed_status_invalidates_the_previous_port_binding() {
        let (_root, host, _runtime_paths) = runtime_host_fixture().await;
        let config_text: Arc<str> = "mode: rule\nexternal-controller: 127.0.0.1:9090\n".into();
        let endpoint = recording_endpoint(false);
        let receipt = restore_receipt(config_text);
        host.ports.confirm(&receipt.ports);
        assert!(host.ports.confirmed().is_some());

        let result = host.restore_runtime_receipt(&endpoint, receipt).await;

        assert!(
            result.is_err(),
            "stale status must not prove a successful restore"
        );
        assert_eq!(
            host.ports.confirmed(),
            None,
            "an unverified restore must not leave a port binding available"
        );
        host.shutdown().await.unwrap();
    }

    /// Contract: replay after Confirm consumed its staged file must recreate
    /// and publish the same product; discard still removes an unconfirmed file.
    #[tokio::test]
    async fn held_profile_candidate_confirmation_replays_idempotently_and_discard_cleans_it() {
        let root = tempfile::TempDir::new().unwrap();
        let paths = crate::utils::path::PathResolver::with_base_dirs(
            root.path().join("config"),
            root.path().join("data"),
        );
        let control = build_local_control(&paths).await;
        let runtime_paths = RuntimePaths::new(
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/clash.yaml"))
                .unwrap(),
            camino::Utf8PathBuf::from_path_buf(root.path().join("config/runtime/candidates"))
                .unwrap(),
        );
        let host = LocalRuntimeHost::new(control, runtime_paths.clone());
        let config_text: Arc<str> = "mode: rule\n".into();
        let config_bytes = config_text.as_bytes();
        let candidate_file = runtime_paths.create_candidate(config_bytes).await.unwrap();
        let candidate_path = candidate_file.path().to_path_buf();
        let operation_id = OperationId::generate();
        host.pending_profile_candidates
            .lock()
            .await
            .insert(operation_id.clone(), candidate_file);

        assert!(host.confirmed_runtime_receipt().is_none());
        assert!(host.lifecycle.snapshot().applied.is_none());
        assert!(host.lifecycle.snapshot().promoted.is_none());
        assert!(!runtime_paths.product().exists());

        let revision = host.lifecycle.allocate_revision().unwrap();
        let target_core = ClashCore::Mihomo;
        let product = Arc::new(RuntimeSnapshot::new_with_transform_output(
            revision,
            target_core,
            config_bytes.to_vec(),
            serde_yaml::Mapping::new(),
            PostProcessingOutput::default(),
        ));
        let receipt = Arc::new(RuntimeApplyReceipt {
            revision,
            config_text: config_text.clone(),
            config_digest: chimera_core_manager::payload_digest(config_bytes),
            target_core,
            core_spec: chimera_core_manager::CoreSpec {
                kind: CoreKind::Mihomo,
                binary_path: camino::Utf8PathBuf::from("/test/mihomo"),
                version: None,
                features: Vec::new(),
            },
            host: ExecutionHost::Local,
            run_intent: crate::client::application_workflow::policy::CoreRunIntent::Running,
            local_ipc: LocalIpcSettings {
                policy: LocalIpcPolicy::Disable,
                keep_http_controller: true,
            },
            instance_id: None,
            controller_generation: None,
            applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "candidate-source".into(),
                effective_hash: "candidate-effective".into(),
            },
            target: None,
            ports: Default::default(),
            artifact: Some(product.clone()),
        });
        // Prepare publishes the Core-confirmed receipt before the source
        // transaction reaches Confirm.
        host.lifecycle.publish_confirmed(receipt.clone());

        host.confirm_profile_runtime(
            &operation_id,
            crate::client::application_workflow::mutation::AppliedCandidate {
                replaced: true,
                receipt: receipt.clone(),
                product: product.clone(),
            },
        )
        .await
        .unwrap();

        assert!(!candidate_path.exists());
        assert_eq!(
            tokio::fs::read(runtime_paths.product()).await.unwrap(),
            config_bytes
        );
        assert_eq!(
            host.confirmed_runtime_receipt().unwrap().config_digest,
            receipt.config_digest
        );
        assert!(host.lifecycle.snapshot().applied.is_some());

        host.confirm_profile_runtime(
            &operation_id,
            crate::client::application_workflow::mutation::AppliedCandidate {
                replaced: true,
                receipt: receipt.clone(),
                product: product.clone(),
            },
        )
        .await
        .expect("replayed confirmation should recreate the consumed candidate");
        assert_eq!(
            tokio::fs::read(runtime_paths.product()).await.unwrap(),
            config_bytes,
            "replay must keep the committed product bytes"
        );
        assert!(host.lifecycle.snapshot().applied.is_some());

        let discard_id = OperationId::generate();
        let discarded = runtime_paths.create_candidate(config_bytes).await.unwrap();
        let discarded_path = discarded.path().to_path_buf();
        host.pending_profile_candidates
            .lock()
            .await
            .insert(discard_id.clone(), discarded);
        host.discard_profile_runtime(&discard_id).await;
        assert!(!discarded_path.exists());

        host.shutdown().await.unwrap();
    }

    // Contract: the IPC v2 rollback variant proves the old config stayed
    // active; other terminal reconcile outcomes may be projected as applied.
    #[test]
    fn reconcile_outcome_distinguishes_a_rollback_from_an_applied_runtime() {
        let revision = chimera_ipc::api::status::ConfigRevisionInfo {
            epoch: 1,
            generation: 1,
            source_hash: "source".into(),
            effective_hash: "effective".into(),
        };
        let rolled_back = ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::RolledBack,
            revision: revision.clone(),
            warning: None,
            failed_apply: Some("injected apply failure".into()),
        };
        let started = ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::Started,
            revision,
            warning: None,
            failed_apply: None,
        };
        assert!(accept_reconcile_outcome(&rolled_back).is_err());
        assert!(accept_reconcile_outcome(&started).is_ok());
        assert!(reconcile_replaced(&started));

        let patched = ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::Patched,
            revision: started.revision.clone(),
            warning: None,
            failed_apply: None,
        };
        assert!(!reconcile_replaced(&patched));
    }

    // Contract: a durability warning accompanies a terminal applied result.
    #[test]
    fn durability_warning_preserves_the_underlying_apply_result() {
        let outcome = ReconcileOutcomeInfo {
            outcome: ReconcileOutcomeKind::Started,
            revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 3,
                generation: 1,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
            },
            warning: Some("directory sync was not confirmed".into()),
            failed_apply: None,
        };

        assert!(accept_reconcile_outcome(&outcome).is_ok());
    }

    #[test]
    fn tracked_status_task_can_start_from_synchronous_setup() {
        let tasks = TaskTracker::new();
        let (started, started_rx) = std::sync::mpsc::channel();
        let (release, release_rx) = tokio::sync::oneshot::channel();
        spawn_tracked_on_tauri_runtime(&tasks, async move {
            started.send(()).expect("test receiver is alive");
            let _ = release_rx.await;
        });
        started_rx.recv().expect("tracked task should start");
        assert_eq!(tasks.len(), 1);
        tasks.close();
        let wait_is_pending = tauri::async_runtime::block_on(async {
            let mut wait = Box::pin(tasks.wait());
            std::future::poll_fn(|context| {
                std::task::Poll::Ready(wait.as_mut().poll(context).is_pending())
            })
            .await
        });
        assert!(wait_is_pending, "tracker must wait for its in-flight task");
        release.send(()).expect("tracked task is still waiting");
        tauri::async_runtime::block_on(tasks.wait());
    }
}
