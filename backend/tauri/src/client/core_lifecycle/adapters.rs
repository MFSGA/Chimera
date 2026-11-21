//! Legacy adapters retained behind the client core-lifecycle ports.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use serde_yaml::Mapping;
use std::sync::Arc;

use super::ports::{
    CoreLifecyclePort, CoreStatusSnapshot, RunningConfigPort, RuntimeTransformDiagnostics,
    RuntimeTransformFailureDiagnostics, ServiceLifecyclePort, ServiceTransitionLease,
};

pub(crate) struct LegacyServiceBridge {
    facade: Arc<CoreFacade>,
    /// The ref ServiceActor owns production probe and unsettled-command
    /// evidence. The Facade remains only for Chimera's host transition lease
    /// and restart policy while those adapter boundaries are being retired.
    service_actor: Option<chimera_core::actor_v2::service_actor::ServiceClient>,
}

impl LegacyServiceBridge {
    /// Production may only create a Service transition adapter with the ref
    /// ServiceActor already attached. A Facade-only port is a test fixture.
    pub(crate) fn with_actor(
        facade: Arc<CoreFacade>,
        service_actor: chimera_core::actor_v2::service_actor::ServiceClient,
    ) -> Self {
        Self {
            facade,
            service_actor: Some(service_actor),
        }
    }

    #[cfg(test)]
    pub(crate) fn new(facade: Arc<CoreFacade>) -> Self {
        Self {
            facade,
            service_actor: None,
        }
    }
}

struct LegacyServiceTransition {
    inner: crate::core::actor_v2::facade::ServiceTransition,
    /// Compatibility fallback for lifecycle fixtures without the shared Core
    /// recovery facade. Production re-adopts the returned endpoint in
    /// `CoreLifecyclePort::recover_service_endpoint` instead.
    service_actor: Option<chimera_core::actor_v2::service_actor::ServiceClient>,
}

#[async_trait]
impl ServiceLifecyclePort for LegacyServiceBridge {
    fn actor_status(&self) -> Option<chimera_core::actor_v2::service_status::ServiceHostStatus> {
        self.service_actor.as_ref().map(|actor| {
            crate::client::service_status::project_actor_service_status(&actor.status())
        })
    }

    fn actor_status_updates(
        &self,
    ) -> Option<
        tokio::sync::watch::Receiver<chimera_core::actor_v2::service_router::ServiceHostStatus>,
    > {
        self.service_actor.as_ref().map(|actor| actor.subscribe())
    }

    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>> {
        if let Some(service) = &self.service_actor {
            let status = service.probe().await?;
            return Ok(chimera_ipc::types::StatusInfo {
                name: status.name,
                version: status.version,
                status: status.status,
                server: status.server,
            });
        }
        // Only manager-only tests have no ServiceActor. Production must
        // fail closed instead of quietly querying the legacy Facade.
        #[cfg(test)]
        {
            self.facade.probe_service().await
        }
        #[cfg(not(test))]
        {
            anyhow::bail!("Service probe requires ref ServiceActor")
        }
    }

    async fn command_settled(&self) -> bool {
        if let Some(service) = &self.service_actor {
            // A failed Actor query cannot prove the last OS helper settled.
            return service.command_settled().await.unwrap_or(false);
        }
        #[cfg(test)]
        {
            self.facade.service_command_settled().await
        }
        #[cfg(not(test))]
        {
            false
        }
    }

    fn restart_policy(
        &self,
    ) -> chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        if let Some(service) = &self.service_actor {
            let status = service.status();
            return chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
                attempts: status.restart_attempts,
                exhausted: status.phase
                    == chimera_core::actor_v2::service_router::ServicePhase::Exhausted,
            };
        }
        // Facade's historic policy is only compiled for legacy test ports.
        // Production must never silently substitute a second retry budget.
        #[cfg(test)]
        {
            self.facade.service_restart_policy()
        }
        #[cfg(not(test))]
        {
            panic!("production ServiceLifecyclePort requires ref ServiceActor")
        }
    }

    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(LegacyServiceTransition {
            inner: self.facade.begin_service_transition().await?,
            service_actor: self.service_actor.clone(),
        }))
    }

    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        Ok(Box::new(LegacyServiceTransition {
            inner: self.facade.try_begin_service_transition().await?,
            service_actor: self.service_actor.clone(),
        }))
    }
}

#[async_trait]
impl ServiceTransitionLease for LegacyServiceTransition {
    async fn report_endpoint_down(&mut self) -> anyhow::Result<()> {
        if let Some(service) = &self.service_actor {
            // Test-only compatibility path. Production uses the lifecycle
            // Core facade so the returned endpoint is re-adopted by its router.
            if let Err(error) = service.recover_endpoint().await {
                tracing::warn!(%error, "ref ServiceActor could not recover the down endpoint");
            }
            return Ok(());
        }
        #[cfg(test)]
        {
            self.inner.report_endpoint_down().await
        }
        #[cfg(not(test))]
        {
            anyhow::bail!("Service endpoint recovery requires ref ServiceActor")
        }
    }

