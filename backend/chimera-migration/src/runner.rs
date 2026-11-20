//! Pure migration scheduling decisions from the ref Runner path.
//! IO, backup creation, and module execution remain injected by the application adapter.

use crate::{
    MigrationAdvice, MigrationState, StepSpec,
    context::Ctx,
    document,
    module::{
        DocumentSpec, MigrationCheckError, MigrationStep, ModuleKind, ModuleMigrator, StepCheck,
    },
    registry::Registry,
    store::{MigrationStore, ModuleState, STORE_FILE_NAME},
};
use anyhow::Context as _;
use semver::Version;
use std::{fs, io, path::Path};

/// A snapshot producer injected by the application shell. The shared Runner
/// decides *whether* migration must be blocked on a backup, while the shell
/// provides platform-aware file enumeration and verified manifest creation.
pub trait BackupPort {
    type Snapshot;

    fn create_snapshot(&mut self, ctx: &Ctx) -> anyhow::Result<Self::Snapshot>;
}

/// Ensure that existing user state is snapshotted exactly once before any
/// migration rewrite. A failed snapshot is not recorded, so a later retry can
/// create a new one. No writes happen in this shared gate itself.
pub fn ensure_backup<P: BackupPort>(
    ctx: &Ctx,
    existing: &mut Option<P::Snapshot>,
    port: &mut P,
) -> anyhow::Result<bool> {
    if existing.is_some() || has_nothing_to_protect(ctx)? {
        return Ok(false);
    }
    let snapshot = port
        .create_snapshot(ctx)
        .map_err(|source| ExecutionError::CreateBackup { source })?;
    *existing = Some(snapshot);
    Ok(true)
}

/// A fresh installation may have only migration-state baselines. Never treat
/// a dangling database symlink as absent or ignore an inaccessible root.
pub fn has_nothing_to_protect(ctx: &Ctx) -> anyhow::Result<bool> {
    let storage_path = ctx.storage_path();
    if crate::fs::config_node_exists(&storage_path)
        .with_context(|| format!("failed to inspect {}", storage_path.display()))?
    {
        return Ok(false);
    }
    let config = ctx.paths().app_config_dir();
    let entries = match fs::read_dir(config) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(error) => {
            return Err(error).with_context(|| format!("failed to list {}", config.display()));
        }
    };
    for entry in entries {
        let entry = entry.with_context(|| format!("failed to inspect {}", config.display()))?;
        if entry.file_name() != STORE_FILE_NAME {
            return Ok(false);
        }
    }
    Ok(true)
}

/// An on-disk revision written by a future build; never downgrade it.
#[derive(Debug, thiserror::Error)]
pub enum RevisionError {
    #[error("{} is stamped at revision {read}, but this build only knows revisions up to {supported}; the file was written by a newer version. Update to that version, or restore this file from a compatible backup.", path.display())]
    NewerDocument {
        path: std::path::PathBuf,
        read: u64,
        supported: u64,
    },
    #[error(
        "migration state records {module} at revision {read}, but this build only knows revisions up to {supported}; the data was migrated by a newer version"
    )]
    NewerAppliedModule {
        path: std::path::PathBuf,
        module: String,
        read: u64,
        supported: u64,
    },
    #[error("{} is stamped at revision {read}, although {} records {module} as migrated to revision {applied}. The file may have been restored from a backup or rewritten by an older version. Restore it, or delete {} so the file is migrated again from its stamp.", path.display(), state_path.display(), state_path.display())]
    DocumentBehindState {
        path: std::path::PathBuf,
        state_path: std::path::PathBuf,
        module: String,
        read: u64,
        applied: u64,
    },
    #[error("{} has lost the `_nyanpasu` stamp it was written with; it may have been rewritten by an older version or edited by hand. Restore it from a backup, or delete {} so its revision is detected from its content.", path.display(), state_path.display())]
    MissingDocumentStamp {
        path: std::path::PathBuf,
        state_path: std::path::PathBuf,
    },
    #[error("{reason}, although {} records {module} as migrated to revision {applied}. The files may have been moved, restored from a backup or rewritten by an older version. Restore them, or delete {} so the files on disk are detected and migrated again.", state_path.display(), state_path.display())]
    FilesBehindState {
        state_path: std::path::PathBuf,
        module: String,
        applied: u64,
        reason: String,
    },
}

/// Structured execution failure variants from ref Runner. The checked
/// migration contracts remain unchanged: each original source is kept in the
/// error chain for startup diagnostics and safe retries.
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("failed to detect {module} baseline")]
    DetectBaseline {
        module: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to inspect {module} files")]
    InspectModule {
        module: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to inspect the {module} files")]
    InspectDocument {
        module: String,
        #[source]
        source: MigrationCheckError,
    },
    #[error("failed to check migration {step}")]
    CheckStep {
        step: String,
        #[source]
        source: MigrationCheckError,
    },
    #[error("failed to run migration {step}")]
    RunStep {
        step: String,
        #[source]
        source: anyhow::Error,
    },
    #[error(
        "migration {step} ran without leaving its document stamped; this is a bug in the migration"
    )]
    VerifyDocumentStamp { step: String },
    #[error(
        "migration {step} ran but its check still reports it as needed; this is a bug in the migration"
    )]
    VerifyStep { step: String },
    /// Exposes both failures: the original step error remains the source,
    /// while callers can inspect the independent persistence error by type.
    #[error("failed to persist failed migration {step} ({persistence:#})")]
    RecordFailure {
        step: String,
        #[source]
        source: anyhow::Error,
        persistence: anyhow::Error,
    },
    #[error("failed to persist {operation} at {}", path.display())]
    PersistState {
        operation: String,
        path: std::path::PathBuf,
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to back up the config before migrating")]
    CreateBackup {
        #[source]
        source: anyhow::Error,
    },
    #[error("failed to stamp the {module} files at {}", path.display())]
    StampDocument {
        module: String,
        path: std::path::PathBuf,
        #[source]
        source: anyhow::Error,
    },
}

/// One error boundary for every shared Runner state transition.
/// Preserve the actual flush error for callers that inspect IO causes.
fn persist_state(
    store: &MigrationStore,
    state_path: &Path,
    operation: impl Into<String>,
) -> anyhow::Result<()> {
    store.flush_atomic(state_path).map_err(|source| {
        ExecutionError::PersistState {
            operation: operation.into(),
            path: state_path.to_path_buf(),
            source,
        }
        .into()
    })
}

