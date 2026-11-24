//! Shared lifecycle commands. Only the system proxy handle is host-specific.
use super::ports::{PreparedCoreBinary, ServiceTransitionLease};
use crate::client::reconciler::workflow::AdmissionCommand;
use chimera_config::application::ChimeraAppConfigPatch;
#[cfg(any(test, feature = "test-support"))]
use chimera_config::application::ClashCore;
use std::sync::{Arc, atomic::AtomicBool};

/// The Source commit is final even when the runtime is still pending in the
/// checked Core mailbox. Never report pending checked work as an inline apply.
pub struct ApplicationPatchResult {
    pub degradations: Vec<crate::client::mutation_outcome::Degradation>,
    /// Durable CAS version already delivered to the sole Reconciler.
    pub notified_version: Option<u64>,
}

pub enum Command<P> {
    #[cfg(any(test, feature = "test-support"))]
    ProfileMutation(Box<crate::client::reconciler::workflow_mutation::MutationRequest>),
    Shutdown,
    /// Explicit user intent, distinct from automatic reconciliation.
    StartCore,
    StopCore,
    PatchApplicationConfig {
        patch: ChimeraAppConfigPatch,
        service_connected: bool,
        /// Reply after the source commit; runtime failures are degradations,
        /// not evidence that the committed application.yaml rolled back.
        degradations: tokio::sync::oneshot::Sender<ApplicationPatchResult>,
        /// Captured by the old actor completion path until the ref single
        /// Reconciler owns both source-version and applied-target status.
        runtime_converged: Arc<AtomicBool>,
    },
    DisableStaleSystemProxy {
        system_proxy: P,
        expected_port: u16,
        expected_core_state_changed_at: i64,
        desired_before: bool,
        original: chimera_contracts::system_proxy::OsProxyConfig,
        notifications: Arc<dyn crate::client::effects::ports::CommitNotifications>,
        persist_legacy_mirror: Arc<dyn Fn() -> anyhow::Result<()> + Send + Sync>,
    },
    /// Retired production entry. Legacy actor tests retain the old adapter.
    #[cfg(any(test, feature = "test-support"))]
    SelectCore(ClashCore),
    RecoverCore,
    /// Re-evaluate the runtime owner after Service readiness changes.
    FollowService {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
    StartupReconcile {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
    RecoverStartup {
        operation_id: u64,
        transition: Box<dyn ServiceTransitionLease>,
    },
    RetryStartup {
        operation_id: u64,
        identity: String,
        transition: Box<dyn ServiceTransitionLease>,
    },
    CacheStartupUnsettled {
        operation_id: u64,
        reason: String,
    },
    Reconcile,
    /// The exact upstream checked bytes, handed from the only lifecycle
    /// mailbox into the existing CoreFacade mutation executor. No rebuild.
    ApplyChecked {
        prepared: super::ports::PreparedRuntime,
        core_spec: chimera_core_manager::CoreSpec,
        domain: crate::client::reconciler::committed::SourceDomain,
        version: u64,
        authority: Arc<dyn crate::client::reconciler::authority::RuntimeSourceAuthority>,
        interrupt_mode: bool,
    },
    /// One source notification requests a latest-committed-target round.
    ReconcileSource,
    /// Ref release-owed maintenance; no Core submit or startup handoff.
    MaintainServiceRelease,
    RetryDeferredProfiles {
        digest: String,
    },
    ReconcileProfiles {
        profiles: Arc<chimera_config::profile::Profiles>,
        staged_content: std::collections::BTreeMap<String, String>,
    },
    ReplaceCoreBinary(PreparedCoreBinary),
    InstallService(Box<dyn ServiceTransitionLease>),
    UninstallService(Box<dyn ServiceTransitionLease>),
    UpdateService(Box<dyn ServiceTransitionLease>),
    StartService {
        transition: Box<dyn ServiceTransitionLease>,
        ready_timeout: std::time::Duration,
    },
    RestartService {
        transition: Box<dyn ServiceTransitionLease>,
        ready_timeout: std::time::Duration,
    },
    StopService(Box<dyn ServiceTransitionLease>),
    ServiceEndpointDown(Box<dyn ServiceTransitionLease>),
}

pub fn admission_command<P>(command: &Command<P>) -> AdmissionCommand {
    match command {
        Command::StopCore => AdmissionCommand::Stop,
        Command::StartCore => AdmissionCommand::Start,
        Command::ApplyChecked {
            domain,
            version,
            prepared,
            ..
        } => AdmissionCommand::Checked {
            domain: *domain,
            version: *version,
            has_target: prepared.target.is_some(),
        },
        Command::Reconcile
        | Command::ReconcileSource
        | Command::ReconcileProfiles { .. }
        | Command::RetryDeferredProfiles { .. }
        | Command::RecoverCore
        | Command::RecoverStartup { .. }
        | Command::RetryStartup { .. }
        | Command::FollowService { .. } => AdmissionCommand::Automatic,
        _ => AdmissionCommand::Other,
    }
}

pub fn runtime_trigger<P>(command: &Command<P>) -> &'static str {
    match command {
        Command::StartCore => "Start",
        Command::StopCore => "Stop",
        Command::Shutdown => "Shutdown",
        Command::Reconcile => "Retry",
        Command::ReconcileSource | Command::ApplyChecked { .. } => "SourceCommitted",
        Command::MaintainServiceRelease => "ServiceRelease",
        Command::StartupReconcile { .. } => "Startup",
        Command::RetryStartup { .. } => "Retry",
        Command::FollowService { .. } => "ServiceReadiness",
        Command::ReplaceCoreBinary(_) => "BinaryReplaced",
        #[cfg(any(test, feature = "test-support"))]
        Command::ProfileMutation(_) => "Mutation",
        Command::RecoverCore => "CoreChanged",
        Command::RetryDeferredProfiles { .. } => "Tick",
        _ => "ServiceOperation",
    }
}
