//! Ports used by the client-owned core lifecycle boundary.

use async_trait::async_trait;
use chimera_config::clash::config::ClashConfig;
use chimera_core_manager::OperationId;
use serde::{Deserialize, Serialize};
use serde_yaml::Mapping;
use std::{path::PathBuf, sync::Arc};
use tempfile::TempDir;

pub(crate) use crate::core::actor_v2::CoreStatusSnapshot;

use crate::{
    client::application_workflow::mutation::{AppliedCandidate, CheckRecord, KnownRuntimeState},
    client::runtime::RuntimeApplyReceipt,
    client::runtime::RuntimeSnapshot,
    config::{
        chimera::ClashCore,
        profile::item_type::{ProfileUid, ScriptType},
    },
    core::clash::{api::ClashRuntimeConfig, core::RunType},
    enhance::PostProcessingOutput,
};

/// Owns the staged core binary until installation and any restart have finished.
pub(crate) struct PreparedCoreBinary {
    pub(crate) target: ClashCore,
    pub(crate) source: PathBuf,
    pub(crate) destination: PathBuf,
    pub(crate) staging: Arc<TempDir>,
    pub(crate) progress: Arc<dyn BinaryInstallProgress>,
}

pub(crate) trait BinaryInstallProgress: Send + Sync + 'static {
    fn restarting(&self);
    fn finished(&self, error: Option<&str>);
}

// Upstream core_install owns the port; the legacy UI keeps its progress wrapper.
pub(crate) use chimera_contracts::core_install::BinaryInstaller;

#[async_trait]
pub(crate) trait ServiceTransitionLease: Send {
    async fn report_endpoint_down(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn install_daemon(&mut self) -> anyhow::Result<()>;
    async fn uninstall_daemon(&mut self) -> anyhow::Result<()>;
    async fn update_daemon(&mut self) -> anyhow::Result<()>;
    async fn start_daemon(&mut self) -> anyhow::Result<()>;
    async fn restart_daemon(&mut self) -> anyhow::Result<()>;
    async fn stop_daemon(&mut self) -> anyhow::Result<()>;
    async fn confirm_ready(&mut self, timeout: std::time::Duration) -> anyhow::Result<()>;
    async fn confirm_stopped(&mut self) -> anyhow::Result<()>;
}

#[async_trait]
pub(crate) trait ServiceLifecyclePort: Send + Sync + 'static {
    async fn probe(&self) -> anyhow::Result<chimera_ipc::types::StatusInfo<'static>>;

    /// Ref ServiceActor is the authoritative readiness-generation source.
    /// Legacy test ports have no ref actor and return None.
    fn actor_status(&self) -> Option<chimera_core::actor_v2::service_status::ServiceHostStatus> {
        None
    }

    /// The actor's watch is used for production startup-recovery wakeups,
    /// rather than treating CoreLifecycle's compatibility watch as another
    /// readiness-generation publisher.
    fn actor_status_updates(
        &self,
    ) -> Option<
        tokio::sync::watch::Receiver<chimera_core::actor_v2::service_router::ServiceHostStatus>,
    > {
        None
    }

    fn runtime_owned(&self, info: &chimera_ipc::types::StatusInfo<'_>) -> bool {
        crate::core::service::is_service_runtime_owned(info)
    }
    /// Whether a Service command from an earlier transition has reached a
    /// terminal state. Startup must not make an owner decision while it runs.
    async fn command_settled(&self) -> bool {
        true
    }
    async fn report_endpoint_down(
        &self,
        transition: &mut dyn ServiceTransitionLease,
    ) -> anyhow::Result<()> {
        transition.report_endpoint_down().await
    }
    fn restart_policy(
        &self,
    ) -> chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
        chimera_core::actor_v2::service_policy::ServiceRestartPolicySnapshot {
            attempts: 0,
            exhausted: false,
        }
    }
    async fn begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>>;
    /// Try to acquire the host transition lease without waiting. This is for
    /// work running inside the serialized CoreLifecycle actor: waiting here
    /// could deadlock behind a transition caller queued on that same actor.
    async fn try_begin_transition(&self) -> anyhow::Result<Box<dyn ServiceTransitionLease>> {
        anyhow::bail!("this Service adapter does not support nonblocking transitions")
    }
}