/// Initialize each module from its stamped document or injected baseline.
/// A detected newer schema stops the run before any baseline state is flushed.
pub fn ensure_baselines<C: 'static>(
    modules: impl IntoIterator<Item = &'static dyn ModuleMigrator<C>>,
    ctx: &C,
    store: &mut MigrationStore,
    state_path: &Path,
) -> anyhow::Result<()> {
    let modules: Vec<_> = modules.into_iter().collect();
    // A future build may have written a higher revision to migration-state.
    // Refuse it BEFORE reconciliations or baseline insertion can flush and
    // rewrite that state. `run_pending` itself is too late for this check:
    // callers initialize baselines while constructing the Runner.
    ensure_known_revisions(modules.iter().copied(), store, state_path)?;
    let mut changed = reconcile_documents(modules.iter().copied(), ctx, store, state_path)?;
    for module in modules {
        if !store.modules.contains_key(module.module()) {
            let baseline =
                module
                    .detect_baseline(ctx)
                    .map_err(|source| ExecutionError::DetectBaseline {
                        module: module.module().to_owned(),
                        source,
                    })?;
            changed |= store.ensure_module(module.module(), baseline);
        }
    }
    if changed {
        persist_state(store, state_path, "migration baselines")?;
    }
    Ok(())
}

/// After all steps are verified, stamp legacy unstamped documents at their
/// module head. Snapshot protection is checked before the first rewrite, and
/// no `stamped` flag is set until the actual atomic write succeeds.
pub fn stamp_adopted_documents<C: 'static, P: BackupPort>(
    modules: impl IntoIterator<Item = &'static dyn ModuleMigrator<C>>,
    ctx: &C,
    backup_ctx: &Ctx,
    store: &mut MigrationStore,
    existing: &mut Option<P::Snapshot>,
    port: &mut P,
) -> anyhow::Result<()> {
    for module in modules {
        let ModuleKind::Document(spec) = module.kind() else {
            continue;
        };
        let path = (spec.path)(ctx);
        let Some(file) = document::read_document(&path, spec.document).map_err(|source| {
            ExecutionError::InspectDocument {
                module: module.module().to_owned(),
                source,
            }
        })?
        else {
            continue;
        };
        if file.schema_revision.is_none() {
            let head = head_revision(module);
            if store.module_state(module.module()).applied_revision != head {
                continue;
            }
            ensure_backup(backup_ctx, existing, port)?;
            document::write_document(&path, spec.document, head, file.payload, None).map_err(
                |source| ExecutionError::StampDocument {
                    module: module.module().to_owned(),
                    path: path.clone(),
                    source,
                },
            )?;
        }
        store
            .modules
            .entry(module.module().to_owned())
            .or_default()
            .stamped = true;
    }
    Ok(())
}

pub fn reconcile_documents<C: 'static>(
    modules: impl IntoIterator<Item = &'static dyn ModuleMigrator<C>>,
    ctx: &C,
    store: &mut MigrationStore,
    state_path: &Path,
) -> anyhow::Result<bool> {
    let mut changed = false;
    for module in modules {
        let ModuleKind::Document(spec) = module.kind() else {
            continue;
        };
        let head = head_revision(module);
        let entry = store.modules.get(module.module()).copied();
        // Left to `ensure_known_revisions`, which explains it better.
        if entry.is_some_and(|state| state.applied_revision > head) {
            continue;
        }
        let path = (spec.path)(ctx);
        let file = document::read_document(&path, spec.document).map_err(|source| {
            ExecutionError::InspectDocument {
                module: module.module().to_owned(),
                source,
            }
        })?;
        let Some(file) = file else {
            if entry.is_none() {
                // Nothing to migrate: the file will be created at head.
                store
                    .modules
                    .insert(module.module().to_string(), at_revision(head, false));
                changed = true;
            }
            continue;
        };

        match (file.schema_revision, entry) {
            (Some(stamped), _) if stamped > head => {
                return Err(RevisionError::NewerDocument {
                    path,
                    read: stamped,
                    supported: head,
                }
                .into());
            }
            (Some(stamped), None) => {
                store
                    .modules
                    .insert(module.module().to_string(), at_revision(stamped, true));
                changed = true;
            }
            (Some(stamped), Some(state)) if stamped < state.applied_revision => {
                return Err(RevisionError::DocumentBehindState {
                    path,
                    state_path: state_path.to_path_buf(),
                    module: module.module().to_owned(),
                    read: stamped,
                    applied: state.applied_revision,
                }
                .into());
            }
            (Some(_), Some(state)) => {
                if !state.stamped {
                    store
                        .modules
                        .entry(module.module().to_owned())
                        .or_default()
                        .stamped = true;
                    changed = true;
                }
            }
            (None, Some(state))
                if state.stamped || state.applied_revision > spec.unstamped_ceiling =>
            {
                return Err(RevisionError::MissingDocumentStamp {
                    path,
                    state_path: state_path.to_path_buf(),
                }
                .into());
            }
            // Written before stamps: `detect_baseline` and `files_behind`
            // still judge it, and `stamp_adopted_documents` stamps it once
            // it is verified at head.
            (None, _) => {}
        }
    }
    Ok(changed)
}

pub fn ensure_known_revisions<C: 'static>(
    modules: impl IntoIterator<Item = &'static dyn ModuleMigrator<C>>,
    store: &MigrationStore,
    state_path: &Path,
) -> anyhow::Result<()> {
    for module in modules {
        let applied = store.module_state(module.module()).applied_revision;
        let head = head_revision(module);
        if applied > head {
            return Err(RevisionError::NewerAppliedModule {
                path: state_path.to_path_buf(),
                module: module.module().to_owned(),
                read: applied,
                supported: head,
            }
            .into());
        }
    }
    Ok(())
}

pub fn verify_applied_revisions<C: 'static>(
    modules: impl IntoIterator<Item = &'static dyn ModuleMigrator<C>>,
    ctx: &C,
    store: &MigrationStore,
    state_path: &Path,
) -> anyhow::Result<()> {
    for module in modules {
        let applied = store.module_state(module.module()).applied_revision;
        let behind =
            module
                .files_behind(ctx, applied)
                .map_err(|source| ExecutionError::InspectModule {
                    module: module.module().to_owned(),
                    source,
                })?;
        if let Some(reason) = behind {
            return Err(RevisionError::FilesBehindState {
                state_path: state_path.to_path_buf(),
                module: module.module().to_owned(),
                applied,
                reason,
            }
            .into());
        }
    }
    Ok(())
}

fn head_revision<C: 'static>(module: &dyn ModuleMigrator<C>) -> u64 {
    module.steps().last().map_or(0, |step| step.revision())
}

fn at_revision(revision: u64, stamped: bool) -> ModuleState {
    ModuleState {
        applied_revision: revision,
        baseline_revision: revision,
        stamped,
    }
}

/// Shared top-level Runner orchestration. This owns the durable order of
/// preflight safety checks, backup, independent module execution, revision
/// verification, legacy stamp adoption, and final success state commit.
///
/// The host injects the concrete platform/verified backup port and retains
/// responsibility for pruning old snapshots ONLY after this returns `Ok`.
pub fn run_pending<C: AsRef<Ctx> + 'static, P: BackupPort>(
    modules: &[&'static dyn ModuleMigrator<C>],
    registry: &Registry<C>,
    ctx: &mut C,
    store: &mut MigrationStore,
    snapshot: &mut Option<P::Snapshot>,
    backup_port: &mut P,
    target: &Version,
    force: bool,
) -> anyhow::Result<()> {
    let state_path = ctx.as_ref().state_path();
    ensure_known_revisions(modules.iter().copied(), store, &state_path)?;
    if has_pending_steps(modules, store, target, force) {
        ensure_backup(ctx.as_ref(), snapshot, backup_port)?;
    }
    execute_pending_steps(modules, ctx, store, &state_path, registry, target, force)?;
    verify_applied_revisions(modules.iter().copied(), ctx, store, &state_path)?;
    stamp_adopted_documents(
        modules.iter().copied(),
        ctx,
        ctx.as_ref(),
        store,
        snapshot,
        backup_port,
    )?;
    commit_successful_run(store, target, &state_path)
}

