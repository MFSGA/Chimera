//! Chimera application adapter for the reference `CoreControl` host.
//!
//! Runtime generation and the legacy application projection stay at the
//! Tauri boundary; process, epoch, health, and config transactions belong to
//! `chimera-core-manager`.

use std::fmt;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, bail};
use chimera_config::clash::config::ClashConfig;
use chimera_core_manager::{
    ApplyOutcome, CheckRequest, ConfigInput, CoreCommand, CoreCommandEnvelope, CoreControl,
    CoreState, InstanceOptions, OperationId, OperationOutput, ReconcileRequest,
    spec::LocalIpcSettings,
};
use parking_lot::RwLock;

use crate::{
    client::{
        RuntimePaths,
        runtime::{RuntimeSnapshot, RuntimeSnapshotData, RuntimeTransformFailure},
    },
    config::{chimera::ClashCore, clash::ClashInfo, core::Config},
    core::{
        actor_v2::{CoreStatusSnapshot, local_host},
        clash::core::RunType,
    },
    enhance::PostProcessingOutput,
};

/// Owns the local runtime adapter around the shared control plane.
///
/// `control` is the only process lifecycle owner. `lifecycle` keeps the
/// application-facing transformed snapshot, and `ports` preserves the
/// session's existing port-pick behavior while config compilation remains in
/// the Chimera runtime pipeline.
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

    pub(crate) fn set_run_type(&self, run_type: RunType) {
        *self.run_type.write() = run_type;
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
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
        let (config, exists_keys, transform_output, inspection) =
            match Config::generate_runtime_output_with_ports(&clash, target_core, resolved_ports)
                .await
            {
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
        if let Err(error) = self
            .control
            .check(CheckRequest {
                core: core_spec.clone(),
                config: ConfigInput::Inline {
                    bytes: config_bytes.clone(),
                    expected_digest: Some(digest.clone()),
                },
            })
            .await
        {
            cleanup_candidate(candidate, "advisory check failed").await;
            return Err(error.into());
        }

        let envelope = CoreCommandEnvelope {
            operation_id: OperationId::generate(),
            command: CoreCommand::Reconcile(Box::new(ReconcileRequest {
                core: core_spec,
                config: ConfigInput::Inline {
                    bytes: config_bytes.clone(),
                    expected_digest: Some(digest),
                },
                options: InstanceOptions {
                    local_ipc: Some(LocalIpcSettings {
                        policy: chimera_core_manager::LocalIpcPolicy::Disable,
                        keep_http_controller: true,
                    }),
                    ..InstanceOptions::default()
                },
                expected_applied: None,
            })),
        };
        let handle = match self.control.submit(envelope) {
            Ok(handle) => handle,
            Err(error) => {
                cleanup_candidate(candidate, "operation admission failed").await;
                return Err(error.into());
            }
        };
        let operation_id = handle.id();
        let output = match handle.wait().await {
            Ok(output) => output,
            Err(error) => {
                cleanup_candidate(candidate, "runtime operation failed").await;
                if error.kind.is_some_and(|kind| {
                    matches!(
                        kind,
                        chimera_core_manager::CoreErrorKind::ApplyRollbackFailed
                            | chimera_core_manager::CoreErrorKind::StopUnconfirmed
                            | chimera_core_manager::CoreErrorKind::Quarantined
                    )
                }) || self.control.executor_is_closed()
                {
                    self.outcome_uncertain.store(true, Ordering::Release);
                }
                return Err(error.into());
            }
        };
        let outcome = match output {
            OperationOutput::Reconciled(outcome) => outcome,
            unexpected => {
                cleanup_candidate(candidate, "operation returned an unexpected result").await;
                self.outcome_uncertain.store(true, Ordering::Release);
                bail!("core operation {operation_id} returned unexpected output: {unexpected:?}");
            }
        };
        if let Err(error) = accept_apply_outcome(&outcome) {
            cleanup_candidate(candidate, "runtime apply was rolled back").await;
            return Err(error);
        }

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
        *self.run_type.write() = RunType::Normal;
        Ok(())
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

fn accept_apply_outcome(outcome: &ApplyOutcome) -> anyhow::Result<()> {
    match outcome {
        ApplyOutcome::RolledBack { failed_apply, .. } => {
            bail!("core runtime apply was rolled back: {failed_apply}");
        }
        ApplyOutcome::DurabilityUncertain { outcome, warning } => {
            tracing::warn!(%warning, "core runtime applied with uncertain disk durability");
            accept_apply_outcome(outcome)
        }
        ApplyOutcome::Started { .. }
        | ApplyOutcome::Noop { .. }
        | ApplyOutcome::Patched { .. }
        | ApplyOutcome::Reloaded { .. }
        | ApplyOutcome::Restarted { .. }
        | ApplyOutcome::Switched { .. } => Ok(()),
    }
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

    // Contract: an explicit rollback proves the old config stayed active;
    // successful start outcomes may be projected as applied. The wrong
    // acceptance rule would publish a rolled-back document as current.
    #[test]
    fn apply_outcome_distinguishes_a_rollback_from_an_applied_runtime() {
        let rolled_back = ApplyOutcome::RolledBack {
            revision: chimera_core_manager::ConfigRevision {
                epoch: chimera_core_manager::Epoch::new(1).unwrap(),
                generation: 1,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
                runtime_path: camino::Utf8PathBuf::from("/runtime/config.yaml"),
            },
            failed_apply: "injected apply failure".into(),
        };
        assert!(accept_apply_outcome(&rolled_back).is_err());

        let started = ApplyOutcome::Started {
            revision: chimera_core_manager::ConfigRevision {
                epoch: chimera_core_manager::Epoch::new(2).unwrap(),
                generation: 1,
                source_hash: "source".into(),
                effective_hash: "effective".into(),
                runtime_path: camino::Utf8PathBuf::from("/runtime/config.yaml"),
            },
        };
        assert!(accept_apply_outcome(&started).is_ok());
    }

    // Contract: a durability warning wraps the core's apply result. It must
    // keep the applied result visible while retaining the warning in logs.
    #[test]
    fn durability_warning_preserves_the_underlying_apply_result() {
        let outcome = ApplyOutcome::DurabilityUncertain {
            outcome: Box::new(ApplyOutcome::Started {
                revision: chimera_core_manager::ConfigRevision {
                    epoch: chimera_core_manager::Epoch::new(3).unwrap(),
                    generation: 1,
                    source_hash: "source".into(),
                    effective_hash: "effective".into(),
                    runtime_path: camino::Utf8PathBuf::from("/runtime/config.yaml"),
                },
            }),
            warning: "directory sync was not confirmed".into(),
        };

        assert!(accept_apply_outcome(&outcome).is_ok());
    }
}
