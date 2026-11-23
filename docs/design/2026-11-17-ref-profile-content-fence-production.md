# B2b — ProfileContentReader production fence against file/document skew

## Upstream reference and target

- Chimera `master` baseline `a47497a2`; fixed ref `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Upstream: `ref/backend/nyanpasu-core/src/state/mutation/profile_content.rs`, `state/profiles/actor.rs`, `client/profiles.rs`, `client/reconciler/preparation.rs`.
- The upstream `ProfileContentReader` uses a single read/write fence shared with the Profiles source owner: a committed document and managed file contents must come from **one** source state, not an old document combined with a just-promoted file. This is a prerequisite for the single `RuntimeReconciler` target identity and its commit-first candidate checks.

## Production wiring in this batch

- New `backend/tauri/src/state/mutation/profile_content.rs` copies the ref's `ProfileContentLock` and `ProfileContentReader` semantics. The wrapper uses the existing `chimera_core::state::StateSnapshot<Profiles>` until the Profiles owner is moved to `chimera-state-engine`; it publishes no new mutable state or RPC API.
- `ProfilesClient` retains a clone of one lock, passes it into `ProfilesActor`, and exposes a read-only `content_reader()`.
- The **actual production composition root** passes `Some(typed_profiles.content_reader())` into `CoreLifecycleClient::spawn_with_profiles_and_content`. Tests and compatibility adapters still use the original `spawn_with_profiles`, which provides no reader rather than creating another owner.
- CoreLifecycle's committed runtime capture now reads the document and active managed bytes **inside one `reader.committed(...)` call** for startup identity, application reconfiguration, reconfiguration following Service release, and deferred runtime retry. Explicit candidates use `reader.candidate(...)`, which fences file reads but does not interfere with Core validation or apply.
- The Profiles owner acquires the write fence for startup and periodic materialization journal reconciliation, cleanup activation, and cleanup retry. For an actual source file promotion, its legacy transaction acquires the write fence **inside** the `local_write` step, _after_ its participant's preliminary Core check; that fence is kept through promotion, document persistence, CAS publication and resource recovery. On a rejected preflight, staged compensation is fenced separately.
- The legacy manager can continue an in-flight source transaction in a detached task after the caller drops its future. The write fence is also retained by that task's recovery closure, rather than only by the caller: cancelling a waiter cannot prematurely expose partially written file contents to committed readers.

## Why the lock is not acquired at mutation admission

Current Chimera `ProfilesActor` still uses `replace_if_version_with_participant` and the old `ApplicationWorkflow` to validate or even attempt the candidate before persistent source commit. That check may itself capture profile content. Taking the exclusive lock at admission would make the preflight read wait on its own transaction. This patch starts the exclusive fence only when local persistence actually starts, while keeping the committed and candidate _read_ sides in place. Once the ref commit-first `RuntimeGate`, `check.rs`, and single mailbox replace the legacy mutation participant, this workaround can be simplified to ref's direct `replace_if_version_with_local_write`.

## Invariants and tests

1. File promotion and the associated Profiles YAML snapshot publication are invisible to a committed reader until both have settled. A failed local write keeps the fence until compensation settles.
2. Candidate checks can still capture existing and staged inputs before any write fence is acquired. No Core validation, Core apply, or Core state transition runs while a read fence is held.
3. Startup and deferred retry target identity are derived from a frozen document and captured content belonging to one state; a stale file cannot be paired with a newly published committed document in these paths.
4. Other app instances remain a separate source of possible file modification; this is an **in-process** ownership fence, not a cross-process file lock or external watcher guarantee.
5. New tests verify mutual exclusion, blocked candidate reads during promotion, and a real file being promoted before its state version is published. Existing Profile transaction tests and full Tauri/Core tests remain release gates. Rustfmt/Clippy and TypeScript binding freshness must pass.

## Remaining ref-alignment work

This batch does **not** claim the `RuntimeReconciler` has replaced old production writers. `select_core` and a few older explicit candidate paths may still construct their candidate snapshot before acquiring the read fence; their runtime operations are serialized by the old Workflow but will be replaced as part of the single-Reconciler migration. The remaining work is to inject ref `RuntimeGate` and the source-owned commit-first check, move Profiles to the standalone StateEngine, copy/assemble the ref `reconciler/{mod,workflow,check,preparation,apply,startup,committed,mutation,ports}.rs` and retire old `ApplicationWorkflow/CoreLifecycle` writes in the same ownership cutover. These are not safe to do by merely adding a second actor and leaving the old one live.