    async fn install_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.install_daemon().await
    }

    async fn uninstall_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.uninstall_daemon().await
    }

    async fn update_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.update_daemon().await
    }

    async fn start_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.start_daemon().await
    }

    async fn restart_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.restart_daemon().await
    }

    async fn stop_daemon(&mut self) -> anyhow::Result<()> {
        self.inner.stop_daemon().await
    }

    async fn confirm_ready(&mut self, timeout: std::time::Duration) -> anyhow::Result<()> {
        self.inner.confirm_ready(timeout).await
    }

    async fn confirm_stopped(&mut self) -> anyhow::Result<()> {
        self.inner.confirm_stopped().await
    }
}
use crate::{
    client::runtime::RuntimeSnapshot,
    config::chimera::ClashCore,
    core::{
        actor_v2::CoreFacade,
        clash::{api::ClashRuntimeConfig, core::RunType},
    },
};

/// Production running Clash API config follows the ref CoreActor's
/// instance-bound capability, not LegacyCoreBridge/Facade's host cache.
pub(crate) struct ActorRunningConfigBridge {
    core: chimera_core::actor_v2::CoreClient,
}

impl ActorRunningConfigBridge {
    pub(crate) fn new(core: chimera_core::actor_v2::CoreClient) -> Self {
        Self { core }
    }

    async fn api_client(&self) -> anyhow::Result<crate::core::actor_v2::api::ApiClient> {
        Ok(crate::core::actor_v2::api::ApiClient::from_core_client(
            self.core.api_client().await?,
        ))
    }
}

#[async_trait]
impl RunningConfigPort for ActorRunningConfigBridge {
    async fn read(&self) -> anyhow::Result<ClashRuntimeConfig> {
        self.api_client().await?.get_configs().await
    }

    async fn patch(&self, patch: &Mapping) -> anyhow::Result<()> {
        self.api_client().await?.patch_configs(patch).await
    }
}

pub(crate) struct LegacyCoreBridge {
    facade: Arc<CoreFacade>,
    service_recovery: Option<Arc<tokio::sync::Mutex<chimera_core::actor_v2::facade::CoreFacade>>>,
}

impl LegacyCoreBridge {
    pub(crate) fn new(facade: Arc<CoreFacade>) -> Self {
        Self {
            facade,
            service_recovery: None,
        }
    }

    pub(crate) fn with_service_recovery(
        facade: Arc<CoreFacade>,
        service_recovery: chimera_core::actor_v2::facade::CoreFacade,
    ) -> Self {
        Self {
            facade,
            service_recovery: Some(Arc::new(tokio::sync::Mutex::new(service_recovery))),
        }
    }

    /// Compatibility boundary for the staged lifecycle migration.
    ///
    /// The CoreFacade owns the lower host boundary; this adapter only maps it
    /// into the client lifecycle port. Production local execution uses
    /// LocalRuntimeHost, while the manager-only path is retained for tests.
    fn facade(&self) -> &Arc<CoreFacade> {
        &self.facade
    }
}

#[async_trait]
impl CoreLifecyclePort for LegacyCoreBridge {
    async fn reconcile_transparent_proxy(
        &self,
        request: &chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyRequest,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        self.facade()
            .reconcile_transparent_proxy(request.clone())
            .await
    }

    async fn transparent_proxy_status(
        &self,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        self.facade().transparent_proxy_status().await
    }

    async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()> {
        self.facade().reconcile(clash, target_core, run_type).await
    }

    async fn reconcile_prepared(
        &self,
        prepared: super::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        self.facade().reconcile_prepared(prepared, core_spec).await
    }

    async fn retire_service_core(
        &self,
        expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        self.facade().retire_service_core(expected_owner).await
    }

    async fn recover_service_quarantine(&self) -> anyhow::Result<()> {
        self.facade().recover_service_quarantine().await
    }

    async fn reconcile_after_service_release(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        self.facade()
            .reconcile_after_service_release(clash, target_core)
            .await
    }

    async fn reconcile_profiles(
        &self,
        clash: chimera_config::clash::config::ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.facade()
            .reconcile_with_profiles(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                target,
            )
            .await
    }

    async fn reconcile_profiles_after_service_release(
        &self,
        clash: chimera_config::clash::config::ClashConfig,
        target_core: ClashCore,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.facade()
            .reconcile_with_profiles_after_service_release(
                clash,
                target_core,
                profiles,
                app,
                staged_content,
                target,
            )
            .await
    }

