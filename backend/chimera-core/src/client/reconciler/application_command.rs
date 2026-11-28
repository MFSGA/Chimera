//! Application-level command execution, with one shared attempt owner.
use super::{
    startup_transaction::{StartupTransactionHost, startup_report_result},
    workflow_progress::WorkflowProgress,
};
use crate::client::{
    core_lifecycle::{command::Command, ports::CoreLifecyclePort},
    effects::ports::CommitNotifications,
};
use chimera_config::runtime::executor::ResolvedPortBindings;

#[async_trait::async_trait]
pub trait ApplicationCommandHost: StartupTransactionHost {
    type SystemProxy: Send + 'static;
    async fn execute_lower(&self, command: Command<Self::SystemProxy>) -> anyhow::Result<()>;
    async fn effect_port_binding(&self) -> Option<ResolvedPortBindings>;
    fn take_service_recovery_reconnected(&self) -> bool;
}

/// The caller must settle a previous attempt before the mailbox replies.
/// Test fixtures still have a separate TCC participant for that settlement.
pub enum ApplicationCommandOutcome {
    Settled { service_reconnected: bool },
    RecoverPrevious,
}

pub async fn execute_application_command<H: ApplicationCommandHost>(
    progress: &mut WorkflowProgress,
    lifecycle: &H,
    notifications: Option<&dyn CommitNotifications>,
    command: Command<H::SystemProxy>,
) -> anyhow::Result<ApplicationCommandOutcome> {
    match command {
        Command::StartupReconcile {
            operation_id,
            transition: _transition,
        } => {
            let report = progress.startup_reconcile(operation_id, lifecycle).await;
            startup_report_result(&report)?;
            Ok(ApplicationCommandOutcome::Settled {
                service_reconnected: false,
            })
        }
        Command::RecoverStartup {
            operation_id,
            transition: _transition,
        } => {
            let report = progress.recover_startup(operation_id, lifecycle).await;
            startup_report_result(&report)?;
            Ok(ApplicationCommandOutcome::Settled {
                service_reconnected: false,
            })
        }
        Command::RetryStartup {
            operation_id,
            identity,
            transition: _transition,
        } => {
            let report = progress
                .retry_startup(operation_id, identity, lifecycle)
                .await;
            startup_report_result(&report)?;
            Ok(ApplicationCommandOutcome::Settled {
                service_reconnected: false,
            })
        }
        Command::FollowService {
            operation_id,
            transition: _transition,
        } => {
            progress.follow_service(operation_id, lifecycle).await?;
            Ok(ApplicationCommandOutcome::Settled {
                service_reconnected: false,
            })
        }
        command @ Command::InterruptConnections { .. } => {
            lifecycle.execute_lower(command).await?;
            Ok(ApplicationCommandOutcome::Settled {
                service_reconnected: false,
            })
        }
        Command::RecoverCore => {
            lifecycle.execute_lower(Command::RecoverCore).await?;
            Ok(ApplicationCommandOutcome::RecoverPrevious)
        }
        command @ (Command::Reconcile | Command::ReconcileSource)
            if progress.has_unresolved_attempt() =>
        {
            // Preserve the original ref trigger when recovering a legacy
            // committed target: explicit Retry is forced, whereas a
            // SourceCommitted notice must not replay a known refusal.
            lifecycle.execute_lower(command).await?;
            Ok(ApplicationCommandOutcome::RecoverPrevious)
        }
        Command::CacheStartupUnsettled {
            operation_id,
            reason,
        } => {
            if progress.cache_startup_unsettled(operation_id, reason.clone()) {
                if let Some(notifications) = notifications {
                    let ports = lifecycle.effect_port_binding().await;
                    notifications.publish_full(ports);
                }
            }
            anyhow::bail!(reason)
        }
        command => {
            let source_only_patch = matches!(
                &command,
                Command::PatchApplicationConfig {
                    patch,
                    service_connected,
                    ..
                } if !crate::state::mutation::impact::
                    application_patch_requires_reconcile(patch, *service_connected)
            );
            let preserve_unresolved_attempt = progress.begin_lifecycle_attempt(
                source_only_patch || matches!(&command, Command::Shutdown),
            );
            let refresh_startup_retry_identity =
                matches!(&command, Command::PatchApplicationConfig { .. });
            let service_endpoint_recovery = matches!(&command, Command::ServiceEndpointDown(_));
            let refresh_startup_target = match &command {
                Command::Reconcile
                | Command::ReconcileSource
                | Command::RecoverCore
                | Command::RetryDeferredProfiles { .. }
                | Command::ReconcileProfiles { .. } => true,
                #[cfg(any(test, feature = "test-support"))]
                Command::SelectCore(_) => true,
                Command::PatchApplicationConfig {
                    patch,
                    service_connected,
                    ..
                } => crate::state::mutation::impact::application_patch_requires_reconcile(
                    patch,
                    *service_connected,
                ),
                _ => false,
            };
            let result = lifecycle.execute_lower(command).await;
            if refresh_startup_target {
                progress
                    .refresh_startup_retry_target_after_reconcile(lifecycle, &result)
                    .await;
            } else if refresh_startup_retry_identity && result.is_ok() {
                progress
                    .reschedule_startup_retry_after_committed_config_patch(lifecycle)
                    .await;
            }
            progress.finish_lifecycle_attempt(
                preserve_unresolved_attempt,
                lifecycle.core().outcome_uncertain(),
                &result,
            );
            let recovered = service_endpoint_recovery
                && result.is_ok()
                && lifecycle.take_service_recovery_reconnected();
            result.map(|()| ApplicationCommandOutcome::Settled {
                service_reconnected: recovered,
            })
        }
    }
}