/// Execute an explicitly selected step while preserving the upstream Runner's
/// separate command semantics: record the step but do not claim all modules
/// have migrated, or prune historical backups.
pub fn run_selected_migration<C: AsRef<Ctx> + 'static, P: BackupPort>(
    step: &dyn MigrationStep<C>,
    modules: &[&'static dyn ModuleMigrator<C>],
    registry: &Registry<C>,
    ctx: &mut C,
    store: &mut MigrationStore,
    snapshot: &mut Option<P::Snapshot>,
    backup_port: &mut P,
    target: &Version,
    force: bool,
    enabled: bool,
) -> anyhow::Result<()> {
    println!("Running migration: {} ({})", step.id(), step.name());
    let state_path = ctx.as_ref().state_path();
    ensure_known_revisions(modules.iter().copied(), store, &state_path)?;
    let advice = advice_step(store, spec(step), target, force, enabled);
    println!("Advice: {advice:?}");
    if matches!(advice, MigrationAdvice::Ignored | MigrationAdvice::Done) {
        return Ok(());
    }
    ensure_backup(ctx.as_ref(), snapshot, backup_port)?;
    execute_step(step, ctx, store, &state_path, registry)
}

/// Commit a fully verified migration run only after all steps, stamps and
/// file-state checks succeed. The final version and task state share one
/// crash-safe flush, and no backup is pruned until that flush returns success.
pub fn commit_successful_run(
    store: &mut MigrationStore,
    target: &Version,
    state_path: &Path,
) -> anyhow::Result<()> {
    // This app-wide marker is the final transaction commit: never retain a
    // false success in memory if the atomic persistence operation fails.
    let previous = store.app.last_succeeded.replace(target.clone());
    if let Err(error) = persist_state(store, state_path, "successful migration state") {
        store.app.last_succeeded = previous;
        return Err(error);
    }
    Ok(())
}

/// Decide preflight backup requirements before any step may rewrite data.
pub fn has_pending_steps<C: 'static>(
    modules: &[&'static dyn ModuleMigrator<C>],
    store: &MigrationStore,
    target: &Version,
    force: bool,
) -> bool {
    modules.iter().any(|module| {
        module.steps().iter().any(|step| {
            advice_step(store, spec(*step), target, force, true) == MigrationAdvice::Pending
        })
    })
}

/// Execute all pending modules in registry order. A failed step stops its
/// module but does not suppress independent modules; the first failure is
/// returned *after* attempting them so the next startup can retry safely.
/// Callers must finish the verified preflight snapshot before entering here.
pub fn execute_pending_steps<C: 'static>(
    modules: &[&'static dyn ModuleMigrator<C>],
    ctx: &mut C,
    store: &mut MigrationStore,
    state_path: &Path,
    registry: &Registry<C>,
    target: &Version,
    force: bool,
) -> anyhow::Result<()> {
    let mut first_error = None;
    for module in modules {
        for step in module.steps() {
            let advice = advice_step(store, spec(*step), target, force, true);
            println!(
                "[{advice}] {} rev{} {}",
                step.module(),
                step.revision(),
                step.id()
            );
            if advice != MigrationAdvice::Pending {
                continue;
            }
            if let Err(error) = execute_step(*step, ctx, store, state_path, registry) {
                eprintln!(
                    "Migration {} failed; stopping module {} and continuing",
                    step.id(),
                    step.module()
                );
                if first_error.is_none() {
                    first_error = Some(error);
                }
                break;
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// A migration step transaction, shared with the reference Runner.
///
/// Record `in_progress` before invoking the step, verify the document and
/// step-specific postconditions, and only then publish completed/skipped state.
/// Failures are persisted for restart and best-effort rollback cannot hide
/// the original error. The migration preflight backup is owned by the caller.
pub fn execute_step<C: 'static>(
    step: &dyn MigrationStep<C>,
    ctx: &mut C,
    store: &mut MigrationStore,
    state_path: &Path,
    registry: &Registry<C>,
) -> anyhow::Result<()> {
    match check_step(step, ctx, registry) {
        Ok(Some(StepCheck::Satisfied)) => {
            println!("Migration {} is already satisfied, skipping.", step.id());
            return finish_step(step, ctx, store, state_path, registry, true);
        }
        Ok(Some(StepCheck::Needed) | None) => {}
        Err(error) => {
            let error: anyhow::Error = ExecutionError::CheckStep {
                step: step.id().to_owned(),
                source: error,
            }
            .into();
            eprintln!("Migration {} failed: {error:#}", step.id());
            return Err(record_failure(step, store, state_path, error));
        }
    }

    store.mark_in_progress(spec(step));
    persist_state(
        store,
        state_path,
        format!("{} in-progress state", step.id()),
    )?;

    match step
        .run(ctx)
        .map_err(|source| {
            ExecutionError::RunStep {
                step: step.id().to_owned(),
                source,
            }
            .into()
        })
        .and_then(|()| ensure_satisfied(step, ctx, registry))
    {
        Ok(()) => {
            println!("Migration {} completed.", step.id());
            finish_step(step, ctx, store, state_path, registry, false)
        }
        Err(error) => {
            eprintln!("Migration {} failed: {error:#}", step.id());
            if let Err(rollback_error) = step.rollback(ctx) {
                eprintln!(
                    "Migration {} rollback failed: {rollback_error:#}",
                    step.id()
                );
            }
            Err(record_failure(step, store, state_path, error))
        }
    }
}

fn spec<C: 'static>(step: &dyn MigrationStep<C>) -> StepSpec<'static> {
    StepSpec {
        id: step.id(),
        module: step.module(),
        revision: step.revision(),
        introduced_in: step.introduced_in(),
    }
}

fn finish_step<C: 'static>(
    step: &dyn MigrationStep<C>,
    ctx: &C,
    store: &mut MigrationStore,
    state_path: &Path,
    registry: &Registry<C>,
    skipped: bool,
) -> anyhow::Result<()> {
    // Record the presence of a stamp in the same flush as completion. A later
    // lost stamp must be rejected even if the rest of the run never finishes.
    if let Some(spec) = document_spec(registry, step.module())
        && document::read_document(&(spec.path)(ctx), spec.document)?
            .is_some_and(|file| file.schema_revision.is_some())
    {
        store
            .modules
            .entry(step.module().to_owned())
            .or_default()
            .stamped = true;
    }
    if skipped {
        store.mark_skipped(spec(step));
    } else {
        store.mark_completed(spec(step));
    }
    store.bump_module(step.module());
    persist_state(store, state_path, format!("{} finished state", step.id()))
}

fn record_failure<C: 'static>(
    step: &dyn MigrationStep<C>,
    store: &mut MigrationStore,
    state_path: &Path,
    error: anyhow::Error,
) -> anyhow::Error {
    store.mark_failed(spec(step), &error);
    match persist_state(store, state_path, format!("{} failed state", step.id())) {
        Ok(()) => error,
        Err(persistence) => ExecutionError::RecordFailure {
            step: step.id().to_owned(),
            source: error,
            persistence,
        }
        .into(),
    }
}

/// A successful run must satisfy its check and, for document modules, must
/// leave a stamped document, otherwise its completion cannot be persisted.
fn ensure_satisfied<C: 'static>(
    step: &dyn MigrationStep<C>,
    ctx: &C,
    registry: &Registry<C>,
) -> anyhow::Result<()> {
    if let Some(spec) = document_spec(registry, step.module()) {
        let file = document::read_document(&(spec.path)(ctx), spec.document).map_err(|source| {
            ExecutionError::CheckStep {
                step: step.id().to_owned(),
                source,
            }
        })?;
        if file.is_none_or(|file| file.schema_revision.is_none()) {
            return Err(ExecutionError::VerifyDocumentStamp {
                step: step.id().to_owned(),
            }
            .into());
        }
    }
    match check_step(step, ctx, registry) {
        Ok(Some(StepCheck::Needed)) => Err(ExecutionError::VerifyStep {
            step: step.id().to_owned(),
        }
        .into()),
        Ok(Some(StepCheck::Satisfied) | None) => Ok(()),
        Err(source) => Err(ExecutionError::CheckStep {
            step: step.id().to_owned(),
            source,
        }
        .into()),
    }
}

