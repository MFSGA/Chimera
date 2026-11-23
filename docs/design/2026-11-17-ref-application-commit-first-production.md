# B2d — Application config: independent check, durable commit, then Core apply

## Baseline

- Start from Chimera `master` `5c869b66fbae9b2f429a4f3b78056d5d2c74ed3a` and the pinned upstream `ref` `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Compare `ref/backend/nyanpasu-core/src/state/application.rs`, `state/mutation/ports.rs`, `client/reconciler/check.rs`, `client/reconciler/workflow.rs` with Chimera's production `ApplicationClient`, `CoreLifecycleWorkflow`, `ApplicationActor` and CoreLifecycle mailbox.
- **Scope:** switch the real runtime-affecting `patch_app_config` production path to commit-first _inside Chimera's existing unique CoreLifecycle owner_. Ref's full RuntimeGate/SourceCommitted/Snapshot-status and single RuntimeReconciler mailbox are **not** installed by this batch. Do not run an independent second lifecycle actor.

## Changes

1. `CoreLifecycleWorkflow::patch_application_config` no longer calls `reconcile_application(candidate)` before source persistence. It constructs the typed candidate and legacy bridge in memory, executes an **independent advisory check** using the existing `CoreLifecyclePort::validate_profile_runtime`, then conditionally commits the prepared typed source with its expected StateEngine version. Only after a successful source commit does it attempt the Core reconcile. The advisory check reads the committed Profiles document and managed-file inputs under the shared `ProfileContentReader`, freezes the inputs, and checks the proposed Application candidate; it cannot start, stop, hand off or reconfigure the running Core. Unsupported/unavailable advisory checks do not count as success. Manager-only test fixtures without typed Profiles keep their compatibility source path until the full Reconciler migration.
2. A source CAS conflict or disk persistence error now ends the operation **without a Core apply or compensating stop**. The next invocation can observe the newer source rather than accidentally applying a stale candidate. Failure after the source commit cannot roll back that source: the method returns a `Degradation` with `phase=RuntimeApply`, `code=application_runtime_deferred`, a readable reason, and a retryability decision based on the existing typed Core failure classifier.
3. Postcommit publication cannot mistake a superseded version for convergence. After the Core attempt returns, the workflow reads the Application actor version again. If another independent source write moved it during the attempt, the first operation returns a retryable degradation requiring reconciliation of the **latest** committed target. It never silently declares the stale applied target current.
4. `CoreLifecycleClient::patch_application_config` carries a one-shot degradation response through its existing serialized mailbox; no new runtime executor was created. The application facade composes those degradations into `MutationOutcome::CommittedDegraded` with `RuntimeCommitStatus::Deferred`, preserves its previous legacy Verge/UI/side-effect notifications, and requests an existing coalesced `RuntimeDirty` round only when the failure is classified retryable.
5. The CoreLifecycle actor no longer marks an Application command as `runtime_reconciled` when its postcommit response contains degradations. A shared `runtime_converged` bit retained by the old command's completion path holds this _transitional_ distinction until the ref `ReconcilerStatus` watch is the only result/receipt authority.

## Tests and safety

- Reconcile observation now proves Application source is already committed (for Service Mode and builtin enhancement), while the explicit advisory check sees the precommit source and proposed candidate.
- A rejected independent runtime check leaves Application source/version unchanged and sends **zero Core reconcile calls**.
- After an accepted check, a deliberate Application directory write failure leaves the old version and bytes intact and sends **zero Core reconcile calls**.
- An independent concurrent Application version change during Core apply returns a retryable deferred result; it does not claim the superseded target was converged or roll back newer source changes.
- A failed postcommit Core apply returns committed-degraded rather than erroneously claiming the source patch failed.
- Full `chimera-core` and Tauri unit tests (including TypeScript Specta bindings), Rustfmt, Clippy and `git diff --check` are the release gates. GPG-sign separate coherent commits directly to `master`, fetch/rebase before a fast-forward push; do not touch `ref` or the Chimera runtime gitlinks.

## Remaining ref migration (critical)

- `select_core`, the Profiles file-first transaction participant, and portions of the Clash legacy patch path still use prior precommit or rollback semantics; this batch does not claim that the entire configuration system is commit-first.
- The production `RuntimeGate`/`CandidateCheck` port, `SourceCommitted` notifications, ref's `RuntimeBuildPort::publish`, `RuntimeReconciler` `mod/workflow/check/preparation/apply/startup/committed` and real status/watch receipt remain to migrate together. Source actors should own source commits; the eventual single Reconciler must own running Core state and all application command effects. Once that handoff is verified, remove the transitional CoreLifecycle completion bit and duplicate legacy write paths.
- Real macOS/Windows/Linux startup, Service owner handoff and TUN platform E2E still need manual/automated device validation; passing library unit tests alone is not equivalent to release readiness.
