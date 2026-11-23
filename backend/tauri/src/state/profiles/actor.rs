//! ProfilesActor: single owner of the profiles document.
//! Tauri-free; every filesystem/network effect goes through the ports.

use std::{collections::HashMap, sync::Arc, time::Duration};

use chimera_config::profile::{
    ConfigDefinition, ExternalMode, FileConfig, FolderId, LocalBinding, ManagedProfilePath,
    MaterializedFile, NewProfileRequest, ProfileDefinition, ProfileDependencyIndex, ProfileFolder,
    ProfileId, ProfileItem, ProfileLayout, ProfileMetadata, ProfileMetadataPatch,
    ProfileRevisionError, ProfileSource, ProfileValidationError, Profiles, ProxyGroupFolder,
    ProxyGroupLayout, RemoteProfileOptions, RemoteProfileOptionsPatch, RemoteSyncInterval,
    ScriptRuntime, SubscriptionInfo, TransformDefinition,
};
use chimera_contracts::profiles::{
    command_failure::{
        CommitFailureReason, MaterializationOperation, ProfileContentError, ProfileFailure,
    },
    error::{ProfileFileError, SubscriptionFetchError},
};
use chimera_contracts::{
    mutation::CommitAborted,
    runtime_error::{RuntimeCheckUnavailable, RuntimeError},
};
use chimera_core::state::{PersistentStateManager, ReplaceIfVersionResult, Version};
use chimera_core_manager::OperationId;
use ractor::{Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use crate::{
    client::application_workflow::{
        impact::{
            self, ActivationIntent, ContentDigest, MutationHints, RequestedRuntimeFields,
            RuntimeImpact, StagedResourceToken, TouchedContent,
        },
        policy::CommandClass,
    },
    core::migration::modules::profiles::ProfilesFormat,
    state::mutation::{MutationCoordinator, ProfileContentLock},
};
use tokio::task::JoinHandle;

use super::{
    jobs::{ProfileJobs, ProfileSyncContext},
    ports::{
        MaterializationReconcileReport, MaterializationResource, PreparedCleanup,
        ProfileDegradation, ProfileDegradationCode, ProfileDegradationPhase, ProfileFsPort,
        ProfileMaterializationPort, SubscriptionFetcher,
    },
    scheduler::ExternalWatchers,
};

/// Actor-owned recovery pass over durable materialization/cleanup journals.
/// Background work only casts [`ProfilesActorMessage::ReconcileMaterializations`];
/// the actor performs the blocking reconcile under message serialization.
const MATERIALIZATION_RECONCILE_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, thiserror::Error)]
pub enum ProfilesError {
    #[error("profile not found: {0}")]
    ProfileNotFound(ProfileId),
    #[error(
        "profile {uid} is referenced and cannot be deleted (referrers: {referrers:?}, current: {current}, global_transforms: {global_transforms})"
    )]
    ProfileInUse {
        uid: ProfileId,
        referrers: Vec<ProfileId>,
        /// Referenced by the document-level `current` selection.
        current: bool,
        /// Referenced by the document-level `global_transforms` list.
        global_transforms: bool,
    },
    #[error("profile has no materialized file")]
    ProfileHasNoFile,
    #[error("validation failed: {0:?}")]
    ValidationFailed(Vec<ProfileValidationError>),
    #[error("the reorder list has {got} entries, expected {expected}")]
    ReorderListSizeMismatch { expected: usize, got: usize },
    #[error("the reorder list repeats {uid}")]
    ReorderListDuplicate { uid: ProfileId },
    #[error("profile is not a remote profile")]
    NotARemoteProfile,
    #[error("file not writable: {reason}")]
    FileNotWritable { reason: String },
    #[error("refresh failed: {message}")]
    RefreshFailed { message: String },
    #[error("import failed: {message}")]
    ImportFailed { message: String },
    #[error("failed to persist profiles: {0}")]
    Persist(String),
    #[error("Profiles runtime candidate check failed: {0}")]
    RuntimeCheck(String),
    /// A source candidate failed before the file/Journal/MVCC transaction
    /// began. The REF CommitAborted variant distinguishes real rejection
    /// from an unavailable verdict and keeps the actual check cause.
    #[error("the profiles change was not committed: {source}")]
    Commit {
        #[source]
        source: CommitAborted,
    },
    #[error("profiles source transaction failed: {source}; staging cleanup: {cleanup:?}")]
    SourceTransaction {
        #[source]
        source: chimera_core::state::ReplaceIfVersionError,
        /// Derived from the actual conditional-write result, never a string
        /// guessed from a user-visible error message.
        reason: Option<CommitFailureReason>,
        cleanup: Option<String>,
        /// Unmodified error objects from refused pre-commit compensations.
        cleanup_failures: Vec<anyhow::Error>,
    },
    #[error("profiles state version conflict: expected {expected}, actual {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    #[error(
        "profiles state version conflict: expected {expected}, actual {actual}; staging cleanup failed: {cleanup}"
    )]
    ConflictCleanup {
        expected: u64,
        actual: u64,
        cleanup: String,
        /// The exact independent compensation/cancel errors, if any.
        cleanup_failures: Vec<anyhow::Error>,
    },
    #[error("failed to advance profile revision: {0}")]
    Revision(#[from] ProfileRevisionError),
    #[error("profile materialization failed: {0}")]
    Materialization(String),
    /// Retain the exact filesystem/journal port failure and its causal chain
    /// instead of discarding it when crossing spawn_blocking/anyhow.
    #[error("profile materialization failed: {context}: {source}")]
    MaterializationFile {
        context: &'static str,
        operation: Option<MaterializationOperation>,
        #[source]
        source: ProfileFileError,
    },
    #[error("subscription download failed for {url}: {source}")]
    FetchSubscription {
        url: url::Url,
        #[source]
        source: SubscriptionFetchError,
    },
    #[error("the downloaded content is not a valid profile: {source}")]
    ProfileContentRejected {
        #[source]
        source: ProfileContentError,
    },
    #[error("profiles actor rpc failed: {0}")]
    Rpc(String),
    #[error("profile sync job failed: {0}")]
    ProfileSyncJobs(#[from] chimera_jobs::Error),
    #[error("application is shutting down")]
    ShuttingDown,
}

impl ProfilesError {
    /// Project only real, shape-preserving profile command failures into the
    /// shared Runtime Values boundary. The remaining variants keep their
    /// original Tauri owner and source chain; never fabricate a file, commit
    /// or materialization reason from a diagnostic string.
    pub(crate) fn typed_failure(&self) -> Option<ProfileFailure> {
        match self {
            Self::ProfileNotFound(uid) => {
                Some(ProfileFailure::ProfileNotFound { uid: uid.clone() })
            }
            Self::ValidationFailed(errors) => Some(ProfileFailure::ValidationFailed {
                errors: errors.clone(),
            }),
            Self::VersionConflict { expected, actual } => Some(ProfileFailure::VersionConflict {
                expected: *expected,
                actual: *actual,
                cleanup: None,
            }),
            Self::ConflictCleanup {
                expected,
                actual,
                cleanup,
                ..
            } => Some(ProfileFailure::VersionConflict {
                expected: *expected,
                actual: *actual,
                cleanup: Some(cleanup.clone()),
            }),
            Self::Revision(ProfileRevisionError::Overflow) => {
                Some(ProfileFailure::RevisionOverflow)
            }
            Self::SourceTransaction {
                reason: Some(reason),
                ..
            } => Some(ProfileFailure::Commit { reason: *reason }),
            Self::Commit {
                source: CommitAborted::RuntimeRefused { .. },
            } => Some(ProfileFailure::Commit {
                reason: CommitFailureReason::RuntimeRefused,
            }),
            Self::Commit {
                source: CommitAborted::RuntimeFailed { .. },
            } => Some(ProfileFailure::Commit {
                reason: CommitFailureReason::RuntimeFailed,
            }),
            Self::ProfileHasNoFile => Some(ProfileFailure::ProfileHasNoFile),
            Self::NotARemoteProfile => Some(ProfileFailure::NotARemoteProfile),
            Self::ProfileInUse {
                uid,
                referrers,
                current,
                global_transforms,
            } => Some(ProfileFailure::ProfileInUse {
                uid: uid.clone(),
                referrers: referrers.clone(),
                current: *current,
                global_transforms: *global_transforms,
            }),
            Self::ReorderListSizeMismatch { expected, got } => {
                Some(ProfileFailure::ReorderListSizeMismatch {
                    expected: *expected,
                    got: *got,
                })
            }
            Self::ReorderListDuplicate { uid } => {
                Some(ProfileFailure::ReorderListDuplicate { uid: uid.clone() })
            }
            Self::FileNotWritable { .. }
            | Self::RefreshFailed { .. }
            | Self::ImportFailed { .. }
            | Self::Persist(_)
            | Self::RuntimeCheck(_)
            | Self::Commit { .. }
            | Self::SourceTransaction { reason: None, .. }
            | Self::Materialization(_)
            | Self::MaterializationFile { .. }
            | Self::FetchSubscription { .. }
            | Self::ProfileContentRejected { .. }
            | Self::Rpc(_)
            | Self::ProfileSyncJobs(_)
            | Self::ShuttingDown => None,
        }
    }
}

/// Both compensation branches can fail after a source write refusal.
/// Keep each original anyhow/port error available through downcast, rather
/// than replacing two filesystem/journal failures with a debug-formatted line.
#[derive(Debug, thiserror::Error)]
#[error("resource recovery: {resource:?}; cleanup recovery: {cleanup:?}")]
pub struct MaterializationRecoveryFailure {
    pub resource: Option<anyhow::Error>,
    pub cleanup: Option<anyhow::Error>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CommitReport {
    pub snapshot: Arc<Profiles>,
    /// Dependency-closure judgement per the T04 affects_current rule table.
    pub affects_current: bool,
    /// Crate-internal detail for committed mutations that left maintenance work.
    pub(crate) degradations: Vec<ProfileDegradation>,
    pub(crate) receipt: crate::client::runtime::CommitReceipt,
    pub(crate) runtime_degradations: Vec<crate::client::runtime::Degradation>,
    /// Server-generated uid (D13); set by Add / import commit, consumed by
    /// facade auto-activation (design §9).
    pub created: Option<ProfileId>,
}

#[derive(Debug, Clone)]
pub enum ReorderOp {
    Move { active: ProfileId, over: ProfileId },
    ByList(Vec<ProfileId>),
}

pub struct ProfilesActorArgs {
    pub(crate) mutations: MutationCoordinator,
    pub manager: PersistentStateManager<Profiles, ProfilesFormat>,
    pub(crate) content_lock: ProfileContentLock,
    pub fs: Arc<dyn ProfileFsPort>,
    pub fetcher: Arc<dyn SubscriptionFetcher>,
    pub(crate) materialization: Arc<dyn ProfileMaterializationPort>,
    pub(crate) jobs: ProfileJobs,
}

pub struct ProfilesActorState {
    mutations: MutationCoordinator,
    manager: PersistentStateManager<Profiles, ProfilesFormat>,
    content_lock: ProfileContentLock,
    index: ProfileDependencyIndex,
    fs: Arc<dyn ProfileFsPort>,
    fetcher: Arc<dyn SubscriptionFetcher>,
    materialization: Arc<dyn ProfileMaterializationPort>,
    pending_refresh: HashMap<ProfileId, PendingRefresh>,
    next_refresh_token: u64,
    pending_imports: HashMap<ImportOperationToken, PendingImport>,
    next_import_token: u64,
    gate: ProducerGate,
    jobs: ProfileJobs,
    jobs_revision: u64,
    external_watchers: ExternalWatchers,
    /// Periodic journal recovery. Background task only casts; actor owns work.
    reconcile_task: Option<JoinHandle<()>>,
}

/// Background profile producers stay held until core startup reconciliation
/// finishes, then stop admitting work when the actor shuts down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProducerGate {
    Held,
    Running,
}

struct PendingRefresh {
    context: Option<ProfileSyncContext>,
    token: u64,
    reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    task: JoinHandle<()>,
}

/// In-memory handle for one fetch-before-commit import. Never durable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ImportOperationToken(u64);

struct PendingImport {
    reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    metadata: ProfileMetadata,
    url: url::Url,
    option: RemoteProfileOptions,
    update_interval_explicit: bool,
    folder: Option<FolderId>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RefreshOrigin {
    Manual,
    Scheduled,
}

/// A download/validation failure retains its original typed cause across
/// the actor mailbox, while legacy mock/adapter errors keep their old text.
#[derive(Debug)]
pub enum RefreshFailureSource {
    Subscription {
        url: url::Url,
        source: SubscriptionFetchError,
    },
    Content(ProfileContentError),
}

#[derive(Debug)]
pub enum RefreshOutcome {
    Succeeded {
        subscription: chimera_config::profile::SubscriptionInfo,
        suggested_update_interval_minutes: Option<u64>,
        /// Validated payload; written to the materialized file inside the
        /// commit handler so a stale download can be fenced before any write.
        content: String,
        /// Server-provided display name (`profile-title` / `Content-Disposition`),
        /// applied to a non-user-named profile by name-sync in the commit handler.
        filename: Option<String>,
    },
    Failed {
        message: String,
        source: Option<RefreshFailureSource>,
    },
}

/// Decide the profile name to apply after a refresh. Returns `Some(name)` only
/// when the profile is not user-named and the server supplied a non-blank name;
/// otherwise the current name is kept. Pure so the provenance rule is unit-tested
/// without spawning the actor.
fn synced_name(custom_name: bool, filename: &Option<String>) -> Option<String> {
    if custom_name {
        return None;
    }
    filename
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

#[derive(Debug)]
#[allow(dead_code)]
pub enum ProfilesActorMessage {
    SaveFile {
        uid: ProfileId,
        content: String,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetCurrent {
        current: Option<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Activate `uid` only if nothing is currently selected. The reply is
    /// `Some(report)` when it activated, `None` when a current already existed.
    SetCurrentIfNone {
        uid: ProfileId,
        reply: RpcReplyPort<Result<Option<CommitReport>, ProfilesError>>,
    },
    SetGlobalTransforms {
        ids: Vec<ProfileId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SetValidFields {
        fields: Vec<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Replace {
        profiles: Profiles,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Add {
        request: NewProfileRequest,
        initial_file: Option<String>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Delete {
        uid: ProfileId,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    Reorder {
        op: ReorderOp,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Replace folders and display order in a single durable commit.
    SetLayout {
        layout: ProfileLayout,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Move selected members into a newly minted folder in one commit.
    CreateFolder {
        name: String,
        members: Vec<ProfileId>,
        reply: RpcReplyPort<Result<(CommitReport, FolderId), ProfilesError>>,
    },
    SetProxyGroupLayout {
        profile: ProfileId,
        layout: ProxyGroupLayout,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    CreateProxyGroupFolder {
        profile: ProfileId,
        name: String,
        members: Vec<String>,
        reply: RpcReplyPort<Result<(CommitReport, FolderId), ProfilesError>>,
    },
    PatchMetadata {
        uid: ProfileId,
        patch: ProfileMetadataPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    PatchRemoteOptions {
        uid: ProfileId,
        patch: RemoteProfileOptionsPatch,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    SyncRemote {
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
        context: ProfileSyncContext,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    RefreshRemote {
        uid: ProfileId,
        patch: Option<RemoteProfileOptionsPatch>,
        origin: RefreshOrigin,
        context: Option<ProfileSyncContext>,
        reply: Option<RpcReplyPort<Result<CommitReport, ProfilesError>>>,
    },
    CommitRefreshed {
        uid: ProfileId,
        token: u64,
        /// The URL and serialized definition fingerprint the download started
        /// for. Commit is discarded if either stale fence changed in flight.
        url: url::Url,
        definition_fingerprint: String,
        outcome: RefreshOutcome,
    },
    /// Fetch-before-commit remote import. No durable placeholder is written
    /// until download + validation succeed and the caller is still live.
    ImportRemote {
        url: url::Url,
        metadata: ProfileMetadata,
        option: RemoteProfileOptions,
        update_interval_explicit: bool,
        folder: Option<FolderId>,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    CommitImported {
        token: ImportOperationToken,
        outcome: RefreshOutcome,
    },
    ExternalFileChanged {
        uid: ProfileId,
    },
    ReplaceDefinition {
        uid: ProfileId,
        definition: ProfileDefinition,
        reply: RpcReplyPort<Result<CommitReport, ProfilesError>>,
    },
    /// Recover durable materialization/cleanup journals against the committed
    /// profiles snapshot. Cast-only from the actor-owned periodic task and
    /// handled serially with all other mutations.
    ReconcileMaterializations,
    StartProducers {
        reply: RpcReplyPort<Result<(), ProfilesError>>,
    },
}

pub struct ProfilesActor;

pub(super) enum AffectsRule {
    Never,
    CurrentChanged,
    GlobalChanged,
    Touched(ProfileId),
    Always,
}

impl ProfilesActor {
    fn current_state(state: &ProfilesActorState) -> Profiles {
        state.manager.snapshot_handle().load().state.clone()
    }

    pub(crate) fn current_closure(profiles: &Profiles) -> indexmap::IndexSet<ProfileId> {
        chimera_core::client::runtime_build_port::current_profile_closure(profiles)
    }

    fn evaluate_affects(rule: &AffectsRule, before: &Profiles, after: &Profiles) -> bool {
        match rule {
            AffectsRule::Never => false,
            AffectsRule::Always => true,
            AffectsRule::CurrentChanged => before.current != after.current,
            AffectsRule::GlobalChanged => before.global_transforms != after.global_transforms,
            AffectsRule::Touched(uid) => {
                let closure_before = Self::current_closure(before);
                let closure_after = Self::current_closure(after);
                closure_before != closure_after
                    || closure_before.contains(uid)
                    || closure_after.contains(uid)
            }
        }
    }

    fn prepare_candidate(mut next: Profiles) -> Result<Profiles, ProfilesError> {
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        next.bump_revision()?;
        Ok(next)
    }

    /// Pure document edits follow ref's direct versioned source commit,
    /// without invoking the old Profile runtime participant. Every source
    /// mutation still must pass the actual closure comparison; an AffectsRule
    /// alone is not proof that a current dependency is unchanged.
    fn source_only_document_change(
        before: &Profiles,
        next: &Profiles,
        hints: &MutationHints,
        class: CommandClass,
    ) -> bool {
        class == CommandClass::Save
            && !hints.names_runtime_field()
            && hints.touched.is_empty()
            && hints.staged_content.is_empty()
            && impact::classify_profiles(before, next, hints) == RuntimeImpact::None
    }

    /// Classify only concrete failures returned by the conditional source
    /// owner. CAS conflicts are handled by the separate version-conflict path.
    fn commit_failure_reason(
        source: &chimera_core::state::ReplaceIfVersionError,
    ) -> Option<CommitFailureReason> {
        use chimera_core::state::{ReplaceIfVersionError, error::StateChangedError};
        match source {
            ReplaceIfVersionError::WriteConfig(_) => Some(CommitFailureReason::WriteConfig),
            ReplaceIfVersionError::LocalWrite(_) => Some(CommitFailureReason::LocalWrite),
            ReplaceIfVersionError::ResourceRecovery { .. } => {
                Some(CommitFailureReason::RecoverAfterWriteFailure)
            }
            ReplaceIfVersionError::Recovery { .. } => Some(CommitFailureReason::StateRecovery),
            ReplaceIfVersionError::State(StateChangedError::PrepareAck(_)) => {
                Some(CommitFailureReason::PrepareAck)
            }
            ReplaceIfVersionError::State(StateChangedError::Validation(_)) => {
                Some(CommitFailureReason::StateValidation)
            }
            ReplaceIfVersionError::State(StateChangedError::StateCasMismatch { .. }) => None,
        }
    }

    /// Map exactly one independent candidate check to the ref pre-commit
    /// transaction outcome. Only an explicit Rejected verdict may produce
    /// RuntimeRefused; every unavailable, skipped, untyped or unchecked
    /// result becomes RuntimeFailed. Neither can commit or replay a write.
    fn runtime_gate_failure(error: Arc<RuntimeError>) -> ProfilesError {
        let unavailable = matches!(error.as_ref(), RuntimeError::CheckUnavailable { .. });
        let source: chimera_contracts::BoxedCause = Box::new(error.clone());
        let errors = vec![error];
        let commit = if unavailable {
            CommitAborted::RuntimeFailed { errors, source }
        } else {
            CommitAborted::RuntimeRefused { errors, source }
        };
        ProfilesError::Commit { source: commit }
    }

    /// Persist an already validated, revision-bumped candidate exactly as prepared.
    async fn persist_candidate(
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: &Profiles,
        next: Profiles,
        hints: MutationHints,
        class: CommandClass,
    ) -> Result<
        (
            Arc<Profiles>,
            (
                crate::client::runtime::CommitReceipt,
                Vec<crate::client::runtime::Degradation>,
            ),
        ),
        ProfilesError,
    > {
        state
            .mutations
            .ensure_ready()
            .map_err(|error| ProfilesError::Persist(error.to_string()))?;
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "state_write_ready", "Profile source transaction stage");
        let reaches_runtime = !Self::source_only_document_change(before, &next, &hints, class);
        if reaches_runtime {
            state
                .mutations
                .runtime_gate()
                .map_err(Self::runtime_gate_failure)?
                .check(crate::state::mutation::RuntimeCandidate {
                    change: crate::state::mutation::DomainChange::Profiles {
                        candidate: Arc::new(next.clone()),
                    },
                    staged: hints.staged_content.clone(),
                })
                .await
                .map_err(Self::runtime_gate_failure)?;
            #[cfg(feature = "e2e")]
            tracing::warn!(target: "chimera::e2e_profile", stage = "state_write_gate_passed", "Profile source transaction stage");
        }
        // Use the same publication fence as ref's Profile source actor.
        // A concurrent committed-content reader cannot observe a mixed
        // Profiles document and managed-file generation during the write.
        let fence = state.content_lock.write().await;
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "state_write_publish_start", "Profile source transaction stage");
        let result = state
            .manager
            .replace_if_version(expected_version, next.clone())
            .await;
        drop(fence);
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "state_write_publish_finished", "Profile source transaction stage");
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {
                let version = *state.manager.snapshot_handle().load().version.as_ref();
                let receipt = state.mutations.source_committed(
                    version,
                    reaches_runtime,
                    reaches_runtime.then(OperationId::generate),
                    Self::changed_files(before, &next, None),
                );
                Ok((Arc::new(next), (receipt, Vec::new())))
            }
            Ok(ReplaceIfVersionResult::Conflict { actual_version }) => {
                Err(ProfilesError::VersionConflict {
                    expected: *expected_version.as_ref(),
                    actual: *actual_version.as_ref(),
                })
            }
            Err(chimera_core::state::ReplaceIfVersionError::State(
                chimera_core::state::error::StateChangedError::StateCasMismatch {
                    expected,
                    actual,
                },
            )) => Err(ProfilesError::VersionConflict {
                expected: *expected.as_ref(),
                actual: *actual.as_ref(),
            }),
            Err(source) => Err(ProfilesError::SourceTransaction {
                reason: Self::commit_failure_reason(&source),
                source,
                cleanup: None,
                cleanup_failures: Vec::new(),
            }),
        }
    }

    /// File ids whose source content disappeared or was explicitly touched.
    /// Metadata-only edits do not produce spurious file-change events.
    fn changed_files(
        before: &Profiles,
        next: &Profiles,
        touched: Option<&ProfileId>,
    ) -> Vec<ProfileId> {
        let mut files: Vec<_> = before
            .items
            .keys()
            .filter(|id| !next.items.contains_key(*id))
            .cloned()
            .collect();
        if let Some(uid) = touched
            && !files.contains(uid)
        {
            files.push(uid.clone());
        }
        files
    }

    async fn reconcile_committed(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        snapshot: &Profiles,
    ) {
        state.index = ProfileDependencyIndex::build(snapshot);
        state.jobs_revision = state.jobs_revision.saturating_add(1);
        if let Err(error) = state
            .jobs
            .reconcile(
                snapshot,
                myself,
                state.jobs_revision,
                state.gate == ProducerGate::Running,
            )
            .await
        {
            tracing::error!(%error, "failed to reconcile profile sync jobs");
        }
        if state.gate == ProducerGate::Running {
            state.external_watchers.reconcile(snapshot, myself);
        }
    }

    async fn start_producers(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
    ) {
        if state.gate != ProducerGate::Held {
            return;
        }

        state.gate = ProducerGate::Running;
        let snapshot = Self::current_state(state);
        Self::reconcile_committed(myself, state, &snapshot).await;
        state.jobs.catch_up(&snapshot).await;

        let actor = myself.clone();
        state.reconcile_task = Some(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(MATERIALIZATION_RECONCILE_INTERVAL);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Startup already reconciled in pre_start; skip the immediate first tick.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                if actor
                    .cast(ProfilesActorMessage::ReconcileMaterializations)
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    async fn run_state_write<F>(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        mutate: F,
    ) -> Result<CommitReport, ProfilesError>
    where
        F: FnOnce(&mut Profiles) -> Result<AffectsRule, ProfilesError>,
    {
        let versioned = state.manager.snapshot_handle().load();
        let expected_version = versioned.version;
        let before = versioned.state.clone();
        drop(versioned);
        let mut next = before.clone();
        let affects = mutate(&mut next)?;
        let candidate = Self::prepare_candidate(next)?;
        let (hints, class) = Self::mutation_hints(&affects, &candidate);
        let (snapshot, (receipt, runtime_degradations)) =
            Self::persist_candidate(state, expected_version, &before, candidate, hints, class)
                .await?;
        Self::reconcile_committed(myself, state, &snapshot).await;
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "state_write_reconcile_finished", "Profile source transaction stage");
        Ok(CommitReport {
            affects_current: Self::evaluate_affects(&affects, &before, &snapshot),
            snapshot,
            degradations: Vec::new(),
            receipt,
            runtime_degradations,
            created: None,
        })
    }

    async fn materialization_call<T, F>(
        state: &ProfilesActorState,
        operation: F,
    ) -> anyhow::Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&dyn ProfileMaterializationPort) -> anyhow::Result<T> + Send + 'static,
    {
        let materialization = Arc::clone(&state.materialization);
        tokio::task::spawn_blocking(move || operation(materialization.as_ref()))
            .await
            .map_err(|error| anyhow::Error::new(error).context("materialization task failed"))?
    }

    fn materialization_error(
        context: &'static str,
        error: impl Into<anyhow::Error>,
    ) -> ProfilesError {
        match error.into().downcast::<ProfileFileError>() {
            Ok(source) => ProfilesError::MaterializationFile {
                context,
                operation: match context {
                    "prepare materialization" => Some(MaterializationOperation::PrepareFileFirst),
                    "discard staged resource" => Some(MaterializationOperation::Compensate),
                    "prepare cleanup" => Some(MaterializationOperation::PrepareCleanup),
                    _ => None,
                },
                source,
            },
            Err(error) => ProfilesError::Materialization(format!("{context}: {error}")),
        }
    }

    async fn resource_for_definition(
        state: &ProfilesActorState,
        definition: &ProfileDefinition,
        initial_file: Option<String>,
    ) -> Result<Option<MaterializationResource>, ProfilesError> {
        let Some(source) = definition.source() else {
            return Ok(None);
        };
        match source {
            ProfileSource::Local {
                binding: LocalBinding::Managed { .. },
            } => Ok(Some(MaterializationResource::File {
                content: initial_file.unwrap_or_default(),
            })),
            ProfileSource::Local {
                binding:
                    LocalBinding::External {
                        target,
                        mode: ExternalMode::Symlink,
                        ..
                    },
            } => Ok(Some(MaterializationResource::Symlink {
                target: target.clone(),
            })),
            ProfileSource::Local {
                binding:
                    LocalBinding::External {
                        target,
                        mode: ExternalMode::Mirror,
                        ..
                    },
            } => {
                let fs = Arc::clone(&state.fs);
                let target = target.clone();
                let content = tokio::task::spawn_blocking(move || fs.read_external(&target))
                    .await
                    .map_err(|error| {
                        Self::materialization_error("mirror source read task failed", error)
                    })?
                    .map_err(|error| {
                        Self::materialization_error("failed to read mirror source", error)
                    })?;
                Self::validate_fetched_content(definition, &content).map_err(|error| {
                    ProfilesError::Materialization(format!(
                        "mirror source validation failed: {error}"
                    ))
                })?;
                Ok(Some(MaterializationResource::File { content }))
            }
            ProfileSource::Remote { .. } => Ok(Some(MaterializationResource::File {
                // Direct Add of a remote source still stages an empty file.
                // Remote *import* never uses this path: it fetch-before-commits
                // real bytes through ImportRemote / CommitImported.
                content: String::new(),
            })),
        }
    }

    fn remote_import_definition(
        url: url::Url,
        option: RemoteProfileOptions,
        file: ManagedProfilePath,
        subscription: SubscriptionInfo,
        updated_at: Option<time::OffsetDateTime>,
    ) -> ProfileDefinition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Remote {
                    materialized: MaterializedFile { file, updated_at },
                    url,
                    option,
                    subscription,
                },
                transforms: vec![],
            }),
        }
    }

    /// Validate the URL, options and destination folder before starting any
    /// remote download. The commit stage checks the folder again against the
    /// latest document, so concurrent deletion cannot persist an orphan.
    fn validate_import_request(
        before: &Profiles,
        metadata: &ProfileMetadata,
        url: url::Url,
        option: RemoteProfileOptions,
        folder: Option<&FolderId>,
    ) -> Result<(), ProfilesError> {
        let definition = Self::remote_import_definition(
            url,
            option,
            ManagedProfilePath::new("pending.yaml").expect("static managed path is valid"),
            SubscriptionInfo::default(),
            None,
        );
        let uid = Self::generate_uid(&definition, before);
        let mut next = before.clone();
        let mut definition = definition;
        let ext = Self::canonical_extension(&definition);
        if let Some(source) = definition.source_mut() {
            source.materialized_mut().file = ManagedProfilePath::new(format!("{uid}.{ext}"))
                .expect("uid-derived path is always a valid managed path");
        }
        if !next.append_item(ProfileItem {
            uid: uid.clone(),
            metadata: metadata.clone(),
            definition,
        }) {
            return Err(ProfilesError::Persist("uid collision".into()));
        }
        Self::join_folder(&mut next, &uid, folder)?;
        next.validate().map_err(ProfilesError::ValidationFailed)?;
        Ok(())
    }

    async fn finish_cleanup(
        state: &ProfilesActorState,
        cleanup: PreparedCleanup,
        snapshot: &Profiles,
    ) -> Vec<ProfileDegradation> {
        let _guard = state.content_lock.write().await;
        let retry = cleanup.clone();
        if let Err(error) =
            Self::materialization_call(state, move |port| port.activate_cleanup(&cleanup)).await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup activation deferred: {error}"),
            }];
        }
        let profiles = snapshot.clone();
        if let Err(error) =
            Self::materialization_call(state, move |port| port.retry_cleanup(&retry, &profiles))
                .await
        {
            return vec![ProfileDegradation {
                phase: ProfileDegradationPhase::Cleanup,
                code: ProfileDegradationCode::CleanupDeferred,
                message: format!("profile cleanup retry deferred: {error}"),
            }];
        }
        Vec::new()
    }

    fn mutation_hints(
        affects: &AffectsRule,
        candidate: &Profiles,
    ) -> (MutationHints, CommandClass) {
        let mut hints = MutationHints::default();
        if let AffectsRule::Touched(uid) = affects {
            if let Some(source) = candidate
                .items
                .get(uid)
                .and_then(|item| item.definition.source())
            {
                hints.touched.push(TouchedContent {
                    path: source.materialized().file.clone(),
                    content_digest: None,
                    resource: None,
                });
            }
        }
        let class = if matches!(affects, AffectsRule::CurrentChanged) {
            hints.activation = candidate
                .current
                .clone()
                .map(ActivationIntent::Activate)
                .unwrap_or(ActivationIntent::Deactivate);
            CommandClass::ExplicitSwitch
        } else {
            if matches!(affects, AffectsRule::Always | AffectsRule::GlobalChanged) {
                hints.requested = RequestedRuntimeFields::runtime();
            }
            CommandClass::Save
        };
        (hints, class)
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_with_resources(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: Profiles,
        next: Profiles,
        affects: AffectsRule,
        resource: Option<(ManagedProfilePath, MaterializationResource)>,
        cleanup_path: Option<ManagedProfilePath>,
        created: Option<ProfileId>,
    ) -> Result<CommitReport, ProfilesError> {
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_start", "Profile source transaction stage");
        let candidate = Self::prepare_candidate(next)?;
        let expected_revision = candidate.revision();
        state
            .mutations
            .ensure_ready()
            .map_err(|error| ProfilesError::Persist(error.to_string()))?;
        let operation = OperationId::generate();
        let (mut hints, class) = Self::mutation_hints(&affects, &candidate);
        if let Some((path, resource)) = &resource {
            let content = match resource {
                MaterializationResource::File { content } => Some(content.clone()),
                MaterializationResource::Symlink { .. }
                    if !Self::current_closure(&candidate).iter().any(|uid| {
                        candidate
                            .items
                            .get(uid)
                            .and_then(|item| item.definition.source())
                            .is_some_and(|source| source.materialized().file == *path)
                    }) =>
                {
                    None
                }
                MaterializationResource::Symlink { target } => {
                    let fs = state.fs.clone();
                    let target = target.clone();
                    Some(
                        tokio::task::spawn_blocking(move || fs.read_external(&target))
                            .await
                            .map_err(|error| {
                                Self::materialization_error("source read task", error)
                            })?
                            .map_err(|error| Self::materialization_error("source read", error))?,
                    )
                }
            };
            hints.touched.push(TouchedContent {
                path: (*path).clone(),
                content_digest: content.as_ref().map(|content| {
                    ContentDigest::new(crate::state::profiles::payload_digest(content.as_bytes()))
                }),
                resource: Some(StagedResourceToken::new(operation.to_string())),
            });
            if let Some(content) = content {
                hints.staged_content.insert(path.to_string(), content);
            }
        }
        // Complete the full candidate check before preparing a file/journal.
        let source_only = class == CommandClass::Save
            && !hints.names_runtime_field()
            && impact::classify_profiles(&before, &candidate, &hints) == RuntimeImpact::None;
        if !source_only {
            state
                .mutations
                .runtime_gate()
                .map_err(Self::runtime_gate_failure)?
                .check(crate::state::mutation::RuntimeCandidate {
                    change: crate::state::mutation::DomainChange::Profiles {
                        candidate: Arc::new(candidate.clone()),
                    },
                    staged: hints.staged_content.clone(),
                })
                .await
                .map_err(Self::runtime_gate_failure)?;
        }
        let prepared = if let Some((path, resource)) = resource {
            Some(
                Self::materialization_call(state, move |port| {
                    port.prepare_file_first(&path, resource, expected_revision)
                })
                .await
                .map_err(|error| Self::materialization_error("prepare materialization", error))?,
            )
        } else {
            None
        };
        let cleanup = if let Some(path) = cleanup_path {
            match Self::materialization_call(state, move |port| {
                port.prepare_cleanup(&path, expected_revision)
            })
            .await
            {
                Ok(cleanup) => Some(cleanup),
                Err(error) => {
                    if let Some(prepared) = prepared {
                        Self::materialization_call(state, move |port| port.compensate(&prepared))
                            .await
                            .map_err(|error| {
                                Self::materialization_error("discard staged resource", error)
                            })?;
                    }
                    return Err(Self::materialization_error("prepare cleanup", error));
                }
            }
        } else {
            None
        };
        // Ref Source Commit owns materialization, document publication and
        // compensation. Runtime application is requested only *after* the
        // durable version is visible, never as a transaction participant.
        let write_lock = state.content_lock.clone();
        // The source-owned local-write commits own file promotion, document
        // publication and recovery under one fence. Keep
        // the guard in the detached source owner if the waiting actor is dropped.
        // Pre-write conflicts never promote any source file.
        let fence = Arc::new(std::sync::Mutex::new(None));
        let write_fence = Arc::clone(&fence);
        // Retain the fence in the detached persistence owner even when the
        // caller is cancelled. A dropped waiter must not free the fence while
        // a file promotion or source publication is still in flight.
        let recovery_fence = Arc::clone(&fence);
        let write_port = state.materialization.clone();
        let write_resource = prepared.clone();
        let recover_port = state.materialization.clone();
        let recover_resource = prepared.clone();
        let recover_cleanup = cleanup.clone();
        let write = move || async move {
            let guard = write_lock.write().await;
            *write_fence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(guard);
            tokio::task::spawn_blocking(move || {
                if let Some(prepared) = write_resource {
                    write_port.promote(&prepared)?;
                }
                anyhow::Ok(())
            })
            .await?
        };
        let recover = move || async move {
            let _owner_fence = recovery_fence;
            tokio::task::spawn_blocking(move || {
                let resource = recover_resource
                    .as_ref()
                    .map(|p| recover_port.compensate(p))
                    .transpose();
                let cleanup = recover_cleanup
                    .as_ref()
                    .map(|c| recover_port.cancel_cleanup(c))
                    .transpose();
                match (resource, cleanup) {
                    (Ok(_), Ok(_)) => Ok(()),
                    (resource, cleanup) => {
                        Err(anyhow::Error::new(MaterializationRecoveryFailure {
                            resource: resource.err(),
                            cleanup: cleanup.err(),
                        }))
                    }
                }
            })
            .await?
        };
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_publish_start", "Profile source transaction stage");
        let result = state
            .manager
            .replace_if_version_with_local_write(
                expected_version,
                candidate.clone(),
                write,
                recover,
            )
            .await;
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_publish_finished", "Profile source transaction stage");
        // Publication or compensation settled in the source owner. Do not
        // hold the file fence while reconciling source jobs or runtime.
        drop(
            fence
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take(),
        );
        match result {
            Ok(ReplaceIfVersionResult::Replaced) => {}
            result => {
                // Only a pre-persistence refusal leaves staging to discard here.
                // Once local_write starts, the source task exclusively owns
                // compensation and publishes its result before releasing admission.
                let before_write = matches!(
                    &result,
                    Ok(ReplaceIfVersionResult::Conflict { .. })
                        | Err(chimera_core::state::ReplaceIfVersionError::State(
                            chimera_core::state::error::StateChangedError::PrepareAck(_)
                        ))
                );
                let mut failures = Vec::new();
                // A refused preflight never entered the manager's local write
                // step. Compensation can still touch staged resource journals,
                // so fence it against a simultaneous committed-content read.
                let _cleanup_guard = if before_write {
                    Some(state.content_lock.write().await)
                } else {
                    None
                };
                if before_write {
                    if let Some(prepared) = prepared {
                        if let Err(error) = Self::materialization_call(state, move |port| {
                            port.compensate(&prepared)
                        })
                        .await
                        {
                            failures.push(error);
                        }
                    }
                    if let Some(cleanup) = cleanup {
                        if let Err(error) = Self::materialization_call(state, move |port| {
                            port.cancel_cleanup(&cleanup)
                        })
                        .await
                        {
                            failures.push(error);
                        }
                    }
                }
                // Keep old human-readable cleanup output for the legacy UI,
                // alongside the exact independent port/journal error objects.
                let cleanup_message = (!failures.is_empty()).then(|| {
                    failures
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                });
                return Err(match result {
                    Err(chimera_core::state::ReplaceIfVersionError::State(
                        chimera_core::state::error::StateChangedError::StateCasMismatch {
                            expected,
                            actual,
                        },
                    )) => {
                        if failures.is_empty() {
                            ProfilesError::VersionConflict {
                                expected: *expected.as_ref(),
                                actual: *actual.as_ref(),
                            }
                        } else {
                            ProfilesError::ConflictCleanup {
                                expected: *expected.as_ref(),
                                actual: *actual.as_ref(),
                                cleanup: cleanup_message.clone().unwrap_or_default(),
                                cleanup_failures: failures,
                            }
                        }
                    }
                    Err(source) => ProfilesError::SourceTransaction {
                        reason: Self::commit_failure_reason(&source),
                        source,
                        cleanup: cleanup_message,
                        cleanup_failures: failures,
                    },
                    Ok(ReplaceIfVersionResult::Conflict { actual_version }) => {
                        if failures.is_empty() {
                            ProfilesError::VersionConflict {
                                expected: *expected_version.as_ref(),
                                actual: *actual_version.as_ref(),
                            }
                        } else {
                            ProfilesError::ConflictCleanup {
                                expected: *expected_version.as_ref(),
                                actual: *actual_version.as_ref(),
                                cleanup: cleanup_message.clone().unwrap_or_default(),
                                cleanup_failures: failures,
                            }
                        }
                    }
                    Ok(ReplaceIfVersionResult::Replaced) => unreachable!(),
                });
            }
        }
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_source_published", "Profile source transaction stage");
        let version = *state.manager.snapshot_handle().load().version.as_ref();
        let touched = match &affects {
            AffectsRule::Touched(uid) => Some(uid),
            _ => created.as_ref(),
        };
        let receipt = state.mutations.source_committed(
            version,
            !source_only,
            (!source_only).then_some(operation),
            Self::changed_files(&before, &candidate, touched),
        );
        let runtime_degradations = Vec::new();
        let snapshot = Arc::new(candidate);
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_reconcile_start", "Profile source transaction stage");
        Self::reconcile_committed(myself, state, &snapshot).await;
        #[cfg(feature = "e2e")]
        tracing::warn!(target: "chimera::e2e_profile", stage = "file_commit_reconcile_finished", "Profile source transaction stage");
        let mut degradations = Vec::new();
        if let Some(prepared) = prepared {
            if let Err(error) =
                Self::materialization_call(state, move |port| port.complete(&prepared)).await
            {
                degradations.push(ProfileDegradation {
                    phase: ProfileDegradationPhase::Reconcile,
                    code: ProfileDegradationCode::MaterializationDeferred,
                    message: format!("materialization completion deferred: {error}"),
                });
            }
        }
        if let Some(cleanup) = cleanup {
            degradations.extend(Self::finish_cleanup(state, cleanup, &snapshot).await);
        }
        Ok(CommitReport {
            affects_current: Self::evaluate_affects(&affects, &before, &snapshot),
            snapshot,
            degradations,
            receipt,
            runtime_degradations,
            created,
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn commit_file_first(
        myself: &ActorRef<ProfilesActorMessage>,
        state: &mut ProfilesActorState,
        expected_version: Version,
        before: Profiles,
        next: Profiles,
        affects: AffectsRule,
        path: ManagedProfilePath,
        content: String,
    ) -> Result<CommitReport, ProfilesError> {
        Self::commit_with_resources(
            myself,
            state,
            expected_version,
            before,
            next,
            affects,
            Some((path, MaterializationResource::File { content })),
            None,
            None,
        )
        .await
    }

    async fn reconcile_materializations(
        state: &ProfilesActorState,
    ) -> anyhow::Result<MaterializationReconcileReport> {
        let _guard = state.content_lock.write().await;
        let snapshot = Self::current_state(state);
        let materialization = Arc::clone(&state.materialization);
        tokio::task::spawn_blocking(move || materialization.reconcile(&snapshot))
            .await
            .map_err(|error| anyhow::anyhow!("materialization reconcile join failed: {error}"))?
    }

    fn log_reconcile_report(report: &MaterializationReconcileReport) {
        for degradation in &report.degradations {
            tracing::warn!(
                phase = ?degradation.phase,
                code = ?degradation.code,
                retryable = degradation.code.retryable(),
                message = %degradation.message,
                "profile materialization reconcile degradation"
            );
        }
        if report.discarded
            + report.promoted
            + report.completed
            + report.compensated
            + report.cleanups_completed
            + report.cleanups_fenced
            > 0
        {
            tracing::info!(
                discarded = report.discarded,
                promoted = report.promoted,
                completed = report.completed,
                compensated = report.compensated,
                cleanups_completed = report.cleanups_completed,
                cleanups_fenced = report.cleanups_fenced,
                "profile materialization reconcile advanced journals"
            );
        }
    }

    fn generate_uid(definition: &ProfileDefinition, existing: &Profiles) -> ProfileId {
        let prefix = match definition {
            ProfileDefinition::Config { .. } => 'c',
            ProfileDefinition::Transform { .. } => 't',
        };
        loop {
            let candidate = ProfileId(format!("{prefix}{}", nanoid::nanoid!(11)));
            if existing.items.get(&candidate).is_none() {
                return candidate;
            }
        }
    }

    fn generate_folder_id(existing: &Profiles) -> FolderId {
        loop {
            let candidate = FolderId(format!("f{}", nanoid::nanoid!(11)));
            if existing.folders.iter().all(|folder| folder.id != candidate) {
                return candidate;
            }
        }
    }

    fn generate_proxy_group_folder_id(layout: &ProxyGroupLayout) -> FolderId {
        loop {
            let candidate = FolderId(format!("f{}", nanoid::nanoid!(11)));
            if layout.folders.iter().all(|folder| folder.id != candidate) {
                return candidate;
            }
        }
    }

    /// Attach a new Config to a folder before the source transaction commits.
    /// Missing folders reject the entire add/import; never persist an orphan.
    fn join_folder(
        profiles: &mut Profiles,
        uid: &ProfileId,
        folder: Option<&FolderId>,
    ) -> Result<(), ProfilesError> {
        let Some(id) = folder else {
            return Ok(());
        };
        let Some(target) = profiles.folders.iter_mut().find(|item| &item.id == id) else {
            return Err(ProfilesError::ValidationFailed(vec![
                ProfileValidationError::FolderNotFound { id: id.clone() },
            ]));
        };
        target.members.push(uid.clone());
        Ok(())
    }

    fn canonical_extension(definition: &ProfileDefinition) -> &'static str {
        match definition {
            ProfileDefinition::Config { .. } => "yaml",
            ProfileDefinition::Transform { transform } => match transform {
                TransformDefinition::Overlay(_) => "yaml",
                TransformDefinition::Script(script) => match script.runtime {
                    ScriptRuntime::JavaScript => "js",
                    ScriptRuntime::Lua => "lua",
                },
            },
        }
    }

    /// Keep the original YAML parser failure, not a guessed content error.
    /// Both import and scheduled refresh use exactly this validator before
    /// any materialized file or source transaction can be written.
    fn fetch_failure(
        message: String,
        source: Option<RefreshFailureSource>,
        importing: bool,
    ) -> ProfilesError {
        match source {
            Some(RefreshFailureSource::Subscription { url, source }) => {
                ProfilesError::FetchSubscription { url, source }
            }
            Some(RefreshFailureSource::Content(source)) => {
                ProfilesError::ProfileContentRejected { source }
            }
            None if importing => ProfilesError::ImportFailed { message },
            None => ProfilesError::RefreshFailed { message },
        }
    }

    fn validate_fetched_content_typed(
        definition: &ProfileDefinition,
        content: &str,
    ) -> Result<(), ProfileContentError> {
        let needs_yaml = match definition {
            ProfileDefinition::Config { .. } => true,
            ProfileDefinition::Transform { transform } => {
                matches!(transform, TransformDefinition::Overlay(_))
            }
        };
        if needs_yaml {
            let mapping = serde_yaml::from_str::<serde_yaml::Mapping>(content)
                .map_err(|source| ProfileContentError::NotYamlMapping { source })?;
            if matches!(definition, ProfileDefinition::Config { .. })
                && !mapping.contains_key("proxies")
                && !mapping.contains_key("proxy-providers")
            {
                return Err(ProfileContentError::MissingProxies);
            }
            Ok(())
        } else if content.trim().is_empty() {
            Err(ProfileContentError::EmptyScript)
        } else {
            Ok(())
        }
    }

    /// Legacy call sites still need the exact historical text for sync jobs
    /// and old GUI consumers. Typed callers retain the actual source instead.
    fn content_error_message(error: &ProfileContentError) -> String {
        match error {
            ProfileContentError::NotYamlMapping { source } => {
                format!("downloaded content is not a YAML mapping: {source}")
            }
            ProfileContentError::ReserializeYaml { source } => {
                format!("could not reserialize the YAML mapping: {source}")
            }
            ProfileContentError::MissingProxies => {
                "subscription does not contain `proxies` or `proxy-providers`".into()
            }
            ProfileContentError::EmptyScript => "downloaded script is empty".into(),
        }
    }

    fn validate_fetched_content(
        definition: &ProfileDefinition,
        content: &str,
    ) -> Result<(), String> {
        Self::validate_fetched_content_typed(definition, content)
            .map_err(|error| Self::content_error_message(&error))
    }

    /// design §17 five reference categories. Item-level referrers plus the two
    /// document-level flags (current / global_transforms) so the IPC layer can
    /// render an unambiguous message even when the referrer list is empty.
    fn referrers_of(
        state: &ProfilesActorState,
        profiles: &Profiles,
        uid: &ProfileId,
    ) -> Option<(Vec<ProfileId>, bool, bool)> {
        let mut referrers: indexmap::IndexSet<ProfileId> = Default::default();
        if let Some(set) = state.index.composition_base_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.extend_proxies_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }
        if let Some(set) = state.index.transform_dependents.get(uid) {
            referrers.extend(set.iter().cloned());
        }

        let current = profiles.current.as_ref() == Some(uid);
        let global_transforms = state.index.global_transform_ids.contains(uid);
        if referrers.is_empty() && !current && !global_transforms {
            None
        } else {
            Some((referrers.into_iter().collect(), current, global_transforms))
        }
    }

    fn retains_materialization(
        previous: &ProfileSource,
        next: &ProfileSource,
        canonical: &ManagedProfilePath,
    ) -> bool {
        previous.materialized().file == *canonical
            && match (previous, next) {
                (
                    ProfileSource::Local {
                        binding: LocalBinding::Managed { .. },
                    },
                    ProfileSource::Local {
                        binding: LocalBinding::Managed { .. },
                    },
                ) => true,
                (
                    ProfileSource::Local {
                        binding:
                            LocalBinding::External {
                                target: previous_target,
                                mode: previous_mode,
                                ..
                            },
                    },
                    ProfileSource::Local {
                        binding:
                            LocalBinding::External {
                                target: next_target,
                                mode: next_mode,
                                ..
                            },
                    },
                ) => previous_target == next_target && previous_mode == next_mode,
                (
                    ProfileSource::Remote {
                        url: previous_url, ..
                    },
                    ProfileSource::Remote { url: next_url, .. },
                ) => previous_url == next_url,
                _ => false,
            }
    }
}

impl Actor for ProfilesActor {
    type Msg = ProfilesActorMessage;
    type State = ProfilesActorState;
    type Arguments = ProfilesActorArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        // Startup recovery must finish before jobs/watchers/mutations are
        // armed. Blocking port work stays off the async runtime via spawn_blocking.
        let loaded = args.manager.snapshot_handle().load().state.clone();
        let materialization = Arc::clone(&args.materialization);
        let _guard = args.content_lock.write().await;
        let report = tokio::task::spawn_blocking(move || materialization.reconcile(&loaded))
            .await
            .map_err(|error| {
                ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile join failed: {error}"
                ))
            })?
            .map_err(|error| {
                ActorProcessingErr::from(anyhow::anyhow!(
                    "startup materialization reconcile failed: {error}"
                ))
            })?;
        Self::log_reconcile_report(&report);
        drop(_guard);

        args.jobs
            .reconcile(
                &args.manager.snapshot_handle().load().state,
                &myself,
                1,
                false,
            )
            .await?;

        let index = ProfileDependencyIndex::build(&args.manager.snapshot_handle().load().state);
        Ok(ProfilesActorState {
            mutations: args.mutations,
            manager: args.manager,
            content_lock: args.content_lock,
            index,
            fs: args.fs,
            fetcher: args.fetcher,
            materialization: args.materialization,
            pending_refresh: HashMap::new(),
            next_refresh_token: 1,
            pending_imports: HashMap::new(),
            next_import_token: 1,
            gate: ProducerGate::Held,
            jobs: args.jobs,
            jobs_revision: 1,
            external_watchers: ExternalWatchers::default(),
            reconcile_task: None,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let message = match message {
            ProfilesActorMessage::SyncRemote {
                uid,
                patch,
                context,
                reply,
            } => {
                let origin = if context.0.trigger == chimera_jobs::Trigger::Manual {
                    RefreshOrigin::Manual
                } else {
                    RefreshOrigin::Scheduled
                };
                ProfilesActorMessage::RefreshRemote {
                    uid,
                    patch,
                    origin,
                    context: Some(context),
                    reply: Some(reply),
                }
            }
            message => message,
        };
        match message {
            ProfilesActorMessage::SyncRemote { .. } => unreachable!(),
            ProfilesActorMessage::SaveFile {
                uid,
                content,
                reply,
            } => {
                let result = async {
                    let versioned = state.manager.snapshot_handle().load();
                    let before = versioned.state.clone();
                    let item = before
                        .items
                        .get(&uid)
                        .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
                    let source = item
                        .definition
                        .source()
                        .ok_or(ProfilesError::ProfileHasNoFile)?;
                    let path = match source {
                        ProfileSource::Local {
                            binding: LocalBinding::Managed { materialized },
                        } => materialized.file.clone(),
                        _ => {
                            return Err(ProfilesError::FileNotWritable {
                                reason: "only managed local profiles are writable".into(),
                            });
                        }
                    };
                    Self::commit_file_first(
                        &myself,
                        state,
                        versioned.version,
                        before.clone(),
                        before,
                        AffectsRule::Touched(uid),
                        path,
                        content,
                    )
                    .await
                }
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrent { current, reply } => {
                let result = Self::run_state_write(&myself, state, |profiles| {
                    profiles.set_current(current);
                    Ok(AffectsRule::CurrentChanged)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetCurrentIfNone { uid, reply } => {
                #[cfg(feature = "e2e")]
                tracing::warn!(target: "chimera::e2e_profile", stage = "auto_activate_received", "Profile source transaction stage");
                // Atomic conditional activation: select `uid` only when nothing
                // is currently selected. Serialized actor message handling makes
                // this read-then-write race-free without a second RPC, so a
                // concurrent SetCurrent cannot be silently overwritten.
                if Self::current_state(state).current.is_some() {
                    let _ = reply.send(Ok(None));
                } else {
                    let result = Self::run_state_write(&myself, state, |profiles| {
                        profiles.set_current(Some(uid));
                        Ok(AffectsRule::CurrentChanged)
                    })
                    .await;
                    #[cfg(feature = "e2e")]
                    tracing::warn!(target: "chimera::e2e_profile", stage = "auto_activate_replied", succeeded = result.is_ok(), "Profile source transaction stage");
                    let _ = reply.send(result.map(Some));
                }
            }
            ProfilesActorMessage::SetGlobalTransforms { ids, reply } => {
                let result = Self::run_state_write(&myself, state, |profiles| {
                    profiles.global_transforms = ids;
                    Ok(AffectsRule::GlobalChanged)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetValidFields { fields, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    profiles.valid = fields;
                    // Whitelist changes reshape runtime extraction for the active
                    // config, so a rebuild is always required.
                    Ok(AffectsRule::Always)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Replace {
                profiles: next,
                reply,
            } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    // Client-supplied documents must not reset the server-owned
                    // materialization recovery generation; state writes bump once.
                    profiles.current = next.current;
                    profiles.global_transforms = next.global_transforms;
                    profiles.valid = next.valid;
                    profiles.folders = next.folders;
                    profiles.proxy_group_layouts = next.proxy_group_layouts;
                    profiles.items = next.items;
                    Ok(AffectsRule::Always)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Add {
                request,
                initial_file,
                reply,
            } => {
                #[cfg(feature = "e2e")]
                tracing::warn!(target: "chimera::e2e_profile", stage = "add_received", "Profile source transaction stage");
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let uid = Self::generate_uid(&request.definition, &before);
                let ext = Self::canonical_extension(&request.definition);
                let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                    .expect("uid-derived path is always a valid managed path");
                let folder = request.folder;
                let mut definition = request.definition;
                if let Some(source) = definition.source_mut() {
                    let materialized = source.materialized_mut();
                    materialized.file = canonical.clone();
                    materialized.updated_at = None;
                    if let ProfileSource::Remote { subscription, .. } = source {
                        *subscription = SubscriptionInfo::default();
                    }
                }
                let result =
                    match Self::resource_for_definition(state, &definition, initial_file).await {
                        Ok(resource) => {
                            let item = ProfileItem {
                                uid: uid.clone(),
                                metadata: request.metadata,
                                definition,
                            };
                            let mut next = before.clone();
                            if !next.append_item(item) {
                                Err(ProfilesError::Persist("uid collision".into()))
                            } else if let Err(error) =
                                Self::join_folder(&mut next, &uid, folder.as_ref())
                            {
                                Err(error)
                            } else {
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Never,
                                    resource.map(|resource| (canonical, resource)),
                                    None,
                                    Some(uid),
                                )
                                .await
                            }
                        }
                        Err(error) => Err(error),
                    };
                #[cfg(feature = "e2e")]
                tracing::warn!(target: "chimera::e2e_profile", stage = "add_replied", succeeded = result.is_ok(), "Profile source transaction stage");
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Delete { uid, reply } => {
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let result = if before.items.get(&uid).is_none() {
                    Err(ProfilesError::ProfileNotFound(uid.clone()))
                } else if let Some((referrers, current, global_transforms)) =
                    Self::referrers_of(state, &before, &uid)
                {
                    Err(ProfilesError::ProfileInUse {
                        uid: uid.clone(),
                        referrers,
                        current,
                        global_transforms,
                    })
                } else {
                    let cleanup_path = before
                        .items
                        .get(&uid)
                        .and_then(|item| item.definition.source())
                        .map(|source| source.materialized().file.clone());
                    let mut next = before.clone();
                    next.remove_item_unchecked(&uid);
                    for folder in &mut next.folders {
                        folder.members.retain(|member| member != &uid);
                    }
                    next.proxy_group_layouts.shift_remove(&uid);
                    Self::commit_with_resources(
                        &myself,
                        state,
                        expected_version,
                        before,
                        next,
                        AffectsRule::Never,
                        None,
                        cleanup_path,
                        None,
                    )
                    .await
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::Reorder { op, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    match op {
                        ReorderOp::Move { active, over } => {
                            if profiles.items.get(&active).is_none() {
                                return Err(ProfilesError::ProfileNotFound(active));
                            }
                            if profiles.items.get(&over).is_none() {
                                return Err(ProfilesError::ProfileNotFound(over));
                            }
                            profiles.reorder(&active, &over);
                        }
                        ReorderOp::ByList(list) => {
                            if list.len() != profiles.items.len() {
                                return Err(ProfilesError::ReorderListSizeMismatch {
                                    expected: profiles.items.len(),
                                    got: list.len(),
                                });
                            }
                            let mut seen = indexmap::IndexSet::with_capacity(list.len());
                            for uid in &list {
                                if !seen.insert(uid.clone()) {
                                    return Err(ProfilesError::ReorderListDuplicate {
                                        uid: uid.clone(),
                                    });
                                }
                                if profiles.items.get(uid).is_none() {
                                    return Err(ProfilesError::ProfileNotFound(uid.clone()));
                                }
                            }
                            let mut reordered = indexmap::IndexMap::with_capacity(list.len());
                            for uid in list {
                                let item = profiles
                                    .items
                                    .shift_remove(&uid)
                                    .ok_or_else(|| ProfilesError::ProfileNotFound(uid.clone()))?;
                                reordered.insert(uid, item);
                            }
                            profiles.items = reordered;
                        }
                    }
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetLayout { layout, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    profiles
                        .apply_layout(layout)
                        .map_err(ProfilesError::ValidationFailed)?;
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::CreateFolder {
                name,
                members,
                reply,
            } => {
                // The actor owns all folder writes. The generated id remains
                // unused until this mutation has finished committing.
                let id = Self::generate_folder_id(&Self::current_state(state));
                let folder_id = id.clone();
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let mut folders = profiles.folders.clone();
                    for folder in &mut folders {
                        folder.members.retain(|member| !members.contains(member));
                    }
                    folders.push(ProfileFolder { id, name, members });
                    let placed: indexmap::IndexSet<&ProfileId> =
                        folders.iter().flat_map(|folder| &folder.members).collect();
                    let ungrouped = profiles
                        .items
                        .iter()
                        .filter(|(uid, item)| item.definition.is_config() && !placed.contains(uid))
                        .map(|(uid, _)| uid.clone())
                        .collect();
                    profiles
                        .apply_layout(ProfileLayout { folders, ungrouped })
                        .map_err(ProfilesError::ValidationFailed)?;
                    Ok(AffectsRule::Never)
                })
                .await
                .map(|report| (report, folder_id));
                let _ = reply.send(result);
            }
            ProfilesActorMessage::SetProxyGroupLayout {
                profile,
                layout,
                reply,
            } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    if layout.is_empty() {
                        profiles.proxy_group_layouts.shift_remove(&profile);
                    } else {
                        profiles.proxy_group_layouts.insert(profile, layout);
                    }
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::CreateProxyGroupFolder {
                profile,
                name,
                members,
                reply,
            } => {
                let id = Self::generate_proxy_group_folder_id(
                    Self::current_state(state)
                        .proxy_group_layouts
                        .get(&profile)
                        .unwrap_or(&ProxyGroupLayout::default()),
                );
                let folder_id = id.clone();
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let mut layout = profiles
                        .proxy_group_layouts
                        .get(&profile)
                        .cloned()
                        .unwrap_or_default();
                    layout.ungrouped.retain(|group| !members.contains(group));
                    for folder in &mut layout.folders {
                        folder.members.retain(|group| !members.contains(group));
                    }
                    layout.folders.push(ProxyGroupFolder { id, name, members });
                    profiles.proxy_group_layouts.insert(profile, layout);
                    Ok(AffectsRule::Never)
                })
                .await
                .map(|report| (report, folder_id));
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchMetadata { uid, patch, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    item.apply_metadata_patch(patch);
                    Ok(AffectsRule::Never)
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::PatchRemoteOptions { uid, patch, reply } => {
                let result = Self::run_state_write(&myself, state, move |profiles| {
                    let Some(item) = profiles.items.get_mut(&uid) else {
                        return Err(ProfilesError::ProfileNotFound(uid));
                    };
                    match item.definition.source_mut() {
                        Some(ProfileSource::Remote { option, .. }) => {
                            use struct_patch::Patch as _;
                            option.apply(patch);
                            Ok(AffectsRule::Never)
                        }
                        _ => Err(ProfilesError::NotARemoteProfile),
                    }
                })
                .await;
                let _ = reply.send(result);
            }
            ProfilesActorMessage::RefreshRemote {
                uid,
                patch,
                origin,
                context,
                reply,
            } => {
                if matches!(
                    (origin, state.gate),
                    (RefreshOrigin::Scheduled, ProducerGate::Held)
                ) {
                    return Ok(());
                }
                if state.pending_refresh.contains_key(&uid) {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::RefreshFailed {
                            message: "refresh already in progress".into(),
                        }));
                    }
                    return Ok(());
                }

                if let Some(patch) = patch {
                    let patched = Self::run_state_write(&myself, state, {
                        let uid = uid.clone();
                        move |profiles| {
                            let Some(item) = profiles.items.get_mut(&uid) else {
                                return Err(ProfilesError::ProfileNotFound(uid.clone()));
                            };
                            match item.definition.source_mut() {
                                Some(ProfileSource::Remote { option, .. }) => {
                                    use struct_patch::Patch as _;
                                    option.apply(patch);
                                    Ok(AffectsRule::Never)
                                }
                                _ => Err(ProfilesError::NotARemoteProfile),
                            }
                        }
                    })
                    .await;
                    if let Err(err) = patched {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(err));
                        }
                        return Ok(());
                    }
                }

                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::ProfileNotFound(uid.clone())));
                    }
                    return Ok(());
                };
                let Some(ProfileSource::Remote { url, option, .. }) = item.definition.source()
                else {
                    if let Some(reply) = reply {
                        let _ = reply.send(Err(ProfilesError::NotARemoteProfile));
                    }
                    return Ok(());
                };

                let definition = item.definition.clone();
                let definition_fingerprint = match serde_yaml::to_string(&definition) {
                    Ok(fingerprint) => fingerprint,
                    Err(error) => {
                        if let Some(reply) = reply {
                            let _ = reply.send(Err(ProfilesError::RefreshFailed {
                                message: format!(
                                    "failed to fingerprint subscription definition: {error}"
                                ),
                            }));
                        }
                        return Ok(());
                    }
                };
                let url = url.clone();
                let option = option.clone();
                let token = state.next_refresh_token;
                state.next_refresh_token = state.next_refresh_token.wrapping_add(1).max(1);
                let fetcher = Arc::clone(&state.fetcher);
                let actor = myself.clone();
                let task_context = context.clone();
                let commit_uid = uid.clone();
                let task = tokio::spawn(async move {
                    // Download and validate only: the file write happens in the
                    // CommitRefreshed handler, after the stale-download fence,
                    // so an in-flight refresh can never clobber the file of a
                    // definition that was replaced meanwhile.
                    let fetch = async {
                        tracing::info!(target: "chimera::profile_sync", stage = "download", "Downloading profile");
                        let fetched = fetcher.fetch(&url, &option).await.map_err(|error| {
                            let message = format!("download failed: {error}");
                            let source =
                                error
                                    .downcast::<SubscriptionFetchError>()
                                    .ok()
                                    .map(|source| RefreshFailureSource::Subscription {
                                        url: url.clone(),
                                        source,
                                    });
                            (message, source)
                        })?;
                        Self::validate_fetched_content_typed(&definition, &fetched.content)
                            .map_err(|source| {
                                (
                                    Self::content_error_message(&source),
                                    Some(RefreshFailureSource::Content(source)),
                                )
                            })?;
                        tracing::info!(target: "chimera::profile_sync", stage = "validated", "Profile content validated");
                        Ok::<_, (String, Option<RefreshFailureSource>)>((
                            fetched.subscription,
                            fetched.suggested_update_interval_minutes,
                            fetched.content,
                            fetched.filename,
                        ))
                    };
                    let outcome = match task_context {
                        Some(context) => context.0.instrument(fetch).await,
                        None => fetch.await,
                    };
                    let outcome = match outcome {
                        Ok((
                            subscription,
                            suggested_update_interval_minutes,
                            content,
                            filename,
                        )) => RefreshOutcome::Succeeded {
                            subscription,
                            suggested_update_interval_minutes,
                            content,
                            filename,
                        },
                        Err((message, source)) => RefreshOutcome::Failed { message, source },
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitRefreshed {
                        uid: commit_uid,
                        token,
                        url,
                        definition_fingerprint,
                        outcome,
                    });
                });
                state.pending_refresh.insert(
                    uid,
                    PendingRefresh {
                        context,
                        token,
                        reply,
                        task,
                    },
                );
            }
            ProfilesActorMessage::CommitRefreshed {
                uid,
                token,
                url,
                definition_fingerprint,
                outcome,
            } => {
                if state
                    .pending_refresh
                    .get(&uid)
                    .is_none_or(|pending| pending.token != token)
                {
                    return Ok(());
                }
                let Some(pending) = state.pending_refresh.remove(&uid) else {
                    return Ok(());
                };
                let context = pending.context;
                let reply = pending.reply;
                let commit = async {
                    match outcome {
                        RefreshOutcome::Failed { message, source } => {
                            Err(Self::fetch_failure(message, source, false))
                        }
                        RefreshOutcome::Succeeded {
                            subscription,
                            suggested_update_interval_minutes: _,
                            content,
                            filename,
                        } => {
                            let versioned = state.manager.snapshot_handle().load();
                            let expected_version = versioned.version;
                            let before = versioned.state.clone();
                            drop(versioned);
                            match before.items.get(&uid) {
                                None => Err(ProfilesError::RefreshFailed {
                                    message: "profile deleted during refresh".into(),
                                }),
                                Some(current) => {
                                    let current_fingerprint = serde_yaml::to_string(
                                        &current.definition,
                                    )
                                    .map_err(|error| ProfilesError::RefreshFailed {
                                        message: format!(
                                            "failed to fingerprint current definition: {error}"
                                        ),
                                    });
                                    match current_fingerprint {
                                        Err(error) => Err(error),
                                        Ok(current_fingerprint)
                                            if current_fingerprint != definition_fingerprint =>
                                        {
                                            Err(ProfilesError::RefreshFailed {
                                                message:
                                                    "subscription definition changed during refresh"
                                                        .into(),
                                            })
                                        }
                                        Ok(_) => match current.definition.source() {
                                            Some(ProfileSource::Remote {
                                                url: current_url,
                                                materialized,
                                                ..
                                            }) if *current_url == url => {
                                                let path = materialized.file.clone();
                                                if let Err(message) = Self::validate_fetched_content(
                                                    &current.definition,
                                                    &content,
                                                ) {
                                                    Err(ProfilesError::RefreshFailed {
                                                        message: format!(
                                                            "stale download no longer valid for the current definition: {message}"
                                                        ),
                                                    })
                                                } else {
                                                    let mut next = before.clone();
                                                    let item = next
                                                .items
                                                .get_mut(&uid)
                                                .expect("fenced profile remains in the candidate snapshot");
                                                    if let Some(name) = synced_name(
                                                        item.metadata.custom_name,
                                                        &filename,
                                                    ) {
                                                        item.metadata.name = name;
                                                    }
                                                    match item.definition.source_mut() {
                                                        Some(ProfileSource::Remote {
                                                            materialized,
                                                            subscription: slot,
                                                            ..
                                                        }) => {
                                                            materialized.updated_at = Some(
                                                                time::OffsetDateTime::now_utc(),
                                                            );
                                                            *slot = subscription;
                                                            // Manual/scheduled refresh never adopts
                                                            // server interval suggestions; import
                                                            // applies them only on first commit.
                                                            Self::commit_file_first(
                                                                &myself,
                                                                state,
                                                                expected_version,
                                                                before,
                                                                next,
                                                                AffectsRule::Touched(uid.clone()),
                                                                path,
                                                                content,
                                                            )
                                                            .await
                                                        }
                                                        _ => Err(ProfilesError::NotARemoteProfile),
                                                    }
                                                }
                                            }
                                            _ => Err(ProfilesError::RefreshFailed {
                                                message:
                                                    "subscription definition changed during refresh"
                                                        .into(),
                                            }),
                                        },
                                    }
                                }
                            }
                        }
                    }
                };
                let result = match context {
                    Some(context) => context.0.instrument(commit).await,
                    None => commit.await,
                };

                if let Some(reply) = reply {
                    let _ = reply.send(result);
                }
            }
            ProfilesActorMessage::ImportRemote {
                url,
                metadata,
                option,
                update_interval_explicit,
                folder,
                reply,
            } => {
                let before = Self::current_state(state);
                if let Err(error) = Self::validate_import_request(
                    &before,
                    &metadata,
                    url.clone(),
                    option.clone(),
                    folder.as_ref(),
                ) {
                    let _ = reply.send(Err(error));
                    return Ok(());
                }

                let token = ImportOperationToken(state.next_import_token);
                state.next_import_token = state.next_import_token.wrapping_add(1).max(1);
                state.pending_imports.insert(
                    token,
                    PendingImport {
                        reply,
                        metadata,
                        url: url.clone(),
                        option: option.clone(),
                        update_interval_explicit,
                        folder,
                    },
                );

                let fetcher = Arc::clone(&state.fetcher);
                let actor = myself.clone();
                // Content validation uses a Config definition shape; import is
                // always a remote Config File profile.
                let definition_for_validation = Self::remote_import_definition(
                    url.clone(),
                    option.clone(),
                    ManagedProfilePath::new("pending.yaml").expect("static managed path is valid"),
                    SubscriptionInfo::default(),
                    None,
                );
                // Supervise the fetch future so a panic still produces one
                // CommitImported outcome. Without this, an unsupervised panic
                // leaves pending_imports and a timeout-less RPC stuck forever.
                // Actor shutdown / cast failure remains safe: pending state drops.
                tokio::spawn(async move {
                    let fetch_result = tokio::spawn(async move {
                        let fetched = fetcher.fetch(&url, &option).await.map_err(|error| {
                            let message = format!("download failed: {error}");
                            let source =
                                error
                                    .downcast::<SubscriptionFetchError>()
                                    .ok()
                                    .map(|source| RefreshFailureSource::Subscription {
                                        url: url.clone(),
                                        source,
                                    });
                            (message, source)
                        })?;
                        Self::validate_fetched_content_typed(
                            &definition_for_validation,
                            &fetched.content,
                        )
                        .map_err(|source| {
                            (
                                Self::content_error_message(&source),
                                Some(RefreshFailureSource::Content(source)),
                            )
                        })?;
                        Ok::<_, (String, Option<RefreshFailureSource>)>((
                            fetched.subscription,
                            fetched.suggested_update_interval_minutes,
                            fetched.content,
                            fetched.filename,
                        ))
                    })
                    .await;
                    let outcome = match fetch_result {
                        Ok(Ok((
                            subscription,
                            suggested_update_interval_minutes,
                            content,
                            filename,
                        ))) => RefreshOutcome::Succeeded {
                            subscription,
                            suggested_update_interval_minutes,
                            content,
                            filename,
                        },
                        Ok(Err((message, source))) => RefreshOutcome::Failed { message, source },
                        // Do not downcast panic payloads; emit a stable diagnostic.
                        Err(join_error) => RefreshOutcome::Failed {
                            message: if join_error.is_panic() {
                                "subscription fetch task panicked".into()
                            } else {
                                "subscription fetch task cancelled".into()
                            },
                            source: None,
                        },
                    };
                    let _ = actor.cast(ProfilesActorMessage::CommitImported { token, outcome });
                });
            }
            ProfilesActorMessage::CommitImported { token, outcome } => {
                let Some(pending) = state.pending_imports.remove(&token) else {
                    // Actor restart / late completion: nothing durable was written.
                    return Ok(());
                };
                // Cancellation before durable commit begins: discard fetch result.
                if pending.reply.is_closed() {
                    return Ok(());
                }

                let result = match outcome {
                    RefreshOutcome::Failed { message, source } => {
                        Err(Self::fetch_failure(message, source, true))
                    }
                    RefreshOutcome::Succeeded {
                        subscription,
                        suggested_update_interval_minutes,
                        content,
                        filename,
                    } => {
                        let versioned = state.manager.snapshot_handle().load();
                        let expected_version = versioned.version;
                        let before = versioned.state.clone();
                        drop(versioned);

                        let mut option = pending.option;
                        if !pending.update_interval_explicit
                            && let Some(minutes) = suggested_update_interval_minutes
                        {
                            option.update_interval_minutes =
                                RemoteSyncInterval::from_minutes(minutes);
                        }
                        let mut metadata = pending.metadata;
                        if let Some(name) = synced_name(metadata.custom_name, &filename) {
                            metadata.name = name;
                        }

                        let mut definition = Self::remote_import_definition(
                            pending.url,
                            option,
                            ManagedProfilePath::new("pending.yaml")
                                .expect("static managed path is valid"),
                            subscription,
                            Some(time::OffsetDateTime::now_utc()),
                        );
                        if let Err(message) = Self::validate_fetched_content(&definition, &content)
                        {
                            Err(ProfilesError::ImportFailed {
                                message: format!(
                                    "downloaded content is not valid for import: {message}"
                                ),
                            })
                        } else {
                            let uid = Self::generate_uid(&definition, &before);
                            let ext = Self::canonical_extension(&definition);
                            let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                                .expect("uid-derived path is always a valid managed path");
                            if let Some(source) = definition.source_mut() {
                                source.materialized_mut().file = canonical.clone();
                            }
                            let mut next = before.clone();
                            if !next.append_item(ProfileItem {
                                uid: uid.clone(),
                                metadata,
                                definition,
                            }) {
                                Err(ProfilesError::Persist("uid collision".into()))
                            } else if let Err(error) =
                                Self::join_folder(&mut next, &uid, pending.folder.as_ref())
                            {
                                Err(error)
                            } else {
                                // If the caller closed between the pre-check and
                                // the first durable step, a complete valid profile
                                // may still remain — never an empty shell.
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Never,
                                    Some((canonical, MaterializationResource::File { content })),
                                    None,
                                    Some(uid),
                                )
                                .await
                            }
                        }
                    }
                };
                let _ = pending.reply.send(result);
            }
            ProfilesActorMessage::ExternalFileChanged { uid } => {
                if state.gate != ProducerGate::Running {
                    return Ok(());
                }
                let snapshot = Self::current_state(state);
                let Some(item) = snapshot.items.get(&uid) else {
                    return Ok(());
                };
                let Some(ProfileSource::Local {
                    binding:
                        LocalBinding::External {
                            materialized,
                            target,
                            mode,
                        },
                }) = item.definition.source()
                else {
                    return Ok(());
                };

                if *mode == ExternalMode::Mirror {
                    let expected_target = target.clone();
                    let expected_path = materialized.file.clone();
                    let expected_fingerprint = match serde_yaml::to_string(&item.definition) {
                        Ok(fingerprint) => fingerprint,
                        Err(error) => {
                            tracing::warn!(uid = %uid, %error, "failed to fingerprint external profile");
                            return Ok(());
                        }
                    };
                    let definition = item.definition.clone();
                    let fs = Arc::clone(&state.fs);
                    let read_target = expected_target.clone();
                    let content =
                        tokio::task::spawn_blocking(move || fs.read_external(&read_target))
                            .await
                            .map_err(|error| {
                                anyhow::anyhow!("mirror source read task failed: {error}")
                            })
                            .and_then(|content| content);
                    let content = match content {
                        Ok(content) => content,
                        Err(error) => {
                            tracing::warn!(uid = %uid, %error, "failed to read changed external profile");
                            return Ok(());
                        }
                    };
                    if let Err(error) = Self::validate_fetched_content(&definition, &content) {
                        tracing::warn!(uid = %uid, %error, "changed external profile failed validation");
                        return Ok(());
                    }

                    let versioned = state.manager.snapshot_handle().load();
                    let expected_version = versioned.version;
                    let before = versioned.state.clone();
                    drop(versioned);
                    let Some(current) = before.items.get(&uid) else {
                        return Ok(());
                    };
                    let still_current = matches!(
                        current.definition.source(),
                        Some(ProfileSource::Local {
                            binding: LocalBinding::External {
                                materialized,
                                target,
                                mode: ExternalMode::Mirror,
                            },
                        }) if *target == expected_target && materialized.file == expected_path
                    ) && serde_yaml::to_string(&current.definition)
                        .is_ok_and(|fingerprint| fingerprint == expected_fingerprint);
                    if !still_current {
                        return Ok(());
                    }
                    let mut next = before.clone();
                    let item = next
                        .items
                        .get_mut(&uid)
                        .expect("fenced external profile remains in the candidate snapshot");
                    let Some(ProfileSource::Local {
                        binding: LocalBinding::External { materialized, .. },
                    }) = item.definition.source_mut()
                    else {
                        return Ok(());
                    };
                    materialized.updated_at = Some(time::OffsetDateTime::now_utc());
                    match Self::commit_file_first(
                        &myself,
                        state,
                        expected_version,
                        before,
                        next,
                        AffectsRule::Touched(uid.clone()),
                        expected_path,
                        content,
                    )
                    .await
                    {
                        Ok(_) => {}
                        Err(error) => {
                            tracing::warn!(uid = %uid, %error, "failed to commit external profile change")
                        }
                    }
                    return Ok(());
                }

                let result = Self::run_state_write(&myself, state, {
                    let uid = uid.clone();
                    move |profiles| {
                        let Some(item) = profiles.items.get_mut(&uid) else {
                            return Err(ProfilesError::ProfileNotFound(uid.clone()));
                        };
                        match item.definition.source_mut() {
                            Some(ProfileSource::Local {
                                binding: LocalBinding::External { materialized, .. },
                            }) => {
                                materialized.updated_at = Some(time::OffsetDateTime::now_utc());
                                Ok(AffectsRule::Touched(uid.clone()))
                            }
                            _ => Err(ProfilesError::ProfileNotFound(uid.clone())),
                        }
                    }
                })
                .await;
                match result {
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(uid = %uid, %error, "failed to commit external profile change")
                    }
                }
            }
            ProfilesActorMessage::ReplaceDefinition {
                uid,
                definition,
                reply,
            } => {
                let versioned = state.manager.snapshot_handle().load();
                let expected_version = versioned.version;
                let before = versioned.state.clone();
                drop(versioned);
                let result = match before.items.get(&uid) {
                    None => Err(ProfilesError::ProfileNotFound(uid.clone())),
                    Some(previous_item) => {
                        let previous_source = previous_item.definition.source().cloned();
                        let mut definition = definition;
                        let ext = Self::canonical_extension(&definition);
                        let canonical = ManagedProfilePath::new(format!("{uid}.{ext}"))
                            .expect("uid-derived path is always a valid managed path");
                        let same_slot = match (&previous_source, definition.source()) {
                            (Some(previous), Some(next)) => {
                                Self::retains_materialization(previous, next, &canonical)
                            }
                            _ => false,
                        };
                        if let Some(source) = definition.source_mut() {
                            let materialized = source.materialized_mut();
                            materialized.file = canonical.clone();
                            materialized.updated_at = if same_slot {
                                previous_source
                                    .as_ref()
                                    .and_then(|source| source.materialized().updated_at)
                            } else {
                                None
                            };
                            if let ProfileSource::Remote { subscription, .. } = source {
                                *subscription = match (same_slot, previous_source.as_ref()) {
                                    (
                                        true,
                                        Some(ProfileSource::Remote {
                                            subscription: previous,
                                            ..
                                        }),
                                    ) => previous.clone(),
                                    _ => SubscriptionInfo::default(),
                                };
                            }
                        }
                        let cleanup_path = previous_source.as_ref().and_then(|previous| {
                            let old_path = previous.materialized().file.clone();
                            (definition.source().is_none() || old_path != canonical)
                                .then_some(old_path)
                        });
                        // A changed remote definition receives a durable empty
                        // placeholder instead of retaining stale bytes. Its next
                        // refresh replaces it through the file-first protocol.
                        let resource = if same_slot {
                            Ok(None)
                        } else {
                            Self::resource_for_definition(state, &definition, None).await
                        };
                        match resource {
                            Err(error) => Err(error),
                            Ok(resource) => {
                                let mut next = before.clone();
                                let item = next
                                    .items
                                    .get_mut(&uid)
                                    .expect("replacement target remains in the candidate snapshot");
                                item.set_definition(definition);
                                Self::commit_with_resources(
                                    &myself,
                                    state,
                                    expected_version,
                                    before,
                                    next,
                                    AffectsRule::Touched(uid),
                                    resource.map(|resource| (canonical, resource)),
                                    cleanup_path,
                                    None,
                                )
                                .await
                            }
                        }
                    }
                };
                let _ = reply.send(result);
            }
            ProfilesActorMessage::ReconcileMaterializations => {
                if state.gate != ProducerGate::Running {
                    return Ok(());
                }
                match Self::reconcile_materializations(state).await {
                    Ok(report) => Self::log_reconcile_report(&report),
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            "profile materialization reconcile failed"
                        );
                    }
                }
            }
            ProfilesActorMessage::StartProducers { reply } => {
                Self::start_producers(&myself, state).await;
                let _ = reply.send(Ok(()));
            }
        }
        Ok(())
    }

    async fn post_stop(
        &self,
        _myself: ActorRef<Self::Msg>,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        if let Some(handle) = state.reconcile_task.take() {
            handle.abort();
        }
        state.external_watchers.shutdown();
        let mut pending = Vec::new();
        for (_, refresh) in state.pending_refresh.drain() {
            refresh.task.abort();
            pending.push((refresh.task, refresh.reply));
        }
        for (task, reply) in pending {
            if let Err(error) = task.await
                && let Ok(panic) = error.try_into_panic()
            {
                std::panic::resume_unwind(panic);
            }
            if let Some(reply) = reply {
                let _ = reply.send(Err(ProfilesError::ShuttingDown));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_definition() -> ProfileDefinition {
        ProfileDefinition::Config {
            config: ConfigDefinition::File(FileConfig {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("p1.yaml").unwrap(),
                            updated_at: None,
                        },
                    },
                },
                transforms: vec![],
            }),
        }
    }

    fn overlay_definition() -> ProfileDefinition {
        ProfileDefinition::Transform {
            transform: TransformDefinition::Overlay(chimera_config::profile::OverlayTransform {
                source: ProfileSource::Local {
                    binding: LocalBinding::Managed {
                        materialized: MaterializedFile {
                            file: ManagedProfilePath::new("t1.yaml").unwrap(),
                            updated_at: None,
                        },
                    },
                },
            }),
        }
    }

    /// Config subscriptions must carry proxies (legacy remote.rs semantics);
    /// overlays only require a mapping.
    #[test]
    fn runtime_check_failure_retains_real_core_kind_and_retryability() {
        use crate::client::application_workflow::mutation::RuntimeCheckFailure;
        use chimera_contracts::runtime_error::RuntimeCheckUnavailable;
        use chimera_core_manager::{CoreError, CoreErrorKind};

        let refusal = ProfilesActor::runtime_gate_failure(
            crate::client::reconciler::legacy_check::typed_gate_error(
                "profiles",
                anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                    CoreErrorKind::ConfigCheckFailed,
                    "invalid proposed YAML",
                    false,
                ))),
            ),
        );
        let ProfilesError::Commit {
            source: CommitAborted::RuntimeRefused { errors, source },
        } = &refusal
        else {
            panic!("a Core check rejection must not be recorded as unavailable");
        };
        assert!(matches!(
            errors[0].as_ref(),
            RuntimeError::CoreRejectedConfig {
                core_kind: Some(CoreErrorKind::ConfigCheckFailed),
                ..
            }
        ));
        assert!(source.downcast_ref::<Arc<RuntimeError>>().is_some());
        assert_eq!(
            serde_json::to_value(&refusal.typed_failure().unwrap()).unwrap()["reason"],
            "runtime_refused"
        );

        let unavailable = ProfilesActor::runtime_gate_failure(
            crate::client::reconciler::legacy_check::typed_gate_error(
                "profiles",
                anyhow::Error::new(RuntimeCheckFailure::from_core_error(CoreError::new(
                    CoreErrorKind::BackendUnavailable,
                    "service is reconnecting",
                    true,
                ))),
            ),
        );
        let ProfilesError::Commit {
            source: CommitAborted::RuntimeFailed { errors, .. },
        } = &unavailable
        else {
            panic!("failed validation must preserve the missing verdict");
        };
        assert!(matches!(
            errors[0].as_ref(),
            RuntimeError::CheckUnavailable {
                reason: RuntimeCheckUnavailable::Backend {
                    kind: Some(CoreErrorKind::BackendUnavailable),
                    retryable: true,
                    ..
                }
            }
        ));
    }

    #[test]
    fn unknown_validator_error_does_not_forge_an_explicit_core_rejection() {
        let error = ProfilesActor::runtime_gate_failure(
            crate::client::reconciler::legacy_check::typed_gate_error(
                "profiles",
                anyhow::anyhow!("candidate input resolver could not be reached"),
            ),
        );
        assert!(matches!(
            &error,
            ProfilesError::Commit {
                source: CommitAborted::RuntimeFailed { .. }
            }
        ));
        assert_eq!(
            serde_json::to_value(error.typed_failure().unwrap()).unwrap()["reason"],
            "runtime_failed"
        );
    }

    #[test]
    fn profile_reference_and_reorder_errors_preserve_typed_source_fields() {
        let cases = [
            (
                ProfilesError::ProfileInUse {
                    uid: ProfileId("target".into()),
                    referrers: vec![ProfileId("referrer".into())],
                    current: true,
                    global_transforms: false,
                },
                "profile_in_use",
            ),
            (
                ProfilesError::ReorderListSizeMismatch {
                    expected: 3,
                    got: 2,
                },
                "reorder_list_size_mismatch",
            ),
            (
                ProfilesError::ReorderListDuplicate {
                    uid: ProfileId("duplicate".into()),
                },
                "reorder_list_duplicate",
            ),
        ];
        for (error, expected_kind) in cases {
            let wire = serde_json::to_value(error.typed_failure().unwrap()).unwrap();
            assert_eq!(wire["kind"], expected_kind);
        }
    }

    #[test]
    fn source_write_failures_keep_actual_manager_error_and_typed_commit_class() {
        use chimera_core::state::ReplaceIfVersionError;

        for (source, expected) in [
            (
                ReplaceIfVersionError::WriteConfig(anyhow::anyhow!("config was not persisted")),
                CommitFailureReason::WriteConfig,
            ),
            (
                ReplaceIfVersionError::LocalWrite(anyhow::Error::new(
                    ProfileFileError::JournalNotFound {
                        operation_id: "staged-1".into(),
                    },
                )),
                CommitFailureReason::LocalWrite,
            ),
        ] {
            let reason = ProfilesActor::commit_failure_reason(&source);
            assert_eq!(reason, Some(expected));
            let failure = ProfilesError::SourceTransaction {
                reason,
                source,
                cleanup: None,
                cleanup_failures: Vec::new(),
            };
            let typed = failure.typed_failure().expect("actual commit category");
            let wire = serde_json::to_value(typed).unwrap();
            assert_eq!(wire["kind"], "commit");
            assert_eq!(wire["reason"], serde_json::to_value(expected).unwrap(),);
            assert!(std::error::Error::source(&failure).is_some());
        }
    }

    #[test]
    fn failed_materialization_recovery_retains_both_independent_port_causes() {
        use chimera_core::state::ReplaceIfVersionError;

        let resource = ProfileFileError::JournalNotFound {
            operation_id: "resource-staged".into(),
        };
        let cleanup = ProfileFileError::CleanupAlreadyActivated;
        let recovery = anyhow::Error::new(MaterializationRecoveryFailure {
            resource: Some(anyhow::Error::new(resource)),
            cleanup: Some(anyhow::Error::new(cleanup)),
        });
        let source = ReplaceIfVersionError::ResourceRecovery {
            cause: anyhow::anyhow!("file source write refused"),
            recovery_error: recovery,
        };
        let reason = ProfilesActor::commit_failure_reason(&source);
        assert_eq!(reason, Some(CommitFailureReason::RecoverAfterWriteFailure));
        let ReplaceIfVersionError::ResourceRecovery { recovery_error, .. } = &source else {
            unreachable!()
        };
        let retained = recovery_error
            .downcast_ref::<MaterializationRecoveryFailure>()
            .expect("original port failures must survive persistence recovery");
        assert!(retained.resource.as_ref().unwrap().is::<ProfileFileError>());
        assert!(retained.cleanup.as_ref().unwrap().is::<ProfileFileError>());
        let refused = ProfilesError::SourceTransaction {
            source,
            reason,
            cleanup: None,
            cleanup_failures: Vec::new(),
        };
        assert!(matches!(
            refused.typed_failure(),
            Some(ProfileFailure::Commit {
                reason: CommitFailureReason::RecoverAfterWriteFailure
            })
        ));
    }

    #[test]
    fn refused_precommit_cleanup_keeps_both_sources_and_the_legacy_message() {
        let cleanup_errors = vec![
            anyhow::Error::new(ProfileFileError::JournalNotFound {
                operation_id: "staged".into(),
            }),
            anyhow::Error::new(ProfileFileError::CleanupAlreadyActivated),
        ];
        let combined = cleanup_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ");
        let conflict = ProfilesError::ConflictCleanup {
            expected: 3,
            actual: 4,
            cleanup: combined.clone(),
            cleanup_failures: cleanup_errors,
        };
        let ProfilesError::ConflictCleanup {
            cleanup_failures, ..
        } = &conflict
        else {
            unreachable!()
        };
        assert_eq!(cleanup_failures.len(), 2);
        assert!(
            cleanup_failures
                .iter()
                .all(|error| error.is::<ProfileFileError>())
        );
        assert_eq!(
            serde_json::to_value(conflict.typed_failure().unwrap()).unwrap()["cleanup"],
            combined
        );
    }

    #[test]
    fn profile_validation_and_version_conflicts_keep_original_typed_sources() {
        let id = ProfileId("missing".into());
        let error = ProfilesError::ValidationFailed(vec![ProfileValidationError::CurrentNotFound(
            id.clone(),
        )]);
        let typed = error.typed_failure().expect("typed validation reason");
        let wire = serde_json::to_value(typed).unwrap();
        assert_eq!(wire["kind"], "validation_failed");
        assert_eq!(wire["errors"][0]["CurrentNotFound"], "missing");

        let conflict = ProfilesError::ConflictCleanup {
            expected: 10,
            actual: 11,
            cleanup: "staging recovery needs reconciliation".into(),
            cleanup_failures: vec![anyhow::anyhow!("staging recovery needs reconciliation")],
        };
        let wire = serde_json::to_value(conflict.typed_failure().unwrap()).unwrap();
        assert_eq!(wire["kind"], "version_conflict");
        assert_eq!(wire["expected"], 10);
        assert_eq!(wire["actual"], 11);
        assert_eq!(wire["cleanup"], "staging recovery needs reconciliation");
        assert!(
            ProfilesError::Persist("unspecified".into())
                .typed_failure()
                .is_none()
        );
    }

    #[test]
    fn file_materialization_error_retains_its_actual_port_source() {
        let source = ProfileFileError::ReadFile {
            path: std::path::PathBuf::from("profiles/materialized.yaml").into(),
            source: std::io::Error::other("filesystem permission denied"),
        };
        let error = ProfilesActor::materialization_error("prepare materialization", source);
        let ProfilesError::MaterializationFile {
            context,
            operation,
            source,
        } = &error
        else {
            panic!("typed ProfileFileError must survive the materialization boundary");
        };
        assert_eq!(*context, "prepare materialization");
        assert_eq!(*operation, Some(MaterializationOperation::PrepareFileFirst));
        assert!(source.to_string().contains("profiles/materialized.yaml"));
        assert!(
            std::error::Error::source(&error)
                .expect("the actor preserves the original file failure")
                .to_string()
                .contains("profiles/materialized.yaml")
        );
    }

    #[test]
    fn typed_content_validation_keeps_original_yaml_and_expected_ref_reason() {
        let definition = config_definition();
        let error = ProfilesActor::validate_fetched_content_typed(&definition, "[")
            .expect_err("invalid YAML must be rejected before source commit");
        assert!(matches!(error, ProfileContentError::NotYamlMapping { .. }));
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({ "kind": "not_yaml_mapping" }),
        );
        let missing = ProfilesActor::validate_fetched_content_typed(&definition, "{}")
            .expect_err("remote config must have proxies or providers");
        assert!(matches!(missing, ProfileContentError::MissingProxies));
        assert_eq!(
            serde_json::to_value(missing).unwrap(),
            serde_json::json!({ "kind": "missing_proxies" }),
        );
    }

    #[test]
    fn real_subscription_fetch_source_survives_mailbox_settlement() {
        let source = SubscriptionFetchError::SubscriptionHttpStatus { status: 403 };
        let error = ProfilesActor::fetch_failure(
            "download failed: 403".into(),
            Some(RefreshFailureSource::Subscription {
                url: url::Url::parse("https://example.com/sub.yaml").unwrap(),
                source,
            }),
            false,
        );
        assert!(matches!(
            &error,
            ProfilesError::FetchSubscription {
                source: SubscriptionFetchError::SubscriptionHttpStatus { status: 403 },
                ..
            }
        ));
        assert!(std::error::Error::source(&error).is_some());
        let unknown = ProfilesActor::fetch_failure("download failed: other".into(), None, true);
        assert!(matches!(unknown, ProfilesError::ImportFailed { .. }));
    }

    #[test]
    fn config_content_requires_proxies_key() {
        let definition = config_definition();
        assert!(ProfilesActor::validate_fetched_content(&definition, "{}\n").is_err());
        assert!(ProfilesActor::validate_fetched_content(&definition, "proxies: []\n").is_ok());
        assert!(
            ProfilesActor::validate_fetched_content(&definition, "proxy-providers: {}\n").is_ok()
        );
    }

    #[test]
    fn overlay_content_needs_only_a_mapping() {
        let definition = overlay_definition();
        assert!(ProfilesActor::validate_fetched_content(&definition, "a: 1\n").is_ok());
    }

    #[test]
    fn synced_name_syncs_only_unpinned_profiles_with_a_server_name() {
        assert_eq!(
            synced_name(false, &Some("Server Name".into())),
            Some("Server Name".into())
        );
        assert_eq!(synced_name(true, &Some("Server Name".into())), None);
        assert_eq!(synced_name(false, &None), None);
        assert_eq!(synced_name(false, &Some("   ".into())), None);
    }
}