fn check_step<C: 'static>(
    step: &dyn MigrationStep<C>,
    ctx: &C,
    registry: &Registry<C>,
) -> Result<Option<StepCheck>, MigrationCheckError> {
    if let Some(spec) = document_spec(registry, step.module()) {
        match document::read_document(&(spec.path)(ctx), spec.document)? {
            None => return Ok(Some(StepCheck::Satisfied)),
            Some(file) => {
                if let Some(stamped) = file.schema_revision {
                    return Ok(Some(StepCheck::from_needed(stamped < step.revision())));
                }
            }
        }
    }
    step.check(ctx)
}

fn document_spec<C: 'static>(registry: &Registry<C>, module: &str) -> Option<DocumentSpec<C>> {
    registry
        .modules()
        .find(|candidate| candidate.module() == module)
        .and_then(|candidate| match candidate.kind() {
            ModuleKind::Document(spec) => Some(spec),
            ModuleKind::Heuristic => None,
        })
}

pub fn advice_step(
    store: &MigrationStore,
    step: StepSpec<'_>,
    target: &Version,
    force: bool,
    enabled: bool,
) -> MigrationAdvice {
    if !enabled {
        return MigrationAdvice::Ignored;
    }
    if force {
        return MigrationAdvice::Pending;
    }
    match store.task_state(step.id) {
        Some(MigrationState::Completed | MigrationState::Skipped) => return MigrationAdvice::Done,
        Some(MigrationState::Failed | MigrationState::InProgress | MigrationState::NotStarted) => {
            return MigrationAdvice::Pending;
        }
        None => {}
    }
    if step.revision > store.module_state(step.module).applied_revision
        && introduced_in_reached(step.introduced_in, target)
    {
        MigrationAdvice::Pending
    } else {
        MigrationAdvice::Ignored
    }
}

