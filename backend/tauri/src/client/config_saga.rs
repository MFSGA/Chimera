//! Legacy combined-config compatibility saga owned by the main client facade.
//!
//! Typed Application, Session, and Clash state remain actor-owned. This module
//! only coordinates the legacy `IVerge` compatibility entry point until callers
//! migrate to domain-specific typed patches.

use anyhow::{Result, bail};
use chimera_config::{
    application::ChimeraAppConfig,
    clash::config::{ClashConfig, ClashConfigPatch},
    state::PersistentState,
};
use struct_patch::Patch;

use super::{
    ChimeraClient, ClientError, CompensationFailure, LegacyVergeDomain, PartialCommit,
    error::Result as ClientResult,
};
use crate::{
    bridge::typed_patches_from_legacy_patch,
    config::{chimera::IVerge, core::Config},
    core::{handle, sysopt},
    state::{ConditionalReplaceResult, mirror::PreparedTypedReplace},
    utils,
};

enum PreparedConfigDomain {
    Application {
        expected_version: u64,
        forward: PreparedTypedReplace<ChimeraAppConfig>,
        rollback: PreparedTypedReplace<ChimeraAppConfig>,
    },
    Session {
        expected_version: u64,
        forward: PreparedTypedReplace<PersistentState>,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        expected_version: u64,
        forward: PreparedTypedReplace<ClashConfig>,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

enum CommittedConfigDomain {
    Application {
        committed_version: u64,
        rollback: PreparedTypedReplace<ChimeraAppConfig>,
    },
    Session {
        committed_version: u64,
        rollback: PreparedTypedReplace<PersistentState>,
    },
    Clash {
        committed_version: u64,
        rollback: PreparedTypedReplace<ClashConfig>,
    },
}

struct VergePatchPlan {
    service_mode: Option<bool>,
    auto_launch_changed: bool,
    system_proxy_changed: bool,
    proxy_bypass_changed: bool,
    enable_proxy_guard: bool,
    log_level_changed: bool,
    log_max_files_changed: bool,
    refresh_systray: bool,
}

fn plan_verge_patch(
    patch: &IVerge,
    clash_patch: Option<&ClashConfigPatch>,
) -> Result<VergePatchPlan> {
    if let Some(ref theme_color) = patch.theme_color
        && !theme_color.is_empty()
        && !crate::config::chimera::is_hex_color(theme_color)
    {
        bail!("Invalid theme color: {}", theme_color);
    }

    Ok(VergePatchPlan {
        service_mode: patch.enable_service_mode,
        auto_launch_changed: patch.enable_auto_launch.is_some(),
        system_proxy_changed: patch.enable_system_proxy.is_some(),
        proxy_bypass_changed: patch.system_proxy_bypass.is_some(),
        enable_proxy_guard: patch.enable_proxy_guard == Some(true),
        log_level_changed: patch.app_log_level.is_some(),
        log_max_files_changed: patch.max_log_files.is_some(),
        refresh_systray: patch.enable_system_proxy.is_some()
            || clash_patch.is_some_and(|patch| patch.enable_tun_mode.is_some()),
    })
}

async fn apply_verge_runtime_change(client: &ChimeraClient, plan: &VergePatchPlan) -> Result<()> {
    let ipc_state = crate::core::service::ipc::get_ipc_state();

    if let Some(service_mode) = plan.service_mode
        && ipc_state.is_connected()
    {
        log::debug!(target: "app", "change service mode to {}", service_mode);
        client.rebuild_running_config().await?;
    }

    Ok(())
}

fn run_verge_patch_side_effects(plan: &VergePatchPlan, patch: &IVerge) -> Result<()> {
    if plan.auto_launch_changed {
        sysopt::Sysopt::global().update_launch()?;
    }

    if plan.system_proxy_changed || plan.proxy_bypass_changed {
        sysopt::Sysopt::global().update_sysproxy()?;
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.enable_proxy_guard {
        sysopt::Sysopt::global().guard_proxy();
    }

    if plan.log_level_changed || plan.log_max_files_changed {
        utils::init::refresh_logger((patch.app_log_level.clone(), patch.max_log_files))?;
    }

    if plan.refresh_systray {
        handle::Handle::update_systray_part()?;
    }

    log::debug!("todo: handle other fields");
    Ok(())
}

async fn compensate_legacy_verge_saga(
    client: &ChimeraClient,
    mut committed: Vec<CommittedConfigDomain>,
    primary: ClientError,
    mut failed_compensations: Vec<CompensationFailure>,
) -> ClientResult<()> {
    let committed_domains = committed
        .iter()
        .map(|domain| match domain {
            CommittedConfigDomain::Application { .. } => LegacyVergeDomain::Application,
            CommittedConfigDomain::Session { .. } => LegacyVergeDomain::Session,
            CommittedConfigDomain::Clash { .. } => LegacyVergeDomain::Clash,
        })
        .collect::<Vec<_>>();
    let mut compensated_domains = Vec::new();

    while let Some(domain) = committed.pop() {
        match domain {
            CommittedConfigDomain::Application {
                committed_version,
                rollback,
            } => match client
                .inner
                .application
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Application);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Application,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Application,
                    message: format!("{error:#}"),
                }),
            },
            CommittedConfigDomain::Session {
                committed_version,
                rollback,
            } => match client
                .inner
                .session_state
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Session);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Session,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Session,
                    message: format!("{error:#}"),
                }),
            },
            CommittedConfigDomain::Clash {
                committed_version,
                rollback,
            } => match client
                .inner
                .clash_config
                .replace_prepared_if_version(committed_version, rollback)
                .await
            {
                Ok(ConditionalReplaceResult::Replaced(_)) => {
                    compensated_domains.push(LegacyVergeDomain::Clash);
                }
                Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                    failed_compensations.push(CompensationFailure::Conflict {
                        domain: LegacyVergeDomain::Clash,
                        expected_version: committed_version,
                        actual_version,
                    });
                }
                Err(error) => failed_compensations.push(CompensationFailure::Error {
                    domain: LegacyVergeDomain::Clash,
                    message: format!("{error:#}"),
                }),
            },
        }
    }

    let mut legacy_uncertainties = Vec::new();
    if let Err(error) = Config::verge().data().save_file() {
        legacy_uncertainties.push(format!(
            "legacy verge rollback persistence failed: {error:#}"
        ));
    }
    if let Err(error) = Config::clash().data().save_config() {
        legacy_uncertainties.push(format!(
            "legacy clash rollback persistence failed: {error:#}"
        ));
    }
    handle::Handle::refresh_verge();
    handle::Handle::refresh_clash();

    if failed_compensations.is_empty() && legacy_uncertainties.is_empty() {
        return Err(primary);
    }

    let mut partial = PartialCommit::new(
        &primary,
        committed_domains,
        compensated_domains,
        failed_compensations,
    );
    for message in legacy_uncertainties {
        partial = partial.with_legacy_state_uncertain(message);
    }
    log::error!("legacy verge saga requires reconciliation: {partial:?}");
    Err(partial.into())
}