/// Narrow boundary around the running core's `/configs` API.
///
/// The legacy API remains behind this port while the ref core lifecycle is
/// migrated. Keeping read and patch together lets the transaction path verify
/// the applied state without reaching into the global core manager directly.
#[async_trait]
pub(crate) trait RunningConfigPort: Send + Sync {
    async fn read(&self) -> anyhow::Result<ClashRuntimeConfig>;
    async fn patch(&self, patch: &Mapping) -> anyhow::Result<()>;
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeTransformFailureDiagnostics {
    pub attempt_revision: u64,
    pub transform_uid: ProfileUid,
    pub scope_uid: Option<ProfileUid>,
    pub script_type: Option<ScriptType>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct RuntimeTransformDiagnostics {
    pub revision: u64,
    pub output: PostProcessingOutput,
    pub failure: Option<RuntimeTransformFailureDiagnostics>,
}

/// Checked Core apply result after the lower owner confirmed its revision.
/// The legacy port lacks replacement evidence: unknown must not be treated
/// as an in-place config patch when deciding whether to interrupt a Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CheckedApplyOutcome {
    pub replaced: Option<bool>,
}

/// The Core's result and its *separate* derived-product publication verdict.
/// `Applied` is never inferred from a successful source commit. Production
/// must return the actual accepted receipt and the exact submitted product;
/// old compatibility ports leave those fields absent rather than inventing
/// a Core/RuntimeProduct identity.
#[derive(Debug, Clone)]
pub(crate) struct PreparedCoreSettlement {
    /// A complete verified report from the Core router, when this port can
    /// prove its host/generation and accepted revision. No synthetic report
    /// is constructed for legacy ports without the owner evidence.
    pub report: Option<Box<chimera_core::actor_v2::facade::ReconcileReport>>,
    /// Terminal outcome observed directly from this operation's Core registry.
    /// This is not a substitute for the separate router status proof.
    pub outcome: Option<chimera_ipc::api::core::v2::ReconcileOutcomeInfo>,
    pub replaced: Option<bool>,
    pub confirmed: Option<Arc<RuntimeApplyReceipt>>,
    pub product: Option<Arc<crate::client::runtime::RuntimeSnapshot>>,
    pub publication: PreparedProductPublication,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreparedProductPublication {
    Published,
    Pending { reason: String },
    Unobserved,
}

/// A terminal Core command may have restored its old revision, or its reply
/// may have become unobservable. Neither case is an AppliedCandidate.
#[derive(Debug, Clone)]
pub(crate) enum PreparedSubmission {
    Applied(PreparedCoreSettlement),
    RolledBack(chimera_ipc::api::core::v2::ReconcileOutcomeInfo),
    Unknown(chimera_core_manager::CoreError),
}

#[derive(Debug, thiserror::Error)]
#[error("core runtime apply was rolled back: {reason}")]
pub(crate) struct PreparedRolledBack {
    pub report: chimera_ipc::api::core::v2::ReconcileOutcomeInfo,
    pub reason: String,
}

impl PreparedCoreSettlement {
    /// The real accepted candidate carries one exact source, Core revision,
    /// product and digest. Old test/compatibility adapters are explicitly
    /// Unobserved; a half-filled receipt may never pretend to be Applied.
    pub(crate) fn validate_identity(&self, target: &str) -> anyhow::Result<()> {
        match (&self.confirmed, &self.product) {
            (None, None)
                if self.publication == PreparedProductPublication::Unobserved
                    && self.outcome.is_none()
                    && self.report.is_none() =>
            {
                Ok(())
            }
            (Some(receipt), Some(product)) => {
                anyhow::ensure!(
                    receipt.target.as_deref() == Some(target),
                    "confirmed Core receipt belongs to a different committed source"
                );
                anyhow::ensure!(
                    receipt.revision == product.revision
                        && receipt.target_core == product.target_core
                        && receipt.config_digest
                            == chimera_core_manager::payload_digest(product.product_bytes()),
                    "confirmed Core receipt does not match the accepted runtime product"
                );
                anyhow::ensure!(
                    receipt
                        .artifact
                        .as_ref()
                        .is_some_and(|saved| Arc::ptr_eq(saved, product)),
                    "confirmed Core receipt did not retain the exact submitted product"
                );
                if let Some(outcome) = &self.outcome {
                    use chimera_ipc::api::core::v2::ReconcileOutcomeKind;
                    anyhow::ensure!(
                        outcome.revision == receipt.applied_revision
                            && outcome.outcome != ReconcileOutcomeKind::RolledBack,
                        "applied candidate carried a rolled-back or mismatched Core revision"
                    );
                    anyhow::ensure!(
                        self.replaced
                            == Some(matches!(
                                outcome.outcome,
                                ReconcileOutcomeKind::Started
                                    | ReconcileOutcomeKind::Restarted
                                    | ReconcileOutcomeKind::Switched
                            )),
                        "Core replacement verdict does not match the reported transaction"
                    );
                }
                if let Some(report) = &self.report {
                    anyhow::ensure!(
                        report.applied.host == receipt.host
                            && report.applied.revision == receipt.applied_revision
                            && report.status.host == receipt.host
                            && report.status.generation == report.applied.generation
                            && report.status.snapshot.as_ref().is_some_and(
                                |snapshot| receipt.confirms(report.status.host, snapshot)
                            )
                            && self.outcome.as_ref().is_some_and(|outcome| report.output
                                == chimera_ipc::api::core::v2::OperationOutputInfo::Reconciled(
                                    outcome.clone()
                                )),
                        "Core ReconcileReport is not bound to the accepted host, revision and terminal operation"
                    );
                }
                Ok(())
            }
            _ => anyhow::bail!("prepared Core result has no coherent accepted product receipt"),
        }
    }

