//! Object-safe handle to the single existing reconciler. The shared facade
//! owns the real ActorClient; only isolated desktop fixtures adapt a direct port.
use super::{
    client::ActorClient, committed::SourceDomain, mailbox::CoreLifecycleStatus,
    status::ReconcilerStatus,
};
use crate::{
    actor_v2::service_status::ServiceHostStatus,
    client::{
        app_lifecycle::CompletedLifecycleOperation,
        core_lifecycle::{
            command::{ApplicationPatchResult, Command},
            ports::PreparedCoreBinary,
        },
    },
};
use std::{sync::Arc, time::Duration};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
type ProxyHandle = crate::client::system_proxy::SystemProxyClient;
#[cfg(any(target_os = "android", target_os = "ios"))]
type ProxyHandle = ();

#[async_trait::async_trait]
pub trait RuntimeReconciler: Send + Sync {
    async fn startup_reconcile(&self) -> anyhow::Result<CompletedLifecycleOperation>;
    fn status(&self) -> CoreLifecycleStatus;
    fn subscribe_reconciler(&self) -> Option<tokio::sync::watch::Receiver<ReconcilerStatus>>;
    fn reconciler_status(&self) -> ReconcilerStatus;
    async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus;
    fn source_committed(&self, domain: SourceDomain, version: u64, reconcile_requested: bool);
    fn request_runtime_rebuild(&self);
    async fn request_service_endpoint_down_if(
        &self,
        still_current: Box<dyn Fn() -> bool + Send + 'static>,
    );
    fn service_status(&self) -> ServiceHostStatus;
    fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>);
    fn observe_service_probe_failure(&self);
    async fn start_core(&self) -> anyhow::Result<()>;
    async fn stop_core(&self) -> anyhow::Result<()>;
    async fn shutdown(&self) -> anyhow::Result<()>;
    async fn reconcile(&self) -> anyhow::Result<()>;
    async fn interrupt_connections(
        &self,
    ) -> anyhow::Result<Option<chimera_contracts::runtime::Degradation>>;
    async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<ApplicationPatchResult>;
    async fn replace_core_binary(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()>;
    async fn install_service(&self) -> anyhow::Result<()>;
    async fn uninstall_service(&self) -> anyhow::Result<()>;
    async fn update_service(&self) -> anyhow::Result<()>;
    async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()>;
    async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()>;
    async fn stop_service(&self) -> anyhow::Result<()>;
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError>;
    async fn request_service_endpoint_down(&self) {
        self.request_service_endpoint_down_if(Box::new(|| true))
            .await;
    }
}

#[async_trait::async_trait]
impl<W: Send + 'static> RuntimeReconciler for ActorClient<W, ProxyHandle> {
    async fn startup_reconcile(&self) -> anyhow::Result<CompletedLifecycleOperation> {
        self.startup_reconcile_with_timeout(None).await
    }

    fn status(&self) -> CoreLifecycleStatus {
        ActorClient::status(self)
    }

    fn subscribe_reconciler(&self) -> Option<tokio::sync::watch::Receiver<ReconcilerStatus>> {
        Some(ActorClient::subscribe_reconciler(self))
    }

    fn reconciler_status(&self) -> ReconcilerStatus {
        ActorClient::reconciler_status(self)
    }

    async fn settled(&self, domain: SourceDomain, version: u64) -> ReconcilerStatus {
        ActorClient::settled(self, domain, version).await
    }

    fn source_committed(&self, domain: SourceDomain, version: u64, reconcile_requested: bool) {
        ActorClient::source_committed(self, domain, version, reconcile_requested)
    }

    fn request_runtime_rebuild(&self) {
        ActorClient::request_runtime_rebuild(self)
    }

    async fn request_service_endpoint_down_if(
        &self,
        still_current: Box<dyn Fn() -> bool + Send + 'static>,
    ) {
        ActorClient::request_service_endpoint_down_if(self, still_current).await
    }

    fn service_status(&self) -> ServiceHostStatus {
        ActorClient::service_status(self)
    }

    fn observe_service_status(&self, info: chimera_ipc::types::StatusInfo<'static>) {
        ActorClient::observe_service_status(self, info)
    }

    fn observe_service_probe_failure(&self) {
        ActorClient::observe_service_probe_failure(self)
    }

    async fn start_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StartCore).await
    }

    async fn stop_core(&self) -> anyhow::Result<()> {
        self.execute(Command::StopCore).await
    }

    async fn shutdown(&self) -> anyhow::Result<()> {
        self.execute(Command::Shutdown).await
    }

    async fn reconcile(&self) -> anyhow::Result<()> {
        if self.startup_recovery_needed() {
            return self.recover_startup().await;
        }
        self.execute(Command::Reconcile).await
    }

    async fn interrupt_connections(
        &self,
    ) -> anyhow::Result<Option<chimera_contracts::runtime::Degradation>> {
        ActorClient::interrupt_connections(self).await
    }

    async fn patch_application_config(
        &self,
        patch: chimera_config::application::ChimeraAppConfigPatch,
    ) -> anyhow::Result<ApplicationPatchResult> {
        ActorClient::patch_application_config(self, patch).await
    }

    async fn replace_core_binary(&self, artifact: PreparedCoreBinary) -> anyhow::Result<()> {
        self.execute(Command::ReplaceCoreBinary(artifact)).await
    }

    async fn install_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::InstallService).await
    }

    async fn uninstall_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UninstallService)
            .await
    }

    async fn update_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::UpdateService).await
    }

    async fn start_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::StartService {
            transition,
            ready_timeout,
        })
        .await
    }

    async fn restart_service(&self, ready_timeout: Duration) -> anyhow::Result<()> {
        self.execute_service_command(|transition| Command::RestartService {
            transition,
            ready_timeout,
        })
        .await
    }

    async fn stop_service(&self) -> anyhow::Result<()> {
        self.execute_service_command(Command::StopService).await
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        self.execute(Command::DisableStaleSystemProxy {
            system_proxy,
            expected_port,
            expected_core_state_changed_at,
            desired_before,
            original,
            notifications,
            persist_legacy_mirror,
        })
        .await
        .map_err(|error| {
            error
                .downcast::<crate::client::system_proxy::SystemProxyActionError>()
                .unwrap_or_else(|error| {
                    crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
                })
        })
    }
}
