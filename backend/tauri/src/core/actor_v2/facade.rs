//! Control helpers owned by the lower core-host boundary.
//!
//! This is the staged Chimera counterpart of ref `core/actor_v2/facade.rs`.
//! Runtime intent is prepared before endpoint admission, CAS is read from the
//! target endpoint, and mutations route through explicit Local/Service handles.
//! Local still wraps the legacy CoreManager; Service core control is daemon-v2.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use super::{
    endpoint::{
        CoreCommand, CoreStatusSnapshot, EndpointHandle, EndpointRegistry, ExecutionHost,
        OperationPhase,
    },
    intent::RuntimeIntentBuilder,
    service_actor::{OsServiceHostAdapter, ServiceClient},
};
use crate::{
    client::runtime::{
        RuntimeDocument, RuntimeRevisionAllocator, RuntimeSnapshot, RuntimeTransformFailure,
    },
    config::{chimera::ClashCore, clash::ClashInfo},
    core::{
        clash::{
            api::ApiClient,
            core::{RunType, RuntimeRestartError},
        },
        connection_interruption::ConnectionInterruptionService,
    },
    enhance::PostProcessingOutput,
};
use anyhow::Context;
use chimera_config::clash::config::ClashConfig;

const SERVICE_RESTART_BUDGET: u8 = 3;
const LOCAL_OPERATION_WAIT: Duration = Duration::from_secs(60);
const ACTIVE_HOST_LOCAL: u8 = 0;
const ACTIVE_HOST_SERVICE: u8 = 1;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct LowerOperationError {
    operation_id: u64,
    message: String,
    error_kind: Option<String>,
    retryable: bool,
}

impl LowerOperationError {
    pub(crate) fn new(operation_id: u64, message: impl Into<String>) -> Self {
        Self {
            operation_id,
            message: message.into(),
            error_kind: None,
            retryable: false,
        }
    }

    pub(crate) fn classified(
        operation_id: u64,
        message: impl Into<String>,
        error_kind: Option<String>,
        retryable: bool,
    ) -> Self {
        Self {
            operation_id,
            message: message.into(),
            error_kind,
            retryable,
        }
    }

    pub(crate) fn core_error_kind(&self) -> Option<chimera_ipc::api::CoreErrorKind> {
        self.error_kind
            .as_deref()
            .and_then(chimera_ipc::api::CoreErrorKind::from_wire)
    }

    pub(crate) fn retryable(&self) -> bool {
        self.retryable
    }
}

