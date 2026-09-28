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
use std::{collections::HashMap, fmt};

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

    pub(crate) fn local_endpoint(&self) -> super::control_endpoint::LocalEndpoint {
        super::control_endpoint::LocalEndpoint::new(self.control.clone())
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) fn confirmed_runtime_receipt(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        self.lifecycle.confirmed_receipt()
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(clash, target_core, endpoint, None, None)
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
    ) -> anyhow::Result<()> {
        self.reconcile_with_profile_source(
            clash,
            target_core,
            endpoint,
            Some((profiles, app, staged_content)),
            None,
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn prepare_profile_runtime(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        operation_id: OperationId,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        self.reconcile_with_profile_source(
            clash,
            target_core,
            endpoint,
            Some((profiles, app, staged_content)),
            Some(operation_id),
        )
        .await?
        .context("Profile runtime preparation returned no candidate")
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

        let resolved_ports = self.ports.resolve(&clash)?;
        let revision = self.lifecycle.allocate_revision()?;
        let output = match Config::generate_runtime_output_from_profiles(
            &clash,
            target_core,
            app,
            profiles,
            resolved_ports,
            staged_content,
        )
        .await
        {
            Ok(output) => output,
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
        clash: ClashConfig,
        target_core: ClashCore,
        endpoint: &dyn ControlEndpoint,
        profile_source: Option<(
            Arc<chimera_config::profile::Profiles>,
            chimera_config::application::ChimeraAppConfig,
            std::collections::BTreeMap<String, String>,
        )>,
        profile_operation_id: Option<OperationId>,
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

        let resolved_ports = self.ports.resolve(&clash)?;
        let revision = self.lifecycle.allocate_revision()?;
        let generated = match profile_source {
            Some((profiles, app, staged_content)) => {
                Config::generate_runtime_output_from_profiles(
                    &clash,
                    target_core,
                    app,
                    profiles,
                    resolved_ports.clone(),
                    staged_content,
                )
                .await
            }
            None => {
                Config::generate_runtime_output_with_ports(
                    &clash,
                    target_core,
                    resolved_ports.clone(),
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
        let local_ipc = LocalIpcSettings {
            policy: chimera_core_manager::LocalIpcPolicy::Disable,
            keep_http_controller: true,
        };
        let config_text: Arc<str> = String::from_utf8(config_bytes.clone())?.into();
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
            expected_owner: None,
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
            ports: resolved_ports,
            artifact: Some(snapshot.clone()),
        });

        *self.run_type.write() = match endpoint.host() {
            ExecutionHost::Local => RunType::Normal,
            ExecutionHost::Service => RunType::Service,
        };

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
        let candidate_file = self
            .pending_profile_candidates
            .lock()
            .await
            .remove(operation_id)
            .context("held Profile runtime candidate is unavailable at Confirm")?;
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
        if let Err(error) = self.lifecycle.publish_applied(product) {
            return Err(error).context(
                "the core applied the committed Profile runtime, but Chimera could not publish its applied snapshot",
            );
        }
        Config::runtime().apply();
        Ok(())
    }

    pub(crate) async fn restore_runtime_receipt(
        &self,
        endpoint: &dyn ControlEndpoint,
        receipt: Arc<RuntimeApplyReceipt>,
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
            expected_owner: None,
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
                return Err(error);
            }
        };

        let applied_status = match endpoint.status().await {
            Ok(status) => status,
            Err(error) => {
                cleanup_candidate(candidate, "could not confirm restored runtime").await;
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
        if let Err(error) = self.lifecycle.publish_applied(snapshot) {
            self.outcome_uncertain.store(true, Ordering::Release);
            return Err(error).context(
                "the core restored the runtime, but Chimera could not publish its applied snapshot",
            );
        }
        Config::runtime().apply();
        Ok(())
    }

    async fn submit_and_wait(
        &self,
        endpoint: &dyn ControlEndpoint,
        submission: CoreSubmission,
        operation_id: OperationId,
    ) -> anyhow::Result<ReconcileOutcomeInfo> {
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
                accept_reconcile_outcome(&outcome)?;
                Ok(outcome)
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

    struct RecordingEndpoint {
        status: StdMutex<super::super::control_endpoint::CoreStatusSnapshot>,
        submitted: StdMutex<Vec<(Vec<u8>, Option<LocalIpcSettings>)>>,
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
                .push((bytes, request.options.local_ipc));

            let generation = {
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
            };
            let revision = ConfigRevisionInfo {
                epoch: 1,
                generation,
                source_hash: digest,
                effective_hash: format!("effective-{generation}"),
            };
            Ok(OperationInfo {
                id: operation_id.to_string(),
                phase: OperationPhase::Succeeded,
                output: Some(OperationOutputInfo::Reconciled(ReconcileOutcomeInfo {
                    outcome: ReconcileOutcomeKind::Started,
                    revision,
                    warning: None,
                    failed_apply: None,
                })),
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

    #[tokio::test]
    async fn restore_runtime_receipt_reapplies_the_exact_confirmed_bytes_and_settings() {
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
        let host = LocalRuntimeHost::new(control, runtime_paths.clone());
        let status = super::super::control_endpoint::CoreStatusSnapshot {
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
        };
        let endpoint = RecordingEndpoint {
            status: StdMutex::new(status),
            submitted: StdMutex::new(Vec::new()),
        };
        let config_text: Arc<str> = "mode: rule\nexternal-controller: 127.0.0.1:9090\n".into();
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
        let receipt = Arc::new(RuntimeApplyReceipt {
            revision: runtime_revision,
            config_digest: chimera_core_manager::payload_digest(config_text.as_bytes()),
            config_text: config_text.clone(),
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
            applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "candidate-digest".into(),
                effective_hash: "old-effective".into(),
            },
            ports: Default::default(),
            artifact: Some(artifact),
        });

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
    async fn held_profile_candidate_is_published_only_by_confirm_and_discard_cleans_it() {
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
            applied_revision: chimera_ipc::api::status::ConfigRevisionInfo {
                epoch: 1,
                generation: 1,
                source_hash: "candidate-source".into(),
                effective_hash: "candidate-effective".into(),
            },
            ports: Default::default(),
            artifact: Some(product.clone()),
        });

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
}
