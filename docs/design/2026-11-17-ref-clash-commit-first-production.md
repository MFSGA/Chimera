# B2f — Clash typed configuration commit-first production migration

## Baseline and upstream reference

- Main project: Chimera `master` rebased/fast-forwarded onto `e0d2aafb`; fixed upstream reference: `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Upstream reference implementation: `ref/backend/nyanpasu-core/src/state/clash_config.rs`, `state/mutation/ports.rs`, and `client/reconciler/check.rs`.
- The upstream Clash source actor calculates runtime impact, independently checks the candidate, commits with optimistic versioning, and notifies its reconciler. A failed **post-commit** runtime application is deferred; the source version is not rolled back.

## Actual production changes

1. `ClashConfigClient::patch_typed_config` and its new `TypedClashCommit` receipt implement one ordered transition: compute the patch and runtime impact; prepare the typed legacy mirror without publishing it; independently validate any runtime-affecting candidate; conditionally persist the typed Clash state using `ReplacePreparedIfVersion`; **only then** invoke the existing single CoreLifecycle to reconcile the Core.
2. The read-only validator `ChimeraClient::check_clash_runtime_candidate` uses `ProfilesClient::content_reader().committed(...)` to freeze the committed Profiles document together with the active managed-file closure. It then releases the read fence, overlays the candidate Clash document, and invokes `CoreLifecyclePort::validate_profile_runtime`. A missing or failed/unserviceable checker is not considered a successful check in production; test-only compatibility clients with no Profiles source retain a trivial checker fixture. This is an intermediate bridge to ref's independent `CandidateCheck` RuntimeGate; it does not create a second Core writer.
3. After source persistence, a rejected Core application produces a `Degradation { phase: RuntimeApply, code: "clash_runtime_deferred", ... }` instead of rolling back source state or trying to reverse a running Core operation. The retry classifier reuses the existing error semantics; retryable failures request the existing coalesced CoreLifecycle runtime rebuild.
4. After the Core operation settles, the workflow compares the current Clash source version to the version it just committed. If another source write advanced it, it reports a retryable deferred target rather than declaring an older applied target converged. A failed post-commit version read is also a degradation, not a false source-commit error.
5. A subsequent failure to persist either legacy mirror (`Config::clash()` or `Config::verge()`) produces `legacy_clash_mirror_write_failed`, preserving the authoritative typed Clash source. The application facade returns these degradations through the existing `MutationOutcome::CommittedDegraded` result and marks the per-domain `CommitReceipt` as `Deferred` or `Pending`/`Unchanged` as appropriate. This IPC shape already existed and was not changed.
6. A typed patch which does not reach runtime still commits and persists its desired source but neither invokes the candidate checker nor mutates Core. Existing macOS/Windows TUN authorization preflight, transparent proxy validation, system-proxy port guard, frontend and effects notification ordering remain in place.

## Regression checks

The live `ClashConfigClient` test harness uses a real persisted Clash actor and a controlled mirror/runtime port. The test cases verify:

- A candidate preflight sees the proposed value while the previously committed source is unchanged; the postcommit Core apply sees the newly committed value.
- A preflight rejection never commits or starts a Core operation.
- A filesystem persistence failure after successful preflight never starts Core or saves a legacy mirror.
- A version conflict caused by an independent source write during preflight cannot apply the stale candidate.
- A postcommit Core failure leaves the new Clash version on disk and records a runtime degradation.
- A legacy mirror failure leaves the already committed source in place and reports a separate degradation.
- Another source write that advances the version during runtime apply is detected and marked for reconciliation of the latest committed target.
- UI-only Clash fields do not trigger a check or running Core mutation.
- The actual `ChimeraClient::patch_clash_config` facade returns `CommittedDegraded` with a `clash_runtime_deferred` degradation and `RuntimeCommitStatus::Deferred` on injected post-commit Core failure, rather than returning an error or rolling back source.
- A facade patch that only updates UI fields persists the new value without submitting a Core rebuild.
- Full shared Core and Tauri libraries (including Specta binding freshness), Rustfmt and Clippy remain acceptance gates; the main repository must be clean after signed, fast-forward commits to `master`.

## Separate Runtime project

The user permits making corresponding Runtime changes. This batch requires **no new Runtime protocol or core change**: the existing Chimera core adapter already exposes the read-only candidate validator and confirmed runtime reconciliation path. The `backend/chimera-runtime` submodule (`54294d1fdda6f4f574a27e90374a79dd6558dcf2`) is therefore intentionally unchanged rather than carrying an unnecessary commit. If a subsequent migration requires new IPC messages or runtime receipt semantics, update, test and sign the Runtime project first, and then explicitly advance the main gitlink.

## Remaining work and limitations

- This batch migrates the **typed** `patch_clash_config` API, not the separate legacy raw mapping/override patch path (`ClashConfigClient::patch_with_overrides`) nor legacy running-Core live overrides (`RuntimePatchCoordinator`). Those still use their existing compensation policies and should be migrated with the same ref source-domain semantics rather than silently assumed compliant.
- The production source owner is still mediated by `ClashConfigClient` plus the existing unique `CoreLifecycle` execution actor. The upstream `RuntimeGate`, `SourceCommitted` actor message, central `RuntimeReconciler` and direct actor-owned source commit have **not** all been installed. This is a controlled production path migration, not a claim of full ref architecture parity.
- Profiles file transactions still have legacy participant semantics. Actual macOS/Windows/Linux app startup, Service handoff, TUN and transparent-proxy live-device tests remain necessary before a release.