pub(crate) fn operation_id_from_error(error: &anyhow::Error) -> Option<u64> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<LowerOperationError>())
        .map(|error| error.operation_id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceUninstallPlan {
    AlreadyAbsent,
    Uninstall,
    StopThenUninstall,
}

fn service_uninstall_plan(
    status: chimera_ipc::types::ServiceStatus,
    core_state: Option<&chimera_ipc::api::status::CoreState>,
) -> anyhow::Result<ServiceUninstallPlan> {
    use chimera_ipc::{api::status::CoreState, types::ServiceStatus};

    match status {
        ServiceStatus::NotInstalled => Ok(ServiceUninstallPlan::AlreadyAbsent),
        ServiceStatus::Stopped => Ok(ServiceUninstallPlan::Uninstall),
        ServiceStatus::Running => match core_state {
            Some(CoreState::Stopped(_)) => Ok(ServiceUninstallPlan::StopThenUninstall),
            Some(CoreState::Running) => anyhow::bail!(
                "the service daemon still owns a running core; hand off to Local before uninstall"
            ),
            None => anyhow::bail!(
                "the service daemon is running but core ownership cannot be determined; stop it before uninstall"
            ),
        },
    }
}

pub(crate) struct CoreFacade {
    endpoints: EndpointRegistry,
    runtime_preparation: crate::core::clash::core::RuntimePreparation,
    active_host: AtomicU8,
    active_host_known: AtomicBool,
    service_runtime_observation: parking_lot::RwLock<Option<Arc<RuntimeSnapshot>>>,
    service_observation_revisions: RuntimeRevisionAllocator,
    outcome_uncertain: Arc<AtomicBool>,
    service_client: tokio::sync::OnceCell<ServiceClient>,
    service_restart_attempts: Arc<AtomicU8>,
    service_restart_exhausted: Arc<AtomicBool>,
}

pub(crate) struct ServiceTransition {
    _guard: tokio::sync::MutexGuard<'static, ()>,
    client: ServiceClient,
}

impl CoreFacade {
    pub(crate) fn new_local(
        service_core_client: chimera_ipc::client::shortcuts::Client<'static>,
    ) -> Self {
        let (endpoints, runtime_preparation) = EndpointRegistry::new(service_core_client);
        let active_host = match RunType::default() {
            RunType::Service => ACTIVE_HOST_SERVICE,
            RunType::Normal | RunType::Elevated => ACTIVE_HOST_LOCAL,
        };
        Self {
            endpoints,
            runtime_preparation,
            active_host: AtomicU8::new(active_host),
            active_host_known: AtomicBool::new(false),
            service_runtime_observation: parking_lot::RwLock::new(None),
            service_observation_revisions: RuntimeRevisionAllocator::default(),
            outcome_uncertain: Arc::new(AtomicBool::new(false)),
            service_client: tokio::sync::OnceCell::new(),
            service_restart_attempts: Arc::new(AtomicU8::new(0)),
            service_restart_exhausted: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn outcome_uncertain(&self) -> bool {
        self.outcome_uncertain.load(Ordering::Acquire)
    }

    pub(crate) fn operation_info(&self, id: u64) -> Option<super::endpoint::OperationInfo> {
        self.endpoints
            .local()
            .operation_info(super::endpoint::OperationId::from_raw(id))
    }

    pub(crate) fn operation_history(&self) -> Vec<super::endpoint::OperationInfo> {
        self.endpoints.local().operation_history()
    }

    async fn service_client(&self) -> anyhow::Result<&ServiceClient> {
        let outcome_uncertain = self.outcome_uncertain.clone();
        let restart_attempts = self.service_restart_attempts.clone();
        let restart_exhausted = self.service_restart_exhausted.clone();
        self.service_client
            .get_or_try_init(|| async move {
                ServiceClient::spawn(
                    Arc::new(OsServiceHostAdapter),
                    outcome_uncertain,
                    restart_attempts,
                    restart_exhausted,
                    SERVICE_RESTART_BUDGET,
                )
                .await
            })
            .await
    }

    fn endpoint_for_run_type(&self, run_type: RunType) -> EndpointHandle {
        self.endpoints.endpoint(match run_type {
            RunType::Service => ExecutionHost::Service,
            RunType::Normal | RunType::Elevated => ExecutionHost::Local,
        })
    }

    fn active_host(&self) -> ExecutionHost {
        match self.active_host.load(Ordering::Acquire) {
            ACTIVE_HOST_SERVICE => ExecutionHost::Service,
            _ => ExecutionHost::Local,
        }
    }

    fn set_active_host(&self, host: ExecutionHost) {
        self.active_host.store(
            match host {
                ExecutionHost::Local => ACTIVE_HOST_LOCAL,
                ExecutionHost::Service => ACTIVE_HOST_SERVICE,
            },
            Ordering::Release,
        );
        self.active_host_known.store(true, Ordering::Release);
    }

    fn service_runtime_observation(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.service_runtime_observation.read().clone()
    }

    fn publish_service_runtime_observation(&self, document: RuntimeDocument) -> anyhow::Result<()> {
        let snapshot =
            Arc::new(document.into_snapshot(self.service_observation_revisions.allocate()?));
        *crate::config::core::Config::runtime().draft() = crate::config::runtime::IRuntime {
            config: Some(snapshot.config.clone()),
        };
        crate::config::core::Config::runtime().apply();
        *self.service_runtime_observation.write() = Some(snapshot);
        Ok(())
    }

    async fn active_endpoint(&self) -> anyhow::Result<EndpointHandle> {
        let preferred_host = self.active_host();
        let preferred = self.endpoints.endpoint(preferred_host);
        if self.active_host_known.load(Ordering::Acquire) {
            return Ok(preferred);
        }
        let preferred_status = preferred.status().await?;
        if matches!(
            preferred_status.state,
            chimera_ipc::api::status::CoreState::Running
        ) {
            self.set_active_host(preferred_host);
            return Ok(preferred);
        }

        let alternate_host = match preferred_host {
            ExecutionHost::Local => ExecutionHost::Service,
            ExecutionHost::Service => ExecutionHost::Local,
        };
        let alternate = self.endpoints.endpoint(alternate_host);
        match alternate.status().await {
            Ok(status) if matches!(status.state, chimera_ipc::api::status::CoreState::Running) => {
                self.set_active_host(alternate_host);
                Ok(alternate)
            }
            _ => Ok(preferred),
        }
    }

    async fn run_mutation(
        &self,
        endpoint: EndpointHandle,
        command: CoreCommand,
        local_document: Option<RuntimeDocument>,
    ) -> anyhow::Result<super::endpoint::OperationOutput> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        let submission = self.endpoints.submission(command, local_document);
        let admitted = match endpoint.submit(submission).await {
            Ok(admitted) => admitted,
            Err(error) if crate::core::service::core_host::is_outcome_uncertain(&error) => {
                self.outcome_uncertain.store(true, Ordering::Release);
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        let id = admitted.id;
        match endpoint.wait_operation(id, LOCAL_OPERATION_WAIT).await {
            Some(info) if info.phase == OperationPhase::Succeeded => info.output.ok_or_else(|| {
                anyhow::anyhow!(
                    "lower core operation {} succeeded without a typed output",
                    id.get()
                )
            }),
            Some(info) if info.phase == OperationPhase::Failed => {
                let error = LowerOperationError::classified(
                    id.get(),
                    info.error.unwrap_or_else(|| {
                        "lower core operation failed without an error".to_string()
                    }),
                    info.error_kind,
                    info.retryable,
                );
                if let Some(kind) = error.core_error_kind() {
                    tracing::debug!(
                        operation_id = id.get(),
                        %kind,
                        retryable = error.retryable(),
                        "lower core operation failed with a typed classification"
                    );
                }
                Err(anyhow::Error::new(error))
            }
            Some(info) if info.phase == OperationPhase::Uncertain => {
                self.outcome_uncertain.store(true, Ordering::Release);
                Err(anyhow::Error::new(LowerOperationError::new(
                    id.get(),
                    info.error.unwrap_or_else(|| {
                        "lower core operation reached an uncertain terminal state".to_string()
                    }),
                )))
            }
            Some(_) | None => {
                self.outcome_uncertain.store(true, Ordering::Release);
                Err(anyhow::Error::new(LowerOperationError::new(
                    id.get(),
                    format!(
                        "lower core operation {} did not reach a terminal state within the wait budget",
                        id.get()
                    ),
                )))
            }
        }
    }

    pub(crate) async fn reconcile(
        &self,
        clash: ClashConfig,
        profiles: crate::config::profile::profiles::Profiles,
        target_core: ClashCore,
        run_type: RunType,
        enable_builtin_enhanced: bool,
    ) -> anyhow::Result<()> {
        let local = self.endpoints.local();
        let prepared_ports = self.runtime_preparation.prepare_ports(&clash)?;
        let document = match self
            .runtime_preparation
            .build_runtime_document(
                target_core,
                &clash,
                &profiles,
                prepared_ports.bindings().clone(),
                enable_builtin_enhanced,
            )
            .await
        {
            Ok(document) => {
                self.runtime_preparation.clear_prepare_failure();
                document
            }
            Err(error) => {
                self.runtime_preparation.record_prepare_failure(&error)?;
                return Err(RuntimeRestartError::Prepare(error).into());
            }
        };
        let endpoint = self.endpoint_for_run_type(run_type);
        let target_host = endpoint.host();
        debug_assert_eq!(
            target_host,
            if run_type == RunType::Service {
                ExecutionHost::Service
            } else {
                ExecutionHost::Local
            }
        );

        let result = async {
            let expected_applied = endpoint.status().await?.applied;

            let active = self.active_endpoint().await?;
            if active.host() != target_host {
                let active_status = active.status().await?;
                if matches!(
                    active_status.state,
                    chimera_ipc::api::status::CoreState::Running
                ) {
                    let _ = self.run_mutation(active, CoreCommand::Stop, None).await?;
                }
                // Ref handoff is commit-first: once the source is proven stopped,
                // ownership moves to the target even if the target reconcile fails.
                self.set_active_host(target_host);
            }

            let command = RuntimeIntentBuilder::build(
                target_core,
                run_type,
                document.product_bytes().to_vec(),
                expected_applied,
            );
            let local_document = (run_type != RunType::Service).then(|| document.clone());
            self.run_mutation(endpoint.clone(), command, local_document)
                .await
        }
        .await;

        match result {
            Ok(super::endpoint::OperationOutput::Reconciled(outcome)) => {
                self.runtime_preparation.commit_ports(prepared_ports);
                self.set_active_host(target_host);
                if run_type != RunType::Service {
                    return Ok(());
                }
                let observation_matches = self
                    .service_runtime_observation()
                    .as_deref()
                    .is_some_and(|current| {
                        hex::encode(current.product_sha256) == outcome.revision.effective_hash
                            && current.product_sha256 == document.product_sha256()
                    });
                if outcome.outcome != chimera_ipc::api::core::v2::ReconcileOutcomeKind::Noop
                    || !observation_matches
                {
                    self.publish_service_runtime_observation(document)?;
                }
                Ok(())
            }
            Ok(output) => {
                anyhow::bail!("core reconcile returned unexpected lower output: {output:?}")
            }
            Err(error) => {
                local.discard_runtime_draft();
                Err(error)
            }
        }
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        let endpoint = self.active_endpoint().await?;
        let _ = self.run_mutation(endpoint, CoreCommand::Stop, None).await?;
        Ok(())
    }

    pub(crate) async fn recover(&self) -> anyhow::Result<()> {
        let endpoint = self.active_endpoint().await?;
        anyhow::ensure!(
            endpoint.host() == ExecutionHost::Service,
            "core recovery requires Service to be the active host"
        );
        let _ = self
            .run_mutation(endpoint, CoreCommand::Recover, None)
            .await?;
        Ok(())
    }

    pub(crate) async fn change_core(
        &self,
        profiles: crate::config::profile::profiles::Profiles,
        clash_core: ClashCore,
    ) -> anyhow::Result<()> {
        crate::config::core::Config::verge().draft().clash_core = Some(clash_core);
        let clash = crate::bridge::clash::clash_config_from_legacy(
            &crate::config::core::Config::verge().latest(),
            &crate::config::core::Config::clash().latest().0,
        )?;
        let run_type = RunType::default();
        let enable_builtin_enhanced = crate::config::core::Config::verge()
            .latest()
            .enable_builtin_enhanced
            .unwrap_or(true);
        match self
            .reconcile(
                clash,
                profiles,
                clash_core,
                run_type,
                enable_builtin_enhanced,
            )
            .await
        {
            Ok(()) => {
                crate::config::core::Config::verge().apply();
                crate::config::core::Config::verge().latest().save_file()?;
                Ok(())
            }
            Err(error) => {
                crate::config::core::Config::verge().discard();
                crate::config::core::Config::runtime().discard();
                Err(error)
            }
        }
    }

    pub(crate) async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        let status = self.active_endpoint().await?.status().await?;
        if status.health.as_ref().is_some_and(|health| {
            health.state == chimera_ipc::api::status::CoreHealthState::Unhealthy
        }) {
            tracing::warn!(
                consecutive_failures = status
                    .health
                    .as_ref()
                    .map_or(0, |health| health.consecutive_failures),
                "active Service core reports unhealthy runtime health"
            );
        }
        Ok(status)
    }

    pub(crate) fn recovery_notify(&self) -> Arc<tokio::sync::Notify> {
        self.endpoints.local().recovery_notify()
    }

    pub(crate) fn runtime_transform_output(&self) -> Option<(u64, PostProcessingOutput)> {
        match self.active_host() {
            ExecutionHost::Local => self.endpoints.local().runtime_transform_output(),
            ExecutionHost::Service => self.service_runtime_observation().map(|snapshot| {
                (
                    snapshot.revision.get(),
                    snapshot.postprocessing_output.clone(),
                )
            }),
        }
    }

    pub(crate) fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        match self.active_host() {
            ExecutionHost::Local => self.endpoints.local().promoted_runtime_snapshot(),
            ExecutionHost::Service => self.service_runtime_observation(),
        }
    }

    pub(crate) fn runtime_transform_failure(&self) -> Option<RuntimeTransformFailure> {
        self.endpoints.local().runtime_transform_failure()
    }

    pub(crate) fn effective_clash_info(&self) -> ClashInfo {
        match self.active_host() {
            ExecutionHost::Local => self.endpoints.local().effective_clash_info(),
            ExecutionHost::Service => self
                .service_runtime_observation()
                .map(|snapshot| snapshot.clash_info())
                .unwrap_or_else(|| self.endpoints.local().effective_clash_info()),
        }
    }

    pub(crate) async fn api_connection(
        &self,
    ) -> anyhow::Result<Option<chimera_ipc::api::core::v2::CoreApiConnection>> {
        self.active_endpoint().await?.api_connection().await
    }

    async fn api_client(&self) -> anyhow::Result<ApiClient> {
        let endpoint = self.active_endpoint().await?;
        if let Some(connection) = endpoint.api_connection().await? {
            return ApiClient::from_connection(connection);
        }
        if endpoint.host() == ExecutionHost::Service {
            anyhow::bail!("the running Service core did not publish an instance-bound API binding");
        }
        ApiClient::new(self.effective_clash_info())
    }

    pub(crate) async fn service_status_receiver(
        &self,
    ) -> anyhow::Result<tokio::sync::watch::Receiver<super::service_actor::ServiceHostStatus>> {
        Ok(self.service_client().await?.subscribe())
    }

    pub(crate) fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        if let Some(client) = self.service_client.get() {
            client.observe(info);
        }
    }

    pub(crate) fn observe_service_probe_failure(&self) {
        if let Some(client) = self.service_client.get() {
            client.observe_probe_failure();
        }
    }

    pub(crate) async fn report_service_endpoint_down(&self) -> anyhow::Result<()> {
        self.service_client().await?.report_endpoint_down();
        Ok(())
    }

    pub(crate) async fn begin_service_transition(&self) -> anyhow::Result<ServiceTransition> {
        if self.outcome_uncertain() {
            anyhow::bail!(
                "previous lower core-host mutation has an uncertain outcome; restart the application before further mutations"
            );
        }
        let client = self.service_client().await?.clone();
        Ok(ServiceTransition {
            _guard: crate::core::service::HOST_TRANSITION_LOCK.lock().await,
            client,
        })
    }

    pub(crate) async fn on_profile_change(&self, break_when: bool) {
        let result = match self.api_client().await {
            Ok(api) => ConnectionInterruptionService::on_profile_change(&api, break_when).await,
            Err(error) => Err(error),
        };
        if let Err(error) = result {
            tracing::warn!(%error, "failed to interrupt connections after profile change");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{client::runtime::RuntimeSnapshotData, core::actor_v2::endpoint::ControlEndpoint};

    fn service_observation_document() -> RuntimeDocument {
        RuntimeDocument::from_data(
            ClashCore::Mihomo,
            b"mode: rule\n".to_vec().into(),
            RuntimeSnapshotData {
                config: serde_yaml::Mapping::new(),
                exists_keys: Vec::new(),
                postprocessing_output: PostProcessingOutput::default(),
                inspection: Arc::new(
                    crate::client::runtime_inspection::RuntimeInspectionData::bare(),
                ),
            },
        )
    }

    #[test]
    fn endpoint_selection_maps_run_type_to_explicit_host_handle() {
        let facade = CoreFacade::new_local(chimera_ipc::client::shortcuts::Client::new(
            chimera_ipc::SERVICE_PLACEHOLDER,
        ));
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Normal).host(),
            ExecutionHost::Local
        );
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Service).host(),
            ExecutionHost::Service
        );
        assert_eq!(
            facade.endpoint_for_run_type(RunType::Elevated).host(),
            ExecutionHost::Local
        );
    }

    #[tokio::test]
    async fn committed_active_host_never_silently_falls_back() {
        let facade = CoreFacade::new_local(chimera_ipc::client::shortcuts::Client::new(
            chimera_ipc::SERVICE_PLACEHOLDER,
        ));
        assert!(!facade.active_host_known.load(Ordering::Acquire));

        facade.set_active_host(ExecutionHost::Service);
        assert!(facade.active_host_known.load(Ordering::Acquire));
        assert_eq!(facade.active_host(), ExecutionHost::Service);
        assert_eq!(
            facade.active_endpoint().await.unwrap().host(),
            ExecutionHost::Service,
            "a committed Service host must not probe/fallback to Local"
        );

        facade.set_active_host(ExecutionHost::Local);
        assert_eq!(
            facade.active_endpoint().await.unwrap().host(),
            ExecutionHost::Local
        );
    }

    #[tokio::test]
    async fn service_observation_is_not_a_local_process_or_revision_shadow() {
        let facade = CoreFacade::new_local(chimera_ipc::client::shortcuts::Client::new(
            chimera_ipc::SERVICE_PLACEHOLDER,
        ));
        facade
            .publish_service_runtime_observation(service_observation_document())
            .unwrap();

        let local_status = facade.endpoints.local().status().await.unwrap();
        assert!(matches!(
            local_status.state,
            chimera_ipc::api::status::CoreState::Stopped(None)
        ));
        assert!(local_status.applied.is_none());
        assert!(
            facade
                .endpoints
                .local()
                .promoted_runtime_snapshot()
                .is_none()
        );

        facade.set_active_host(ExecutionHost::Service);
        assert!(facade.promoted_runtime_snapshot().is_some());
        facade.set_active_host(ExecutionHost::Local);
        assert!(facade.promoted_runtime_snapshot().is_none());
    }

    #[test]
    fn lower_operation_id_is_preserved_through_anyhow_context() {
        let error = anyhow::Error::new(LowerOperationError::new(41, "lower failed"))
            .context("outer context");
        assert_eq!(operation_id_from_error(&error), Some(41));
    }

    #[test]
    fn uninstall_guard_requires_proof_that_service_owns_no_running_core() {
        use chimera_ipc::{api::status::CoreState, types::ServiceStatus};

        assert_eq!(
            service_uninstall_plan(ServiceStatus::NotInstalled, None).unwrap(),
            ServiceUninstallPlan::AlreadyAbsent
        );
        assert_eq!(
            service_uninstall_plan(ServiceStatus::Stopped, None).unwrap(),
            ServiceUninstallPlan::Uninstall
        );
        assert_eq!(
            service_uninstall_plan(ServiceStatus::Running, Some(&CoreState::Stopped(None)))
                .unwrap(),
            ServiceUninstallPlan::StopThenUninstall
        );
        assert!(service_uninstall_plan(ServiceStatus::Running, Some(&CoreState::Running)).is_err());
        assert!(service_uninstall_plan(ServiceStatus::Running, None).is_err());
    }
}

