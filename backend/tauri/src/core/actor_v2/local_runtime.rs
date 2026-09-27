//! Chimera runtime generation and snapshot adapter shared by reference-aligned
//! Local and Service control endpoints.
//!
//! App config conversion and the legacy application projection stay at the
//! Tauri boundary; process, epoch, health, and config transactions belong to
//! the selected endpoint and `chimera-core-manager`.

use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, bail};
use chimera_config::clash::config::ClashConfig;
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

use crate::{
    client::{
        RuntimePaths,
        runtime::{RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransformFailure},
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
};

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
    ports: crate::client::SessionPortResolver,
    lifecycle: crate::client::runtime::RuntimeLifecycle,
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
    pub(crate) fn new(control: CoreControl, runtime_paths: RuntimePaths) -> Self {
        let recovery_notify = Arc::new(tokio::sync::Notify::new());
        let mut status = control.subscribe();
        let recovery_signal = recovery_notify.clone();
        tauri::async_runtime::spawn(async move {
            let mut previous = status.borrow().state.clone();
            while status.changed().await.is_ok() {
                let current = status.borrow_and_update().state.clone();
                if matches!(previous, CoreState::Restarting { .. })
                    && matches!(current, CoreState::Running { .. })
                {
                    recovery_signal.notify_one();
                }
                previous = current;
            }
        });
        Self {
            control,
            runtime_paths,
            ports: crate::client::SessionPortResolver::default(),
            lifecycle: crate::client::runtime::RuntimeLifecycle::default(),
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

    pub(crate) fn local_endpoint(&self) -> super::control_endpoint::LocalEndpoint {
        super::control_endpoint::LocalEndpoint::new(self.control.clone())
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(clash, target_core, endpoint, None)
            .await
    }

    pub(crate) async fn reconcile_with_profiles(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(
            clash,
            target_core,
            endpoint,
            Some((profiles, app, staged_content)),
        )
        .await
    }

    async fn reconcile_with_profile_source(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profile_source: Option<(
            Arc<chimera_config::profile::Profiles>,
            chimera_config::application::ChimeraAppConfig,
            std::collections::BTreeMap<String, String>,
        )>,
    ) -> anyhow::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            bail!("the local core control plane is shutting down");
        }
        if self.outcome_uncertain() {
            bail!(
                "the local core runtime outcome is uncertain; restart Chimera before applying another runtime"
            );
        }

        let resolved_ports = self.ports.resolve(&clash)?;
        let revision = self.lifecycle.allocate_revision()?;
        let generated = match profile_source {
            Some((profiles, app, staged_content)) => {
                Config::generate_runtime_output_from_profiles(
                    &clash,
                    target_core,
                    app,
                    profiles,
                    resolved_ports,
                    staged_content,
                )
                .await
            }
            None => {
                Config::generate_runtime_output_with_ports(&clash, target_core, resolved_ports)
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
                if let Some(transform) =
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
        let digest = chimera_core_manager::payload_digest(&config_bytes);
        let candidate = self.runtime_paths.create_candidate(&config_bytes).await?;
        let staged_config = if endpoint.host() == ExecutionHost::Service {
            match camino::Utf8PathBuf::from_path_buf(candidate.path().to_path_buf()) {
                Ok(path) => Some(path),
                Err(path) => {
                    cleanup_candidate(candidate, "service check path is not UTF-8").await;
                    bail!(
                        "service config check path is not valid UTF-8: {}",
                        path.to_string_lossy()
                    );
                }
            }
        } else {
            None
        };
        match endpoint
            .check_config(CheckSubmission {
                core_spec: core_spec.clone(),
                core_type: (&target_core).into(),
                config_bytes: config_bytes.clone(),
                digest: digest.clone(),
                staged_config,
            })
            .await
        {
            CheckSupport::Ran(Ok(())) => {}
            CheckSupport::Ran(Err(error)) => {
                cleanup_candidate(candidate, "advisory check failed").await;
                return Err(error.into());
            }
            CheckSupport::Unsupported { reason } => {
                cleanup_candidate(candidate, "advisory check is unsupported").await;
                bail!("core config check is unavailable: {reason}");
            }
        }

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
            expected_owner: None,
            envelope: CoreCommandEnvelope {
                operation_id,
                command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                    core: core_spec,
                    config: ConfigInput::Inline {
                        bytes: config_bytes,
                        expected_digest: Some(digest),
                    },
                    options: InstanceOptions {
                        local_ipc: Some(LocalIpcSettings {
                            policy: chimera_core_manager::LocalIpcPolicy::Disable,
                            keep_http_controller: true,
                        }),
                        ..InstanceOptions::default()
                    },
                    expected_applied,
                })),
            },
            core_type: Some((&target_core).into()),
        };
        if let Err(error) = self
            .submit_and_wait(endpoint, submission, operation_id)
            .await
        {
            cleanup_candidate(candidate, "runtime operation failed").await;
            return Err(error);
        }

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
                    "the core applied the runtime, but Chimera could not promote its runtime product",
                );
            }
        };
        if let Err(error) = cleanup {
            tracing::warn!(%error, "failed to clean a promoted runtime candidate");
        }

        let snapshot = Arc::new(RuntimeSnapshot::from_data(
            revision,
            target_core,
            product_bytes.into(),
            RuntimeSnapshotData {
                config,
                exists_keys,
                postprocessing_output: transform_output,
                inspection: Arc::new(inspection),
            },
        ));
        self.lifecycle.publish_promoted(snapshot.clone());
        self.lifecycle.publish_applied(snapshot)?;
        Config::runtime().apply();
        Ok(())
    }

    async fn submit_and_wait(
        &self,
        endpoint: &dyn ControlEndpoint,
        submission: CoreSubmission,
        operation_id: OperationId,
    ) -> anyhow::Result<()> {
        let mut operation = match endpoint.submit(submission).await {
            Ok(operation) => operation,
            Err(error) => {
                if endpoint.host() == ExecutionHost::Service
                    && (error.retryable
                        || error.kind.is_some_and(|kind| {
                            matches!(
                                kind,
                                CoreErrorKind::BackendUnavailable | CoreErrorKind::Internal
                            )
                        }))
                {
                    self.outcome_uncertain.store(true, Ordering::Release);
                }
                return Err(error.into());
            }
        };

        if operation.id != operation_id.to_string() {
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
                    self.outcome_uncertain.store(true, Ordering::Release);
                    bail!("core reconcile outcome could not be observed");
                }
            };
        }
        if operation.id != operation_id.to_string() {
            self.outcome_uncertain.store(true, Ordering::Release);
            bail!("core host returned a different reconcile operation id");
        }

        match (operation.phase, operation.output) {
            (OperationPhase::Succeeded, Some(OperationOutputInfo::Reconciled(outcome))) => {
                accept_reconcile_outcome(&outcome)
            }
            (OperationPhase::Failed, _) => bail!(
                "core reconcile failed: {}",
                operation
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "core host returned no failure detail".into())
            ),
            (OperationPhase::Queued | OperationPhase::Running, _) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                bail!("core reconcile is still running; its outcome is uncertain");
            }
            (OperationPhase::Succeeded, output) => {
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
            Ok(OperationOutput::Stopped) => Ok(()),
            Ok(output) => {
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
                info.server = url.to_string();
                info.port = url.port_or_known_default().unwrap_or(info.port);
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let control = local_host::build(&paths).await.unwrap();
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
}