    async fn validate_profile_runtime(
        &self,
        clash: chimera_config::clash::config::ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::CheckRecord> {
        self.facade()
            .validate_profile_runtime(clash, target_core, run_type, profiles, app, staged_content)
            .await
    }

    async fn prepare_profile_runtime(
        &self,
        clash: chimera_config::clash::config::ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        target: Option<String>,
        operation_id: chimera_core_manager::OperationId,
    ) -> anyhow::Result<(
        crate::client::application_workflow::mutation::AppliedCandidate,
        crate::client::application_workflow::mutation::CheckRecord,
    )> {
        self.facade()
            .prepare_profile_runtime(
                clash,
                target_core,
                run_type,
                profiles,
                app,
                staged_content,
                target,
                operation_id,
            )
            .await
    }

    async fn confirm_profile_runtime(
        &self,
        operation_id: chimera_core_manager::OperationId,
        candidate: crate::client::application_workflow::mutation::AppliedCandidate,
    ) -> anyhow::Result<()> {
        self.facade()
            .confirm_profile_runtime(operation_id, candidate)
            .await
    }

    async fn discard_profile_runtime(&self, operation_id: chimera_core_manager::OperationId) {
        self.facade().discard_profile_runtime(&operation_id).await;
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.facade().stop().await
    }

    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        self.facade().finish_shutdown().await
    }

    async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()> {
        self.facade().change_core(clash_core).await
    }

    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot> {
        Ok(self.facade().status().await)
    }

    fn service_endpoint_degraded(&self) -> bool {
        self.facade().service_endpoint_degraded()
    }

    fn service_endpoint_recovery_supported(&self) -> bool {
        self.service_recovery.is_some()
    }

    async fn settle_service_recovery_action(&self) -> Option<Result<(), String>> {
        let service_recovery = self.service_recovery.as_ref()?;
        Some(service_recovery.lock().await.consume_settled_action().await)
    }

    async fn recover_service_endpoint(
        &self,
        closing: &tokio_util::sync::CancellationToken,
    ) -> Option<Result<(), chimera_core_manager::CoreError>> {
        let service_recovery = self.service_recovery.as_ref()?;
        Some(
            service_recovery
                .lock()
                .await
                .recover_service_endpoint(closing)
                .await
                .map(|_| ()),
        )
    }

    async fn select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        self.facade().select_execution_host(host).await
    }

    async fn active_runtime_receipt(
        &self,
    ) -> anyhow::Result<Option<Arc<crate::client::runtime::RuntimeApplyReceipt>>> {
        self.facade().active_runtime_receipt().await
    }

    fn last_confirmed_runtime_receipt(
        &self,
    ) -> Option<Arc<crate::client::runtime::RuntimeApplyReceipt>> {
        self.facade().last_confirmed_runtime_receipt()
    }

    async fn active_effective_config(
        &self,
    ) -> anyhow::Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>> {
        self.facade().active_effective_config().await
    }

    async fn observe_runtime_baseline(
        &self,
    ) -> anyhow::Result<crate::client::application_workflow::mutation::KnownRuntimeState> {
        self.facade().observe_runtime_baseline().await
    }

    async fn restore_runtime_receipt(
        &self,
        receipt: Arc<crate::client::runtime::RuntimeApplyReceipt>,
    ) -> anyhow::Result<()> {
        self.facade().restore_runtime_receipt(receipt).await
    }

    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(self.facade().recovery_notify())
    }

    fn outcome_uncertain(&self) -> bool {
        self.facade().outcome_uncertain()
    }

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        let core = self.facade();
        let failure =
            core.runtime_transform_failure()
                .map(|failure| RuntimeTransformFailureDiagnostics {
                    attempt_revision: failure.attempt_revision.get(),
                    transform_uid: failure.transform_uid,
                    scope_uid: failure.scope_uid,
                    script_type: failure.script_type,
                    message: failure.message,
                });
        Ok(core
            .runtime_transform_output()
            .map(|(revision, output)| RuntimeTransformDiagnostics {
                revision,
                output,
                failure,
            }))
    }

    fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        self.facade().promoted_runtime_snapshot()
    }

    fn pending_runtime_product_target(&self) -> Option<String> {
        self.facade().pending_runtime_product_target()
    }

    async fn republish_pending_runtime_product(&self, target: &str) -> anyhow::Result<bool> {
        self.facade()
            .republish_pending_runtime_product(target)
            .await
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        self.facade().effective_clash_info()
    }

    async fn active_clash_info(&self) -> anyhow::Result<crate::config::clash::ClashInfo> {
        self.facade().active_clash_info().await
    }

    async fn clash_api_client(&self) -> anyhow::Result<crate::core::actor_v2::api::ApiClient> {
        self.facade().clash_api_client().await
    }

    async fn on_profile_change(&self, break_when: bool) {
        self.facade().on_profile_change(break_when).await;
    }
}