impl ServiceTransition {
    pub(crate) async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.client.install().await
    }

    pub(crate) async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        let info = self
            .client
            .probe()
            .await
            .context("failed to determine service core ownership before uninstall")?;
        let core_state = info.server.as_ref().map(|server| &server.core_infos.state);
        match service_uninstall_plan(info.status, core_state)? {
            ServiceUninstallPlan::AlreadyAbsent => Ok(()),
            ServiceUninstallPlan::Uninstall => self.client.uninstall().await,
            ServiceUninstallPlan::StopThenUninstall => {
                self.client.stop().await?;
                let stopped = self.client.probe().await?;
                anyhow::ensure!(
                    stopped.status != chimera_ipc::types::ServiceStatus::Running,
                    "service daemon did not prove it stopped; uninstall was refused"
                );
                self.client.uninstall().await
            }
        }
    }

    pub(crate) async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.client.update().await
    }

    pub(crate) async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.client.start().await
    }

    pub(crate) async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.client.restart().await
    }

    pub(crate) async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.client.stop().await
    }

    pub(crate) async fn confirm_ready(&mut self, timeout: Duration) -> anyhow::Result<()> {
        crate::core::service::ipc::wait_until_ready(timeout).await?;
        self.client.probe().await?;
        Ok(())
    }

    pub(crate) async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        let observation = crate::core::service::ipc::refresh_state_now()
            .await
            .context("failed to verify Chimera Service after stop")?;
        if observation.status == chimera_ipc::types::ServiceStatus::Running {
            anyhow::bail!("Chimera Service still reports running after stop");
        }
        self.client.probe().await?;
        crate::core::service::ipc::mark_disconnected_now();
        Ok(())
    }
}
