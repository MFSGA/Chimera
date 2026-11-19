# RuntimeReconciler: postcommit Service refusal and guarded Local fallback

## Baseline and upstream reference

- Chimera baseline: `1bff62fe8f835d134d285ddf9143086b1aae35dd` (`master`).
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; no gitlink update.
- Authoritative upstream rules: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs` (`Step::FallBackToLocal`), `startup.rs::converge`, `apply.rs` and `committed.rs`.
- Chimera bridge: `backend/tauri/src/client/core_lifecycle/workflow.rs::reconcile_with_trigger` / `fallback_after_service_refusal`; `client/reconciler/attempt.rs`; `core_lifecycle/mod.rs::Message::Completed`.

## Behavior migrated

1. The source commit remains durable. Core application uses the latest committed Application/Clash/Profiles and managed-file bytes under the existing source-content fence, never a pre-commit Candidate.
2. When a Service-hosted runtime attempt is **refused**, the single CoreLifecycle executor first checks that its actual selected Core reports `Stopped`. It does not fall back when Core is still running, status is unreadable, a dependency error is transient, or lower-host work is uncertain. The previously refused target is still remembered per target/host.
3. Before local application, the executor acquires the nonblocking Service transition lease. It holds this lease **across** the instance-bound Service release proof, committed-target identity check, and Local application, preventing another Service transition from invalidating that proof between release and apply.
4. The existing conditional Service stop uses the CoreOwner token from an owned, current Service probe. A fresh probe must show there is no Core owned by this application. A foreign service owner is never retired. Unreadable/quarantined Service state cannot authorize Local start.
5. After release, the single production CoreFacade's existing `reconcile_after_service_release` path applies the latest committed configuration to Local. Source target identity is checked before and after. A newer committed target produces a typed supersession and schedules another latest-source round rather than recording stale success.
6. Once the Service Core is **proven released**, the failed Service-ready generation is remembered, so a further ordinary notification does not immediately re-adopt the same failing Service generation. A different readiness generation may become eligible again.
7. Lease/release failure is `ServiceFallbackDeferred` (a `Waiting` dependency, not a permanent Local refusal) and is projected as `ReconcilerStatus.maintenance`. The same single Actor clears this debt on subsequent successful runtime convergence, without clearing unrelated Service maintenance.

## Safety and regression scope

- Injected Core/Service tests assert Service refusal then conditional release then Local apply ordering.
- If the transition lease is unavailable, Local is not started and the daemon Core remains owned; the later successful lease can retry safely.
- An unproven refusal while Service Core remains Running never retires Service or starts Local.
- Waiting/committed-target-supersession classifications do not become permanent refusals.
- Existing explicit Stop intent, target/host attempt-policy, version races and startup Service fallback tests must keep passing.
- No new runtime owner, UI command, config schema, source writer, worktree or gitlink. The complete upstream `plan(Facts)` loop, automatic maintenance retry and pending RuntimeProduct publication remain later migration work.

## Verification

Run Rustfmt, complete `chimera-core` and Tauri library tests, Clippy and `git diff --check` before merging. Tests use injected ports and do **not** verify real macOS/Windows/Linux Service daemon, Core process or TUN behavior. Perform real installed-app integration testing before calling the full upstream architecture migrated.

If rollback is necessary, revert the production and regression code commit, then this design note. Preserve unrelated remotely added commits, and use non-squashed signed commits.