    pub(crate) fn require_published(self) -> anyhow::Result<CheckedApplyOutcome> {
        if let PreparedProductPublication::Pending { reason } = self.publication {
            anyhow::bail!("Core applied but RuntimeProduct publication failed: {reason}");
        }
        Ok(CheckedApplyOutcome {
            replaced: self.replaced,
        })
    }
}

#[async_trait]
pub(crate) trait CoreLifecyclePort: Send + Sync {
    /// The canonical prepared-runtime session, only present when this port
    /// delegates to the active LocalRuntimeHost. A compatibility port without
    /// that owner cannot safely start a new independent revision/port pool.
    fn runtime_preparation_session(
        &self,
    ) -> Option<crate::client::ports::RuntimePreparationSession> {
        None
    }

    async fn reconcile_transparent_proxy(
        &self,
        request: &chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyRequest,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        use chimera_ipc::api::network::transparent_proxy::{
            NetworkTransparentProxyMode::Disabled, NetworkTransparentProxyStatus,
        };
        if request.mode == Disabled {
            return Ok(NetworkTransparentProxyStatus {
                supported: false,
                active: false,
                mode: None,
                revision: None,
                error: None,
            });
        }
        anyhow::bail!("this core lifecycle adapter does not support transparent capture")
    }

    async fn transparent_proxy_status(
        &self,
    ) -> anyhow::Result<chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus>
    {
        Ok(
            chimera_ipc::api::network::transparent_proxy::NetworkTransparentProxyStatus {
                supported: false,
                active: false,
                mode: None,
                revision: None,
                error: None,
            },
        )
    }

    async fn reconcile(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
    ) -> anyhow::Result<()>;
    /// The upstream checked-product apply port. No adapter may re-build the
    /// candidate at this boundary; unsupported ports must explicitly refuse.
    async fn reconcile_prepared(
        &self,
        _prepared: PreparedRuntime,
        _core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this CoreLifecycle port cannot submit checked runtime bytes")
    }