impl ChimeraClient {
    pub(crate) async fn patch_verge(&self, patch: IVerge) -> ClientResult<()> {
        self.apply_legacy_verge_patch_saga(patch).await
    }

    async fn apply_legacy_verge_patch_saga(&self, patch: IVerge) -> ClientResult<()> {
        let base = Config::verge().latest().clone();
        let legacy_clash = Config::clash().latest().clone();
        let mut split = typed_patches_from_legacy_patch(base, &patch, &legacy_clash)?;
        let plan = plan_verge_patch(&patch, split.clash_config.as_ref())?;

        let application_pair = if let Some(application_patch) = split.application.take() {
            let snapshot = self.inner.application.get().await?;
            let mut next = snapshot.state.clone();
            next.apply(application_patch);
            Some((snapshot, next))
        } else {
            None
        };

        let session_pair = if let Some(session_patch) = split.session_state.take() {
            let snapshot = self.inner.session_state.get().await?;
            let mut next = snapshot.state.clone();
            next.apply(session_patch);
            Some((snapshot, next))
        } else {
            None
        };

        let clash_pair = if let Some(clash_patch) = split.clash_config.as_ref() {
            let snapshot = self.inner.clash_config.get_snapshot().await?;
            let mut next = snapshot.state.clone();
            next.apply(clash_patch.clone());
            Some((snapshot, next))
        } else {
            None
        };

        let mut prepared = Vec::new();
        if let Some((snapshot, next)) = application_pair {
            prepared.push(PreparedConfigDomain::Application {
                expected_version: snapshot.version,
                forward: self.inner.application.prepare_replace(next).await?,
                rollback: self
                    .inner
                    .application
                    .prepare_replace(snapshot.state.clone())
                    .await?,
            });
        }
        if let Some((snapshot, next)) = session_pair {
            prepared.push(PreparedConfigDomain::Session {
                expected_version: snapshot.version,
                forward: self.inner.session_state.prepare_replace(next).await?,
                rollback: self
                    .inner
                    .session_state
                    .prepare_replace(snapshot.state.clone())
                    .await?,
            });
        }
        if let Some((snapshot, next)) = clash_pair {
            prepared.push(PreparedConfigDomain::Clash {
                expected_version: snapshot.version,
                forward: self.inner.clash_config.prepare_replace(next).await?,
                rollback: self
                    .inner
                    .clash_config
                    .prepare_replace(snapshot.state.clone())
                    .await?,
            });
        }

        let mut committed = Vec::new();
        for domain in prepared {
            let commit_error = match domain {
                PreparedConfigDomain::Application {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .application
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Application {
                            committed_version: snapshot.version,
                            rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "application config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit application config in legacy verge saga"),
                    ),
                },
                PreparedConfigDomain::Session {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .session_state
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Session {
                            committed_version: snapshot.version,
                            rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "session config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit session state in legacy verge saga"),
                    ),
                },
                PreparedConfigDomain::Clash {
                    expected_version,
                    forward,
                    rollback,
                } => match self
                    .inner
                    .clash_config
                    .replace_prepared_if_version(expected_version, forward)
                    .await
                {
                    Ok(ConditionalReplaceResult::Replaced(snapshot)) => {
                        committed.push(CommittedConfigDomain::Clash {
                            committed_version: snapshot.version,
                            rollback,
                        });
                        continue;
                    }
                    Ok(ConditionalReplaceResult::Conflict { actual_version }) => {
                        ClientError::Custom(format!(
                            "clash config version conflict: expected {expected_version}, actual {actual_version}"
                        ))
                    }
                    Err(error) => ClientError::Anyhow(
                        error.context("failed to commit clash config in legacy verge saga"),
                    ),
                },
            };

            return compensate_legacy_verge_saga(self, committed, commit_error, Vec::new()).await;
        }

        let finalize = async {
            apply_verge_runtime_change(self, &plan).await?;
            if let Some(clash_patch) = split.clash_config.as_ref() {
                self.inner
                    .clash_config
                    .apply_legacy_patch_runtime(self, clash_patch)
                    .await?;
            }
            run_verge_patch_side_effects(&plan, &patch)?;
            Config::verge().data().save_file()?;
            if split.clash_config.is_some() {
                Config::clash().data().save_config()?;
            }
            handle::Handle::refresh_verge();
            Ok::<_, anyhow::Error>(())
        }
        .await;

        if let Err(error) = finalize {
            let legacy_uncertainty = CompensationFailure::LegacyStateUncertain {
                message: format!("{error:#}"),
            };
            return compensate_legacy_verge_saga(
                self,
                committed,
                ClientError::Anyhow(error.context("failed to finalize legacy verge patch")),
                vec![legacy_uncertainty],
            )
            .await;
        }

        Ok(())
    }
}
