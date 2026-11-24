//! Lower lifecycle execution shared by the only runtime owner.
//! The host supplies source snapshots and platform ports, never a second
//! command queue or independent Core controller.
use super::{
    command::{ApplicationPatchResult, Command},
    ports::{BinaryInstaller, CoreLifecyclePort, PreparedRuntime, ServiceTransitionLease},
    workflow::ServiceRecovery,
};
use crate::client::reconciler::{
    authority::RuntimeSourceAuthority, committed::SourceDomain, plan::Trigger,
    startup_policy::StartupRetryTarget, state::RuntimeProgress,
};
use chimera_config::{
    application::{ChimeraAppConfig, ChimeraAppConfigPatch},
    profile::Profiles,
};
use chimera_core_manager::CoreSpec;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::Ordering},
};

#[async_trait::async_trait]
pub trait LifecycleHost: Send + Sync {
    type SystemProxy: Send + 'static;
    fn application_config(&self) -> ChimeraAppConfig;
    fn core(&self) -> &dyn CoreLifecyclePort;
    fn installer(&self) -> &dyn BinaryInstaller;
    fn service_recovery(&self) -> &parking_lot::Mutex<ServiceRecovery>;
    fn progress(&self) -> &parking_lot::Mutex<RuntimeProgress>;
    fn clear_round_effects(&self);
    fn canonical_convergence_session(&self) -> bool;
    async fn reconcile_with_trigger(&self, trigger: Trigger) -> anyhow::Result<()>;
    async fn apply_checked_runtime(
        &self,
        prepared: PreparedRuntime,
        core_spec: CoreSpec,
        domain: SourceDomain,
        version: u64,
        authority: Arc<dyn RuntimeSourceAuthority>,
        interrupt_mode: bool,
    ) -> anyhow::Result<()>;
    async fn retry_deferred_profiles(&self, digest: &str) -> anyhow::Result<()>;
    async fn reconcile_profiles(
        &self,
        profiles: Arc<Profiles>,
        staged_content: BTreeMap<String, String>,
    ) -> anyhow::Result<()>;
    async fn patch_application_config_with_result(
        &self,
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
    ) -> anyhow::Result<ApplicationPatchResult>;
    async fn maintain_service_release(&self) -> anyhow::Result<()>;
    async fn report_service_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<bool>;
    fn record_service_reconnected(&self);
    async fn refresh_service_recovery_host(&self);
    async fn execute_extension(
        &self,
        command: Command<Self::SystemProxy>,
        startup_retry_target: &mut Option<StartupRetryTarget>,
    ) -> anyhow::Result<()>;
}

pub async fn execute<H: LifecycleHost + ?Sized>(
    host: &H,
    command: Command<H::SystemProxy>,
    startup_retry_target: &mut Option<StartupRetryTarget>,
) -> anyhow::Result<()> {
    host.clear_round_effects();
    host.progress().lock().clear_pending();
    // Retain selection only for the exact checked apply or ref's prepared
    // selection reuse paths. Never carry stale evidence across a new round.
    let prepared_selection_candidate =
        matches!(&command, Command::ReconcileSource | Command::Reconcile)
            || (host.canonical_convergence_session()
                && matches!(&command, Command::StartCore | Command::RecoverCore));
    if !prepared_selection_candidate && !matches!(&command, Command::ApplyChecked { .. }) {
        host.progress().lock().invalidate_selection();
    }
    let result = match command {
        Command::RecoverCore => host.reconcile_with_trigger(Trigger::CoreChanged).await,
        Command::Reconcile => host.reconcile_with_trigger(Trigger::Retry).await,
        Command::ReconcileSource => host.reconcile_with_trigger(Trigger::SourceCommitted).await,
        Command::ApplyChecked {
            prepared,
            core_spec,
            domain,
            version,
            authority,
            interrupt_mode,
        } => {
            host.apply_checked_runtime(
                prepared,
                core_spec,
                domain,
                version,
                authority,
                interrupt_mode,
            )
            .await
        }
        Command::StartCore => host.reconcile_with_trigger(Trigger::Start).await,
        Command::RetryDeferredProfiles { digest } => host.retry_deferred_profiles(&digest).await,
        Command::ReconcileProfiles {
            profiles,
            staged_content,
        } => host.reconcile_profiles(profiles, staged_content).await,
        Command::Shutdown => {
            host.service_recovery().lock().suppress();
            host.core().stop().await
        }
        Command::StopCore => host.core().stop().await,
        Command::PatchApplicationConfig {
            patch,
            service_connected,
            degradations,
            runtime_converged,
        } => host
            .patch_application_config_with_result(patch, service_connected)
            .await
            .map(|result| {
                // Notified source apply is a separate queued Core operation. The
                // source-command completion cannot mark it Converged.
                runtime_converged.store(
                    result.notified_version.is_none() && result.degradations.is_empty(),
                    Ordering::Release,
                );
                let _ = degradations.send(result);
            }),
        Command::ReplaceCoreBinary(artifact) => super::binary::replace_binary(host, artifact).await,
        Command::InstallService(mut transition) => transition.install_daemon().await,
        Command::UninstallService(mut transition) => {
            super::service::uninstall_service(host, transition.as_mut()).await
        }
        Command::UpdateService(mut transition) => transition.update_daemon().await,
        Command::StartService {
            mut transition,
            ready_timeout,
        } => super::service::start_service(host, transition.as_mut(), false, ready_timeout).await,
        Command::RestartService {
            mut transition,
            ready_timeout,
        } => super::service::start_service(host, transition.as_mut(), true, ready_timeout).await,
        Command::StopService(mut transition) => {
            super::service::stop_service(host, transition.as_mut()).await
        }
        Command::MaintainServiceRelease => host.maintain_service_release().await,
        Command::ServiceEndpointDown(mut transition) => {
            match host.report_service_endpoint_down(transition.as_mut()).await {
                Ok(recovered) => {
                    if recovered {
                        host.record_service_reconnected();
                    }
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
        command => host.execute_extension(command, startup_retry_target).await,
    };

    if prepared_selection_candidate && result.is_err() {
        host.progress().lock().invalidate_selection();
    }
    host.refresh_service_recovery_host().await;
    result
}