    /// Ref AppliedCandidate.replaced: only production hosts that retain the
    /// authoritative Core reconcile verdict may report Some(replaced).
    /// Legacy test adapters execute the original port and report Unknown.
    async fn reconcile_prepared_outcome(
        &self,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<CheckedApplyOutcome> {
        self.reconcile_prepared(prepared, core_spec).await?;
        Ok(CheckedApplyOutcome { replaced: None })
    }

    /// Ref's AppliedCandidate boundary. Unlike the compatibility Result<bool>
    /// entry above, a confirmed Core revision may be successful even when
    /// publishing its derived RuntimeProduct fails. Only a real production
    /// owner can supply the corresponding receipt and product evidence.
    async fn reconcile_prepared_settlement(
        &self,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<PreparedCoreSettlement> {
        let applied = self.reconcile_prepared_outcome(prepared, core_spec).await?;
        Ok(PreparedCoreSettlement {
            report: None,
            outcome: None,
            replaced: applied.replaced,
            confirmed: None,
            product: None,
            publication: PreparedProductPublication::Unobserved,
        })
    }

    /// An observed, typed result of the actual Core operation. Production
    /// overrides this instead of flattening RolledBack and Unknown into
    /// general anyhow errors. Existing compatibility ports provide only
    /// the safe legacy Applied/Err boundary.
    async fn reconcile_prepared_submission(
        &self,
        prepared: PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
    ) -> anyhow::Result<PreparedSubmission> {
        Ok(PreparedSubmission::Applied(
            self.reconcile_prepared_settlement(prepared, core_spec)
                .await?,
        ))
    }

    /// Capture the *previous* running Core's instance-bound API before
    /// replacing/patching it. Missing proof is not a license to interrupt
    /// whichever Core happens to be running after the apply.
    async fn capture_interruption_source(
        &self,
    ) -> anyhow::Result<Option<crate::core::actor_v2::api::ApiClient>> {
        anyhow::bail!("this Core port cannot prove the pre-apply API instance")
    }

    /// Stop a Service-hosted Core after startup evidence proved that this
    /// instance owns it. The caller must re-probe before starting Local.
    async fn retire_service_core(
        &self,
        _expected_owner: chimera_core_manager::CoreOwner,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this core lifecycle adapter cannot retire a Service-hosted Core")
    }
    /// Ask the Service manager to recover any quarantined Core epochs. This
    /// succeeds only after the manager proves those processes dead and cleans
    /// their runtime artifacts.
    async fn recover_service_quarantine(&self) -> anyhow::Result<()> {
        anyhow::bail!("this core lifecycle adapter cannot recover Service quarantine")
    }
    /// Reconcile Local after fresh evidence established that this application
    /// no longer owns the shared Service core. The production adapter skips
    /// the generic Service Stop handoff here.
    async fn reconcile_after_service_release(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
    ) -> anyhow::Result<()> {
        self.reconcile(clash, target_core, RunType::Normal).await
    }
    /// Candidate Profile input for the ref-aligned runtime builder. Existing
    /// lifecycle adapters can retain their compatibility behavior until they
    /// implement the typed snapshot path.
    async fn reconcile_profiles(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        run_type: RunType,
        _profiles: Arc<chimera_config::profile::Profiles>,
        _app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
        _target: Option<String>,
    ) -> anyhow::Result<()> {
        self.reconcile(clash, target_core, run_type).await
    }
    async fn reconcile_profiles_after_service_release(
        &self,
        clash: ClashConfig,
        target_core: ClashCore,
        profiles: Arc<chimera_config::profile::Profiles>,
        app: chimera_config::application::ChimeraAppConfig,
        staged_content: std::collections::BTreeMap<String, String>,
        target: Option<String>,
    ) -> anyhow::Result<()> {
        self.reconcile_profiles(
            clash,
            target_core,
            RunType::Normal,
            profiles,
            app,
            staged_content,
            target,
        )
        .await
    }
    async fn validate_profile_runtime(
        &self,
        _clash: ClashConfig,
        _target_core: ClashCore,
        _run_type: RunType,
        _profiles: Arc<chimera_config::profile::Profiles>,
        _app: chimera_config::application::ChimeraAppConfig,
        _staged_content: std::collections::BTreeMap<String, String>,
    ) -> anyhow::Result<CheckRecord> {
        anyhow::bail!("this core lifecycle adapter cannot validate a typed Profile runtime")
    }
    /// Profile TCC's precommit Try: consume the same Core-owned prepared
    /// candidate as source Checked, but retain its staged file until the
    /// source transaction explicitly Confirms or Cancels. Only the production
    /// Core host supports this path; no second revision allocator is allowed.
    async fn prepare_profile_runtime_prepared(
        &self,
        _prepared: PreparedRuntime,
        _core_spec: chimera_core_manager::CoreSpec,
        _run_type: RunType,
        _operation_id: OperationId,
    ) -> anyhow::Result<(AppliedCandidate, CheckRecord)> {
        anyhow::bail!("this Core adapter cannot hold a prepared Profile TCC candidate")
    }

    async fn confirm_profile_runtime(
        &self,
        _operation_id: OperationId,
        _candidate: AppliedCandidate,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this core lifecycle adapter cannot confirm a held Profile runtime")
    }
    async fn discard_profile_runtime(&self, _operation_id: OperationId) {}
    async fn stop(&self) -> anyhow::Result<()>;
    /// Drain the full Core Actor and its in-process executor at application
    /// exit. Unlike Stop Core, this is terminal for the current session.
    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        Ok(())
    }
    async fn change_core(&self, clash_core: ClashCore) -> anyhow::Result<()>;
    async fn status(&self) -> anyhow::Result<CoreStatusSnapshot>;
    /// Whether the shared Core router currently reports an unreachable Service
    /// endpoint. Compatibility ports without the ref router report `false`.
    fn service_endpoint_degraded(&self) -> bool {
        false
    }
    fn service_endpoint_recovery_supported(&self) -> bool {
        false
    }
    /// Clear a completed operation retained by the shared Core facade before
    /// another endpoint recovery is submitted. `Err` means prior work is still
    /// pending, so the retry budget must not be spent yet.
    async fn settle_service_recovery_action(&self) -> Option<Result<(), String>> {
        None
    }
    /// Reconnect and re-adopt the Service endpoint through the shared Core
    /// router. `None` is reserved for compatibility fixtures that still use
    /// the legacy Service transition adapter.
    async fn recover_service_endpoint(
        &self,
        _closing: &tokio_util::sync::CancellationToken,
    ) -> Option<Result<(), chimera_core_manager::CoreError>> {
        None
    }
    /// Select an already proven execution host without applying a runtime.
    /// Startup uses this between the host handoff (S2) and owner verification
    /// (S3), matching the reference lifecycle's separate `move_execution_host`
    /// and `apply_committed` operations.
    /// Only the caller that already proved the previous Service Core was
    /// released may select Local with `source_released=true`. This must not
    /// submit any runtime bytes; the subsequent PreparedRuntime submission
    /// remains a separate single-owner Core operation.
    async fn select_local_after_confirmed_service_release(&self) -> anyhow::Result<()> {
        anyhow::bail!(
            "this lifecycle adapter cannot select Local after a confirmed Service release"
        )
    }

    async fn select_execution_host(
        &self,
        host: crate::core::actor_v2::control_endpoint::ExecutionHost,
    ) -> anyhow::Result<()> {
        use crate::core::actor_v2::control_endpoint::ExecutionHost;

        let status = self.status().await?;
        let selected = match status.run_type {
            RunType::Normal => ExecutionHost::Local,
            RunType::Service => ExecutionHost::Service,
            RunType::Elevated => ExecutionHost::Local,
        };
        anyhow::ensure!(
            selected == host,
            "the lifecycle adapter cannot select {host:?}"
        );
        Ok(())
    }
    async fn confirmed_runtime_receipt(&self) -> anyhow::Result<Option<Arc<RuntimeApplyReceipt>>> {
        Ok(None)
    }
    async fn active_runtime_receipt(&self) -> anyhow::Result<Option<Arc<RuntimeApplyReceipt>>> {
        Ok(None)
    }

    /// Read-only and instance-bound: unsupported compatibility ports return no view.
    async fn active_effective_config(
        &self,
    ) -> anyhow::Result<Option<chimera_ipc::api::core::v2::CoreEffectiveConfig>> {
        Ok(None)
    }
    fn last_confirmed_runtime_receipt(&self) -> Option<Arc<RuntimeApplyReceipt>> {
        None
    }
    async fn observe_runtime_baseline(&self) -> anyhow::Result<KnownRuntimeState> {
        anyhow::bail!(
            "this core lifecycle adapter does not expose authoritative runtime baseline evidence"
        )
    }
    async fn restore_runtime_receipt(
        &self,
        _receipt: Arc<RuntimeApplyReceipt>,
    ) -> anyhow::Result<()> {
        anyhow::bail!("this core lifecycle adapter cannot restore a confirmed runtime receipt")
    }
    async fn restore_runtime_baseline(&self, baseline: &KnownRuntimeState) -> anyhow::Result<()> {
        match baseline {
            KnownRuntimeState::Applied(receipt) => {
                self.restore_runtime_receipt(receipt.clone()).await
            }
            KnownRuntimeState::Stopped | KnownRuntimeState::NeverApplied => self.stop().await,
        }
    }
    fn recovery_notify(&self) -> Option<Arc<tokio::sync::Notify>>;
    fn outcome_uncertain(&self) -> bool {
        false
    }

    fn runtime_transform_diagnostics(&self) -> anyhow::Result<Option<RuntimeTransformDiagnostics>> {
        Ok(None)
    }

    fn promoted_runtime_snapshot(&self) -> Option<Arc<RuntimeSnapshot>> {
        None
    }

    /// Ref pending-product maintenance: no new Core submit is permitted.
    /// The production bridge checks the currently accepted runtime receipt.
    fn pending_runtime_product_target(&self) -> Option<String> {
        None
    }
    async fn republish_pending_runtime_product(&self, _target: &str) -> anyhow::Result<bool> {
        Ok(false)
    }

    fn effective_clash_info(&self) -> crate::config::clash::ClashInfo {
        crate::config::core::Config::clash()
            .latest()
            .get_client_info()
    }

    async fn active_clash_info(&self) -> anyhow::Result<crate::config::clash::ClashInfo> {
        let status = self.status().await?;
        if !matches!(status.state, chimera_ipc::api::status::CoreState::Running) {
            anyhow::bail!("the core API is unavailable because the core is not running");
        }
        Ok(self.effective_clash_info())
    }

    /// Acquires the running controller API through the lifecycle owner. The
    /// legacy default remains only for compatibility ports without a v2 host
    /// endpoint; production bridges override it with an instance-bound client.
    async fn clash_api_client(&self) -> anyhow::Result<crate::core::actor_v2::api::ApiClient> {
        crate::core::actor_v2::api::ApiClient::new(self.active_clash_info().await?)
    }

    async fn on_profile_change(&self, break_when: bool);
}

// Pinned ref/backend/nyanpasu-core/src/client/core_lifecycle/ports.rs:
// Typed, read-only candidate build and publication boundary. Its prepared
// value does not apply anything to a running Core.
use super::super::runtime;
use chimera_contracts::publish::PublishRuntimeError;
use chimera_core::client::core_spec::CoreSpecError;

/// Application-owned runtime preparation; implementations never call the workflow actor.
#[async_trait]
pub(in crate::client) trait RuntimePreparationPort: Send + Sync {
    async fn publish(&self, snapshot: &runtime::RuntimeSnapshot)
    -> Result<(), PublishRuntimeError>;
    fn core_spec(
        &self,
        core: &chimera_config::application::ClashCore,
    ) -> Result<chimera_core_manager::CoreSpec, CoreSpecError>;
}

pub(crate) struct PreparedRuntime {
    pub snapshot: Arc<runtime::RuntimeSnapshot>,
    /// The to-be-committed bytes, serialized exactly once so the advisory
    /// check and the reconcile cannot drift apart.
    pub intent: Arc<chimera_core::actor_v2::intent::RuntimeIntent>,
    /// The ports this candidate would bind. Inert: only the receipt of an
    /// apply that succeeded may confirm them.
    pub ports: crate::client::ports::CandidatePortBindings,
    /// The committed target the build came from (`RuntimeInputs::target_key`),
    /// when it has one.
    pub target: Option<String>,
}