/// Compare the release triple so an rc build executes migrations for the same release.
pub fn introduced_in_reached(introduced_in: &Version, target: &Version) -> bool {
    (
        introduced_in.major,
        introduced_in.minor,
        introduced_in.patch,
    ) <= (target.major, target.minor, target.patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::context::MigrationPaths;
    use chimera_contracts::{
        BoxedCause,
        storage::{HotkeyStore, HotkeyStoreOpener},
    };
    use std::{path::Path, sync::Arc};

    #[derive(Debug)]
    struct NeverOpenStore;
    impl HotkeyStoreOpener for NeverOpenStore {
        fn open(&self, _path: &Path) -> Result<Box<dyn HotkeyStore>, BoxedCause> {
            unreachable!("backup detection must never open the database")
        }
    }

    fn context(root: &Path) -> Ctx {
        Ctx::new(
            MigrationPaths::new(root.join("cfg"), root.join("data")),
            Arc::new(NeverOpenStore),
        )
    }

    #[derive(Default)]
    struct RecordingBackup {
        calls: usize,
        fail: bool,
    }
    impl BackupPort for RecordingBackup {
        type Snapshot = usize;
        fn create_snapshot(&mut self, _ctx: &Ctx) -> anyhow::Result<usize> {
            self.calls += 1;
            if self.fail {
                anyhow::bail!("injected snapshot failure")
            }
            Ok(self.calls)
        }
    }

    #[test]
    fn fresh_roots_and_baseline_file_need_no_snapshot_or_directory_creation() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert!(snapshot.is_none());
        assert_eq!(port.calls, 0);
        assert!(!ctx.paths().app_config_dir().exists());
        assert!(!ctx.paths().app_data_dir().exists());

        fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
        fs::write(ctx.state_path(), b"store_schema_version: 2\n").unwrap();
        assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(port.calls, 0);
        assert!(snapshot.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn successful_commit_flush_failure_keeps_old_version_in_memory_and_external_file_untouched() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("migration-state.yaml");
        let target = temp.path().join("external.yaml");
        fs::write(&target, b"user state must remain untouched").unwrap();
        symlink(&target, &path).unwrap();
        let previous = Version::new(0, 27, 0);
        let mut store = MigrationStore::default();
        store.app.last_succeeded = Some(previous.clone());
        let error = commit_successful_run(&mut store, &Version::new(0, 28, 3), &path).unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::PersistState { operation, path: actual, source })
                if operation == "successful migration state" && *actual == path
                    && format!("{source:#}").contains("symbolic link")));
        assert_eq!(store.app.last_succeeded.as_ref(), Some(&previous));
        assert_eq!(
            fs::read(&target).unwrap(),
            b"user state must remain untouched"
        );
        assert_eq!(fs::read_link(&path).unwrap(), target);
    }

    #[cfg(unix)]
    #[test]
    fn unstamped_document_write_refusal_returns_typed_error_and_keeps_verified_snapshot() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
        let document_path = ctx.profiles_path();
        let external = root.path().join("external-profiles.yaml");
        let original = b"items: []\n";
        fs::write(&external, original).unwrap();
        symlink(&external, &document_path).unwrap();
        let mut store = MigrationStore::default();
        store.modules.insert(
            "profiles".to_owned(),
            ModuleState {
                applied_revision: 4,
                baseline_revision: 4,
                stamped: false,
            },
        );
        let modules = [&crate::modules::profiles::MIGRATOR as &dyn ModuleMigrator<Ctx>];
        let mut snapshot = None;
        let mut port = RecordingBackup {
            fail: true,
            ..Default::default()
        };
        let error =
            stamp_adopted_documents(modules, &ctx, &ctx, &mut store, &mut snapshot, &mut port)
                .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::CreateBackup { .. })
        ));
        assert!(snapshot.is_none());
        assert!(!store.module_state("profiles").stamped);
        assert_eq!(fs::read(&external).unwrap(), original);

        port.fail = false;
        let error =
            stamp_adopted_documents(modules, &ctx, &ctx, &mut store, &mut snapshot, &mut port)
                .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::StampDocument { module, path, source })
                if module == "profiles" && *path == document_path
                    && format!("{source:#}").contains("symbolic link")));
        assert_eq!(
            snapshot,
            Some(2),
            "verified pre-write snapshot must be retained"
        );
        assert!(!store.module_state("profiles").stamped);
        assert_eq!(fs::read(&external).unwrap(), original);
        assert_eq!(fs::read_link(&document_path).unwrap(), external);
    }

    #[test]
    fn config_or_storage_requires_one_snapshot_before_rewrites() {
        for source in ["config", "storage"] {
            let root = tempfile::tempdir().unwrap();
            let ctx = context(root.path());
            if source == "config" {
                fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
                fs::write(ctx.chimera_config_path(), b"user settings").unwrap();
            } else {
                fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
                fs::write(ctx.storage_path(), b"user storage").unwrap();
            }
            let mut snapshot = None;
            let mut port = RecordingBackup::default();
            assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
            assert_eq!(snapshot, Some(1));
            assert!(!ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
            assert_eq!(port.calls, 1);
        }
    }

    #[test]
    fn failed_snapshot_does_not_mark_protection_complete() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_config_dir()).unwrap();
        let old = b"hotkeys: [Control+Q]";
        fs::write(ctx.chimera_config_path(), old).unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup {
            fail: true,
            ..Default::default()
        };
        let error = ensure_backup(&ctx, &mut snapshot, &mut port).unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::CreateBackup { source })
                if format!("{source:#}").contains("injected snapshot failure")));
        assert!(format!("{error:#}").contains("injected snapshot failure"));
        assert!(snapshot.is_none());
        assert_eq!(fs::read(ctx.chimera_config_path()).unwrap(), old);
        port.fail = false;
        assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(snapshot, Some(2));
        assert_eq!(port.calls, 2);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_storage_link_still_requires_backup() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
        let missing = root.path().join("not-created.redb");
        symlink(&missing, ctx.storage_path()).unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        assert!(ensure_backup(&ctx, &mut snapshot, &mut port).unwrap());
        assert_eq!(port.calls, 1);
        assert_eq!(fs::read_link(ctx.storage_path()).unwrap(), missing);
    }

    #[cfg(unix)]
    #[test]
    fn inaccessible_configuration_ancestor_blocks_without_backup_callback() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        fs::create_dir_all(ctx.paths().app_data_dir()).unwrap();
        let not_directory = ctx.paths().app_config_dir().to_path_buf();
        fs::write(&not_directory, b"do not delete").unwrap();
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        let error = ensure_backup(&ctx, &mut snapshot, &mut port).unwrap_err();
        assert!(format!("{error:#}").contains("failed to list"));
        assert!(snapshot.is_none());
        assert_eq!(port.calls, 0);
        assert_eq!(fs::read(&not_directory).unwrap(), b"do not delete");
    }

    #[test]
    fn prerelease_matches_release_but_not_future_release() {
        assert!(introduced_in_reached(
            &Version::parse("0.28.3").unwrap(),
            &Version::parse("0.28.3-rc.1").unwrap()
        ));
        assert!(!introduced_in_reached(
            &Version::parse("0.28.4").unwrap(),
            &Version::parse("0.28.3").unwrap()
        ));
    }

    #[test]
    fn all_durable_step_states_follow_runner_retry_policy() {
        use crate::store::TaskRecord;
        let mut store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        for (state, expected) in [
            (MigrationState::NotStarted, MigrationAdvice::Pending),
            (MigrationState::InProgress, MigrationAdvice::Pending),
            (MigrationState::Failed, MigrationAdvice::Pending),
            (MigrationState::Skipped, MigrationAdvice::Done),
            (MigrationState::Completed, MigrationAdvice::Done),
        ] {
            store.tasks.insert(
                step.id.into(),
                TaskRecord {
                    module: step.module.into(),
                    revision: step.revision,
                    introduced_in: version.clone(),
                    state,
                    started_at: None,
                    finished_at: None,
                    error: None,
                },
            );
            assert_eq!(advice_step(&store, step, &version, false, true), expected);
            assert_eq!(
                advice_step(&store, step, &version, true, true),
                MigrationAdvice::Pending
            );
            assert_eq!(
                advice_step(&store, step, &version, true, false),
                MigrationAdvice::Ignored
            );
        }
    }

    #[test]
    fn a_future_step_is_ignored_until_target_release() {
        let store = MigrationStore::default();
        let introduced = Version::new(0, 29, 0);
        let older = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "future/step",
            module: "future",
            revision: 1,
            introduced_in: &introduced,
        };
        assert_eq!(
            advice_step(&store, step, &older, false, true),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &introduced, false, true),
            MigrationAdvice::Pending
        );
    }

    #[test]
    fn disabled_steps_and_completed_steps_are_not_rerun_unless_forced() {
        let store = MigrationStore::default();
        let version = Version::new(0, 28, 3);
        let step = StepSpec {
            id: "storage/hotkeys_to_typed_config",
            module: "storage",
            revision: 2,
            introduced_in: &version,
        };
        assert_eq!(
            advice_step(&store, step, &version, false, false),
            MigrationAdvice::Ignored
        );
        assert_eq!(
            advice_step(&store, step, &version, false, true),
            MigrationAdvice::Pending
        );
        assert_eq!(
            advice_step(&store, step, &version, true, true),
            MigrationAdvice::Pending
        );
    }

    #[derive(Default)]
    struct TransactionCtx {
        shared: Option<Ctx>,
        completed: bool,
        fail_check: bool,
        fail_run: bool,
        fail_rollback: bool,
        leave_check_needed: bool,
        runs: u32,
        rollbacks: u32,
        other_runs: u32,
        other_completed: bool,
    }

    impl AsRef<Ctx> for TransactionCtx {
        fn as_ref(&self) -> &Ctx {
            self.shared
                .as_ref()
                .expect("shared context must be injected")
        }
    }

    struct TransactionStep;
    static TRANSACTION_STEP: TransactionStep = TransactionStep;
    static TRANSACTION_VERSION: Version = Version::new(0, 28, 3);

    impl MigrationStep<TransactionCtx> for TransactionStep {
        fn id(&self) -> &'static str {
            "example/commit"
        }
        fn module(&self) -> &'static str {
            "example"
        }
        fn revision(&self) -> u64 {
            1
        }
        fn introduced_in(&self) -> &'static Version {
            &TRANSACTION_VERSION
        }
        fn name(&self) -> &'static str {
            "TransactionStep"
        }
        fn check(&self, ctx: &TransactionCtx) -> Result<Option<StepCheck>, MigrationCheckError> {
            if ctx.fail_check {
                return Err(MigrationCheckError::Unrecognized(
                    "injected check failure".into(),
                ));
            }
            Ok(Some(StepCheck::from_needed(!ctx.completed)))
        }
        fn run(&self, ctx: &mut TransactionCtx) -> anyhow::Result<()> {
            ctx.runs += 1;
            if ctx.fail_run {
                anyhow::bail!("injected run failure")
            }
            if !ctx.leave_check_needed {
                ctx.completed = true;
            }
            Ok(())
        }
        fn rollback(&self, ctx: &mut TransactionCtx) -> anyhow::Result<()> {
            ctx.rollbacks += 1;
            if ctx.fail_rollback {
                anyhow::bail!("injected rollback failure")
            }
            Ok(())
        }
    }

    struct TransactionModule;
    static TRANSACTION_MODULE: TransactionModule = TransactionModule;
    static TRANSACTION_STEPS: [&dyn MigrationStep<TransactionCtx>; 1] = [&TRANSACTION_STEP];
    impl ModuleMigrator<TransactionCtx> for TransactionModule {
        fn module(&self) -> &'static str {
            "example"
        }
        fn detect_baseline(&self, _: &TransactionCtx) -> anyhow::Result<u64> {
            Ok(0)
        }
        fn steps(&self) -> &'static [&'static dyn MigrationStep<TransactionCtx>] {
            &TRANSACTION_STEPS
        }
    }

    fn transaction_registry() -> Registry<TransactionCtx> {
        Registry::new(vec![&TRANSACTION_MODULE]).unwrap()
    }

    struct InconsistentFilesModule;
    static INCONSISTENT_FILES: InconsistentFilesModule = InconsistentFilesModule;
    impl ModuleMigrator<TransactionCtx> for InconsistentFilesModule {
        fn module(&self) -> &'static str {
            "example"
        }
        fn detect_baseline(&self, _: &TransactionCtx) -> anyhow::Result<u64> {
            Ok(0)
        }
        fn steps(&self) -> &'static [&'static dyn MigrationStep<TransactionCtx>] {
            &TRANSACTION_STEPS
        }
        fn files_behind(&self, _: &TransactionCtx, applied: u64) -> anyhow::Result<Option<String>> {
            Ok((applied != 0).then(|| "the expected example document is absent".to_owned()))
        }
    }

    #[test]
    fn files_behind_migration_state_returns_structured_integrity_error() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let mut store = MigrationStore::default();
        store.modules.insert(
            "example".to_owned(),
            ModuleState {
                applied_revision: 1,
                baseline_revision: 1,
                stamped: false,
            },
        );
        store.flush_atomic(&state_path).unwrap();
        let before = std::fs::read(&state_path).unwrap();
        let error = verify_applied_revisions(
            [&INCONSISTENT_FILES as &dyn ModuleMigrator<TransactionCtx>],
            &TransactionCtx::default(),
            &store,
            &state_path,
        )
        .unwrap_err();
        assert!(matches!(error.downcast_ref::<RevisionError>(),
            Some(RevisionError::FilesBehindState {
                state_path: path, module, applied: 1, reason,
            }) if *path == state_path && module == "example" && reason.contains("expected example document")));
        assert_eq!(std::fs::read(&state_path).unwrap(), before);
    }

    struct OtherTransactionStep;
    static OTHER_STEP: OtherTransactionStep = OtherTransactionStep;
    static OTHER_STEPS: [&dyn MigrationStep<TransactionCtx>; 1] = [&OTHER_STEP];
    struct OtherTransactionModule;
    static OTHER_MODULE: OtherTransactionModule = OtherTransactionModule;

    impl MigrationStep<TransactionCtx> for OtherTransactionStep {
        fn id(&self) -> &'static str {
            "other/independent"
        }
        fn module(&self) -> &'static str {
            "other"
        }
        fn revision(&self) -> u64 {
            1
        }
        fn introduced_in(&self) -> &'static Version {
            &TRANSACTION_VERSION
        }
        fn name(&self) -> &'static str {
            "OtherTransactionStep"
        }
        fn check(&self, ctx: &TransactionCtx) -> Result<Option<StepCheck>, MigrationCheckError> {
            Ok(Some(StepCheck::from_needed(!ctx.other_completed)))
        }
        fn run(&self, ctx: &mut TransactionCtx) -> anyhow::Result<()> {
            ctx.other_runs += 1;
            ctx.other_completed = true;
            Ok(())
        }
    }

    impl ModuleMigrator<TransactionCtx> for OtherTransactionModule {
        fn module(&self) -> &'static str {
            "other"
        }
        fn detect_baseline(&self, _: &TransactionCtx) -> anyhow::Result<u64> {
            Ok(0)
        }
        fn steps(&self) -> &'static [&'static dyn MigrationStep<TransactionCtx>] {
            &OTHER_STEPS
        }
    }

    #[test]
    fn baseline_refuses_newer_module_before_persisting_uninitialized_modules() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let mut store = MigrationStore::default();
        store.modules.insert(
            "example".into(),
            ModuleState {
                applied_revision: 9,
                baseline_revision: 9,
                stamped: false,
            },
        );
        store.flush_atomic(&state_path).unwrap();
        let before = fs::read(&state_path).unwrap();
        let modules: Vec<&'static dyn ModuleMigrator<TransactionCtx>> =
            vec![&OTHER_MODULE, &TRANSACTION_MODULE];
        let error = ensure_baselines(modules, &TransactionCtx::default(), &mut store, &state_path)
            .expect_err("future state must stop baseline initialization");
        assert!(matches!(error.downcast_ref::<RevisionError>(),
            Some(RevisionError::NewerAppliedModule { module, read: 9, supported: 1, .. })
                if module == "example"));
        assert!(!store.modules.contains_key("other"));
        assert_eq!(fs::read(state_path).unwrap(), before);
    }

    #[test]
    fn shared_runner_backup_failure_blocks_steps_then_success_commits_once() {
        let dir = tempfile::tempdir().unwrap();
        let shared = context(dir.path());
        fs::create_dir_all(shared.paths().app_config_dir()).unwrap();
        fs::write(shared.chimera_config_path(), b"legacy configuration").unwrap();
        let state_path = shared.state_path();
        let mut ctx = TransactionCtx {
            shared: Some(shared),
            ..Default::default()
        };
        let registry = transaction_registry();
        let modules = registry.modules().collect::<Vec<_>>();
        let mut store = MigrationStore::default();
        let mut snapshot = None;
        let mut port = RecordingBackup {
            fail: true,
            ..Default::default()
        };
        let target = TRANSACTION_VERSION.clone();

        let error = run_pending(
            &modules,
            &registry,
            &mut ctx,
            &mut store,
            &mut snapshot,
            &mut port,
            &target,
            false,
        )
        .expect_err("failed backup must stop all steps");
        assert!(format!("{error:#}").contains("injected snapshot failure"));
        assert_eq!(ctx.runs, 0);
        assert_eq!(ctx.rollbacks, 0);
        assert!(store.app.last_succeeded.is_none());
        assert!(!state_path.exists());
        assert_eq!(
            fs::read(ctx.as_ref().chimera_config_path()).unwrap(),
            b"legacy configuration"
        );
        assert!(snapshot.is_none());

        port.fail = false;
        run_pending(
            &modules,
            &registry,
            &mut ctx,
            &mut store,
            &mut snapshot,
            &mut port,
            &target,
            false,
        )
        .unwrap();
        assert_eq!(ctx.runs, 1);
        assert_eq!(store.app.last_succeeded.as_ref(), Some(&target));
        assert_eq!(MigrationStore::load(&state_path).unwrap(), store);
        assert_eq!(snapshot, Some(2));
        assert_eq!(port.calls, 2);

        // A new Runner on a healthy restart must not create another snapshot.
        let mut fresh_snapshot = None;
        run_pending(
            &modules,
            &registry,
            &mut ctx,
            &mut store,
            &mut fresh_snapshot,
            &mut port,
            &target,
            false,
        )
        .unwrap();
        assert_eq!(ctx.runs, 1);
        assert!(fresh_snapshot.is_none());
        assert_eq!(port.calls, 2);
    }

    #[test]
    fn explicitly_selected_step_does_not_mark_the_whole_application_upgraded() {
        let dir = tempfile::tempdir().unwrap();
        let shared = context(dir.path());
        fs::create_dir_all(shared.paths().app_config_dir()).unwrap();
        fs::write(shared.chimera_config_path(), b"legacy configuration").unwrap();
        let state_path = shared.state_path();
        let mut ctx = TransactionCtx {
            shared: Some(shared),
            ..Default::default()
        };
        let registry = transaction_registry();
        let modules = registry.modules().collect::<Vec<_>>();
        let mut store = MigrationStore::default();
        let mut snapshot = None;
        let mut port = RecordingBackup::default();
        run_selected_migration(
            &TRANSACTION_STEP,
            &modules,
            &registry,
            &mut ctx,
            &mut store,
            &mut snapshot,
            &mut port,
            &TRANSACTION_VERSION,
            false,
            true,
        )
        .unwrap();
        assert_eq!(ctx.runs, 1);
        assert_eq!(snapshot, Some(1));
        assert_eq!(
            store.task_state("example/commit"),
            Some(MigrationState::Completed)
        );
        assert!(store.app.last_succeeded.is_none());
        assert!(
            MigrationStore::load(&state_path)
                .unwrap()
                .app
                .last_succeeded
                .is_none()
        );
    }

    #[test]
    fn pending_loop_continues_independent_module_after_failure_and_retries_only_failed_step() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let registry = Registry::new(vec![
            &TRANSACTION_MODULE as &dyn ModuleMigrator<TransactionCtx>,
            &OTHER_MODULE,
        ])
        .unwrap();
        let modules: Vec<_> = registry.modules().collect();
        let mut store = MigrationStore::default();
        let mut ctx = TransactionCtx {
            fail_run: true,
            ..Default::default()
        };
        let target = TRANSACTION_VERSION.clone();
        assert!(has_pending_steps(&modules, &store, &target, false));
        let error = execute_pending_steps(
            &modules,
            &mut ctx,
            &mut store,
            &state_path,
            &registry,
            &target,
            false,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("injected run failure"));
        assert_eq!((ctx.runs, ctx.rollbacks, ctx.other_runs), (1, 1, 1));
        let durable = MigrationStore::load(&state_path).unwrap();
        assert_eq!(
            durable.task_state("example/commit"),
            Some(MigrationState::Failed)
        );
        assert_eq!(
            durable.task_state("other/independent"),
            Some(MigrationState::Completed)
        );
        assert_eq!(durable.module_state("example").applied_revision, 0);
        assert_eq!(durable.module_state("other").applied_revision, 1);

        ctx.fail_run = false;
        execute_pending_steps(
            &modules,
            &mut ctx,
            &mut store,
            &state_path,
            &registry,
            &target,
            false,
        )
        .unwrap();
        assert_eq!(ctx.runs, 2);
        assert_eq!(
            ctx.other_runs, 1,
            "previously completed module must not re-execute"
        );
        assert_eq!(
            MigrationStore::load(&state_path)
                .unwrap()
                .task_state("example/commit"),
            Some(MigrationState::Completed)
        );
        assert!(!has_pending_steps(&modules, &store, &target, false));
    }

    #[test]
    fn step_check_run_and_postcondition_failures_are_typed_and_preserve_causes() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let registry = transaction_registry();
        let mut store = MigrationStore::default();
        let mut ctx = TransactionCtx {
            fail_check: true,
            ..Default::default()
        };
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &state_path,
            &registry,
        )
        .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::CheckStep { step, source: MigrationCheckError::Unrecognized(reason) })
                if step == "example/commit" && reason == "injected check failure"));
        assert_eq!(ctx.runs, 0);
        assert_eq!(
            store.task_state("example/commit"),
            Some(MigrationState::Failed)
        );

        ctx.fail_check = false;
        ctx.fail_run = true;
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &state_path,
            &registry,
        )
        .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::RunStep { step, source })
                if step == "example/commit" && format!("{source:#}").contains("injected run failure")));
        assert_eq!((ctx.runs, ctx.rollbacks), (1, 1));

        ctx.fail_run = false;
        ctx.leave_check_needed = true;
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &state_path,
            &registry,
        )
        .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::VerifyStep { step }) if step == "example/commit"));
        assert_eq!((ctx.runs, ctx.rollbacks), (2, 2));
        assert_eq!(store.module_state("example").applied_revision, 0);
        assert_eq!(
            MigrationStore::load(&state_path)
                .unwrap()
                .task_state("example/commit"),
            Some(MigrationState::Failed)
        );
    }

    struct FailingBaseline;
    static FAILING_BASELINE: FailingBaseline = FailingBaseline;
    impl ModuleMigrator<TransactionCtx> for FailingBaseline {
        fn module(&self) -> &'static str {
            "example"
        }
        fn detect_baseline(&self, _: &TransactionCtx) -> anyhow::Result<u64> {
            anyhow::bail!("injected baseline detection failure")
        }
        fn steps(&self) -> &'static [&'static dyn MigrationStep<TransactionCtx>] {
            &TRANSACTION_STEPS
        }
        fn files_behind(&self, _: &TransactionCtx, _: u64) -> anyhow::Result<Option<String>> {
            anyhow::bail!("injected module inspection failure")
        }
    }

    #[test]
    fn baseline_and_module_inspection_keep_original_failure_without_writing_state() {
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let mut store = MigrationStore::default();
        let modules = [&FAILING_BASELINE as &dyn ModuleMigrator<TransactionCtx>];
        let error = ensure_baselines(modules, &TransactionCtx::default(), &mut store, &state_path)
            .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::DetectBaseline { module, source })
                if module == "example" && format!("{source:#}").contains("injected baseline detection failure")));
        assert!(!state_path.exists());
        let error =
            verify_applied_revisions(modules, &TransactionCtx::default(), &store, &state_path)
                .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::InspectModule { module, source })
                if module == "example" && format!("{source:#}").contains("injected module inspection failure")));
        assert!(!state_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn failed_state_record_preserves_original_cause_and_rejected_symlink_write() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("migration-state.yaml");
        let outside = dir.path().join("outside.yaml");
        std::fs::write(&outside, b"user-owned content").unwrap();
        symlink(&outside, &state_path).unwrap();
        let mut store = MigrationStore::default();
        let original: anyhow::Error = ExecutionError::RunStep {
            step: "example/commit".into(),
            source: anyhow::anyhow!("original step failure"),
        }
        .into();
        let result = record_failure(&TRANSACTION_STEP, &mut store, &state_path, original);
        match result.downcast_ref::<ExecutionError>() {
            Some(ExecutionError::RecordFailure {
                step,
                source,
                persistence,
            }) => {
                assert_eq!(step, "example/commit");
                assert!(matches!(source.downcast_ref::<ExecutionError>(),
                    Some(ExecutionError::RunStep { step, source })
                        if step == "example/commit" && format!("{source:#}").contains("original step failure")));
                assert!(matches!(persistence.downcast_ref::<ExecutionError>(),
                    Some(ExecutionError::PersistState { operation, path, source })
                        if operation.contains("failed state") && *path == state_path
                            && format!("{source:#}").contains("symbolic link")));
                assert!(format!("{persistence:#}").contains("symbolic link"));
            }
            other => panic!("expected RecordFailure, got {other:?}"),
        }
        assert_eq!(
            store.task_state("example/commit"),
            Some(MigrationState::Failed)
        );
        assert_eq!(std::fs::read(&outside).unwrap(), b"user-owned content");
        assert_eq!(std::fs::read_link(&state_path).unwrap(), outside);
    }

    #[test]
    fn transaction_records_completed_then_skipped_and_advances_only_contiguous_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-state.yaml");
        let mut ctx = TransactionCtx::default();
        let mut store = MigrationStore::default();
        let registry = transaction_registry();
        execute_step(&TRANSACTION_STEP, &mut ctx, &mut store, &path, &registry).unwrap();
        assert_eq!(ctx.runs, 1);
        assert_eq!(ctx.rollbacks, 0);
        assert_eq!(store.module_state("example").applied_revision, 1);
        assert_eq!(
            store.task_state("example/commit"),
            Some(MigrationState::Completed)
        );
        let durable = MigrationStore::load(&path).unwrap();
        assert_eq!(durable, store);
        assert!(durable.tasks["example/commit"].started_at.is_some());
        assert!(durable.tasks["example/commit"].finished_at.is_some());

        execute_step(&TRANSACTION_STEP, &mut ctx, &mut store, &path, &registry).unwrap();
        assert_eq!(ctx.runs, 1, "satisfied step must not execute again");
        assert_eq!(
            store.task_state("example/commit"),
            Some(MigrationState::Skipped)
        );
        assert_eq!(store.module_state("example").applied_revision, 1);
        assert_eq!(MigrationStore::load(&path).unwrap(), store);
    }

    #[test]
    fn preflight_check_failure_is_durable_without_invoking_run_or_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-state.yaml");
        let mut ctx = TransactionCtx {
            fail_check: true,
            ..Default::default()
        };
        let mut store = MigrationStore::default();
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &path,
            &transaction_registry(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("injected check failure"));
        assert_eq!((ctx.runs, ctx.rollbacks), (0, 0));
        assert_eq!(store.module_state("example").applied_revision, 0);
        let durable = MigrationStore::load(&path).unwrap();
        assert_eq!(
            durable.task_state("example/commit"),
            Some(MigrationState::Failed)
        );
        assert!(
            durable.tasks["example/commit"]
                .error
                .as_deref()
                .unwrap()
                .contains("injected check failure")
        );
    }

    #[test]
    fn rollback_failure_does_not_hide_original_run_error_and_retry_can_finish() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-state.yaml");
        let mut ctx = TransactionCtx {
            fail_run: true,
            fail_rollback: true,
            ..Default::default()
        };
        let mut store = MigrationStore::default();
        let registry = transaction_registry();
        let error =
            execute_step(&TRANSACTION_STEP, &mut ctx, &mut store, &path, &registry).unwrap_err();
        assert!(format!("{error:#}").contains("injected run failure"));
        assert_eq!((ctx.runs, ctx.rollbacks), (1, 1));
        let failed = MigrationStore::load(&path).unwrap();
        assert_eq!(
            failed.task_state("example/commit"),
            Some(MigrationState::Failed)
        );
        assert!(
            failed.tasks["example/commit"]
                .error
                .as_deref()
                .unwrap()
                .contains("injected run failure")
        );
        assert!(
            !failed.tasks["example/commit"]
                .error
                .as_deref()
                .unwrap()
                .contains("injected rollback failure")
        );
        ctx.fail_run = false;
        ctx.fail_rollback = false;
        execute_step(&TRANSACTION_STEP, &mut ctx, &mut store, &path, &registry).unwrap();
        assert_eq!(ctx.runs, 2);
        assert_eq!(store.module_state("example").applied_revision, 1);
        assert_eq!(
            MigrationStore::load(&path)
                .unwrap()
                .task_state("example/commit"),
            Some(MigrationState::Completed)
        );
    }

    #[test]
    fn broken_step_postcondition_rolls_back_and_does_not_advance_revision() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("migration-state.yaml");
        let mut ctx = TransactionCtx {
            leave_check_needed: true,
            ..Default::default()
        };
        let mut store = MigrationStore::default();
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &path,
            &transaction_registry(),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("still reports it as needed"));
        assert_eq!((ctx.runs, ctx.rollbacks), (1, 1));
        assert_eq!(store.module_state("example").applied_revision, 0);
        assert_eq!(
            MigrationStore::load(&path)
                .unwrap()
                .task_state("example/commit"),
            Some(MigrationState::Failed)
        );
    }

    #[cfg(unix)]
    #[test]
    fn in_progress_flush_rejects_symlink_before_step_runs() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("external-state.yaml");
        let path = dir.path().join("migration-state.yaml");
        std::fs::write(&target, b"external state").unwrap();
        symlink(&target, &path).unwrap();
        let mut ctx = TransactionCtx::default();
        let mut store = MigrationStore::default();
        let error = execute_step(
            &TRANSACTION_STEP,
            &mut ctx,
            &mut store,
            &path,
            &transaction_registry(),
        )
        .unwrap_err();
        assert!(matches!(error.downcast_ref::<ExecutionError>(),
            Some(ExecutionError::PersistState { operation, path: actual, source })
                if operation.contains("in-progress") && *actual == path
                    && format!("{source:#}").contains("symbolic link")));
        assert!(format!("{error:#}").contains("symbolic link"));
        assert_eq!((ctx.runs, ctx.rollbacks), (0, 0));
        assert_eq!(std::fs::read(&target).unwrap(), b"external state");
        assert_eq!(std::fs::read_link(path).unwrap(), target);
    }
}
