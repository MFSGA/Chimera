//! Stale system proxy action executed inside the lifecycle mailbox.
//! The source CAS and OS rollback use the existing application and proxy actors.
use crate::client::{
    application::ApplicationClient, clash_config::ClashConfigClient,
    core_lifecycle::ports::CoreLifecyclePort,
};
use std::sync::Arc;

pub struct StaleProxyAction<'a> {
    pub core: &'a dyn CoreLifecyclePort,
    pub application: &'a ApplicationClient,
    pub clash: &'a ClashConfigClient,
}

impl StaleProxyAction<'_> {
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub async fn disable_stale_system_proxy(
        &self,
        system_proxy: crate::client::system_proxy::SystemProxyClient,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        use chimera_ipc::api::status::CoreState;

        let status = self.core.status().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        if !matches!(status.state, CoreState::Stopped(_))
            || status.state_changed_at != expected_core_state_changed_at
        {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }
        let clash = self.clash.get().map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        let receipt = self.core.last_confirmed_runtime_receipt();
        let effective_mixed_port =
            crate::client::ports::effective_mixed_port(receipt.as_deref(), &clash);
        if effective_mixed_port != Some(expected_port) {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }
        let previous = self.application.get().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string())
        })?;
        if previous.state.enable_system_proxy != desired_before {
            return Err(crate::client::system_proxy::SystemProxyActionError::StateChanged);
        }

        if let Err(error) = system_proxy.disable_stale_proxy(expected_port).await {
            return Err(self
                .recover_stale_proxy_action(
                    &system_proxy,
                    original,
                    desired_before,
                    notifications.as_ref(),
                    error,
                )
                .await);
        }
        if !desired_before {
            notifications.runtime_bound(None, false);
            return Ok(());
        }

        let mut candidate = previous.state.clone();
        candidate.enable_system_proxy = false;
        let clash = match self.clash.get() {
            Ok(clash) => clash,
            Err(error) => {
                return Err(self
                    .recover_stale_proxy_action(
                        &system_proxy,
                        original,
                        desired_before,
                        notifications.as_ref(),
                        crate::client::system_proxy::SystemProxyActionError::Failed(
                            error.to_string(),
                        ),
                    )
                    .await);
            }
        };
        let prepared = match self.application.prepare_replace(candidate.clone()).await {
            Ok(prepared) => prepared,
            Err(error) => {
                return Err(self
                    .recover_stale_proxy_action(
                        &system_proxy,
                        original,
                        desired_before,
                        notifications.as_ref(),
                        crate::client::system_proxy::SystemProxyActionError::Failed(
                            error.to_string(),
                        ),
                    )
                    .await);
            }
        };
        match self
            .application
            .replace_prepared_if_version(previous.version, prepared)
            .await
        {
            Ok(crate::state::ConditionalReplaceResult::Replaced(_)) => {
                let persist_error = persist_legacy_mirror().err();
                let fields = crate::client::effects::plan::ApplicationEffectInputs::project(
                    &candidate, &clash, None,
                )
                .app;
                notifications.application_committed(
                    fields,
                    vec![crate::client::effects::plan::EffectKind::SystemProxy],
                );
                notifications.runtime_bound(None, false);
                match persist_error {
                    Some(error) => Err(
                        crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                            "the proxy preference was committed, but the legacy mirror could not be persisted: {error:#}"
                        )),
                    ),
                    None => Ok(()),
                }
            }
            Ok(crate::state::ConditionalReplaceResult::Conflict { .. }) => Err(self
                .publish_latest_proxy_preference(notifications.as_ref())
                .await
                .err()
                .unwrap_or(crate::client::system_proxy::SystemProxyActionError::StateChanged)),
            Err(error) => Err(self
                .recover_stale_proxy_action(
                    &system_proxy,
                    original,
                    desired_before,
                    notifications.as_ref(),
                    crate::client::system_proxy::SystemProxyActionError::Failed(error.to_string()),
                )
                .await),
        }
    }

    async fn publish_latest_proxy_preference(
        &self,
        notifications: &dyn crate::client::effects::ports::CommitNotifications,
    ) -> Result<(), crate::client::system_proxy::SystemProxyActionError> {
        let latest = self.application.get().await.map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                "could not read the latest application preference: {error:#}"
            ))
        })?;
        let clash = self.clash.get().map_err(|error| {
            crate::client::system_proxy::SystemProxyActionError::PartialApply(error.to_string())
        })?;
        let fields = crate::client::effects::plan::ApplicationEffectInputs::project(
            &latest.state,
            &clash,
            None,
        )
        .app;
        notifications.application_committed(
            fields,
            vec![crate::client::effects::plan::EffectKind::SystemProxy],
        );
        notifications.runtime_bound(None, false);
        Ok(())
    }

    async fn recover_stale_proxy_action(
        &self,
        system_proxy: &crate::client::system_proxy::SystemProxyClient,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        desired_before: bool,
        notifications: &dyn crate::client::effects::ports::CommitNotifications,
        error: crate::client::system_proxy::SystemProxyActionError,
    ) -> crate::client::system_proxy::SystemProxyActionError {
        match self.application.get().await {
            Ok(latest) if latest.state.enable_system_proxy != desired_before => {
                match self.publish_latest_proxy_preference(notifications).await {
                    Ok(()) => crate::client::system_proxy::SystemProxyActionError::StateChanged,
                    Err(reconcile_error) => reconcile_error,
                }
            }
            Ok(_) => rollback_stale_proxy_action(system_proxy, original, error.to_string()).await,
            Err(read_error) => {
                crate::client::system_proxy::SystemProxyActionError::PartialApply(format!(
                    "{error}; could not determine the latest preference before rollback: {read_error:#}"
                ))
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
async fn rollback_stale_proxy_action(
    system_proxy: &crate::client::system_proxy::SystemProxyClient,
    original: chimera_contracts::system_proxy::OsProxyConfig,
    error: String,
) -> crate::client::system_proxy::SystemProxyActionError {
    match system_proxy.restore_after_failed_action(original).await {
        Ok(()) => crate::client::system_proxy::SystemProxyActionError::Failed(error),
        Err(rollback_error) => crate::client::system_proxy::SystemProxyActionError::PartialApply(
            format!("{error}; rollback failed: {rollback_error}"),
        ),
    }
}
