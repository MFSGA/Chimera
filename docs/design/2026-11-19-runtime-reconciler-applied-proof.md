# RuntimeReconciler: verified committed-target proof

## Fixed baselines

- Chimera before this migration: `822e04c5388934e642d644a6ae55e7082913a33a`.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`. This batch must not update its gitlink.
- Upstream reference: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs` (`CoreFact::Running { verified }`, `plan`) and `ref/backend/nyanpasu-core/src/client/reconciler/workflow.rs` (applied target/host verification).
- Chimera integration: `backend/tauri/src/client/reconciler/proof.rs`, `backend/tauri/src/client/core_lifecycle/workflow.rs::verified_applied_runtime`, `backend/tauri/src/client/core_lifecycle/mod.rs` and `backend/tauri/src/client/reconciler/status.rs`.

## Why this bridge is needed

A completed Core reconcile command and a `CoreState::Running` observation do not prove that **the intended committed configuration** was applied. The instance could have changed, another host could be selected, or a different runtime revision could have been accepted while the source actor was committing changes. The previous compatibility status could publish `Conclusion::Converged` on `Running` alone.

## Proof contract

To conclude `Converged`, the **sole** CoreLifecycle runtime actor now requires:

1. A successful reconciler command and a genuine `Running` observation.
2. A captured immutable runtime identity from the latest committed Application, Clash, Profiles and frozen managed-file contents.
3. The selected Core host's active accepted `RuntimeApplyReceipt`, validated by the underlying `CoreFacade::active_runtime_receipt` against the host's epoch, generation, source hash, effective hash and core kind.
4. That receipt must have `CoreRunIntent::Running`, an execution host matching the observed selected host and `receipt.target == committed_target`.
5. A second Core status read must match the original instance, state, host and changed-at timestamp. A second active receipt read must resolve to the **same** accepted receipt allocation. A final committed target re-read must match the original captured source target.

If any verification is unavailable or disagrees, the runtime remains `Waiting`. No proof read changes source state, applies Core commands or selects a new host. Source-only notifications still do not themselves imply convergence.

## Existing Service handoff and remaining work

The current Chimera `ApplicationWorkflow::startup_reconcile_once` already implements a Service→Local fallback after the Service Core is conditionally retired and a fresh probe proves its release. Existing tests such as `ready_service_reconcile_failure_retires_owned_core_before_local_fallback` and `startup_adoption_falls_back_when_service_reconcile_fails` cover this compatibility path. Replacing it wholesale without migrating the actor's host transition lease and the upstream `plan(Facts)` owner state would risk two separate Core owners. This batch therefore leaves that handoff intact.

Remaining upstream work: move the complete `plan(Facts)` decision and Service host fallback/maintenance scheduling into one dedicated RuntimeReconciler Actor; align Applied vs Waiting vs NotInEffect outcomes, per-target applied revision and daemon release maintenance, and expose the status DTO to UI when the RPC contract is ready. Real desktop Core/Service E2E must follow before declaring full alignment.

## Acceptance evidence

- Pure tests for exact target, selected host and run intent mapping.
- Actor-level test with a stable accepted receipt reports `Converged`; an old-target receipt or a changed Core instance reports `Waiting`.
- Full `chimera-core` and Tauri library test suites plus Rustfmt, Clippy, and `git diff --check`.
- Existing startup Service fallback and stop-intent regressions must continue passing.
- No schema, submodule, UI or IPC change. Revert the scoped commit if runtime-status regressions appear; the runtime application path itself remains unchanged.

The Rust tests use injected adapters and do **not** substitute for production Core/Service E2E on macOS, Windows or Linux.
