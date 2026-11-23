# Runtime safety: preserve explicit Stop across delayed Startup ownership commands

## Baseline and scope

- Chimera baseline: `885fb90b2f71472fce37dc3500c53e2f8cea5c7b` on `master`.
- Fixed upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; fixed `backend/chimera-runtime` gitlink: `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Upstream source: `ref/backend/nyanpasu-core/src/client/reconciler/{workflow.rs,plan.rs}`, where explicit `StopCore` latches `RunIntent::Stopped` and only an explicit `Start` changes it back.
- Scope: command admission in Chimera's existing **sole** `CoreLifecycle` Actor, not a new Runtime owner or any change to Service ownership and OS IPC.

## Safety defect reproduced before the fix

An asynchronous Startup worker may acquire the Service transition lease and queue `RecoverStartup` only **after** a user-requested `StopCore` has finished. Chimera's Actor correctly latched `RunIntent::Stopped` and filtered late `ReconcileSource`, `Reconcile`, `RecoverCore` and Profile retries, but omitted `RecoverStartup`, `RetryStartup`, `StartupReconcile` and `FollowService` from that filter.

The direct Actor integration first established a verified Core, stopped it, then supplied late `RecoverStartup`. On the unmodified implementation, the Core was applied a second time, violating the explicit Stop intent (test **failed** with submission count 2 vs expected 1).

## Fix and invariant

Extend the **existing** stopped-intent gate in `ApplicationWorkflowState::drive` to skip three _automatic_ startup/owner commands — `RecoverStartup`, `RetryStartup`, and `FollowService` — before the workflow is borrowed, any Core mutation begins or a Service transition is executed. These commands settle as safe no-ops while stopped. `RunIntent::Stopped`, accepted Core receipt, inactive reconciler status and the Service/Startup ownership state are not silently changed.

The **initial one-shot `StartupReconcile`** remains distinct: an already-queued initial startup must retain its previous completion contract even if its caller timed out while `StopCore` occupied the mailbox. The existing `timed_out_startup_reconcile_has_no_terminal_completion` regression caught an overbroad first implementation; this was corrected by excluding `StartupReconcile` from the new gate. After startup is established, `StartCore` remains the explicit command to resume a stopped runtime. Existing S2 owner handoff, Service transition leases, pending product repair, source dirty coalescing and release-owed behavior are otherwise unchanged.

## Verification and limits

- The same red test turns **green** after the stopped-intent gate. It verifies no duplicate Core apply or stop for late `RecoverStartup`, `RetryStartup` and `FollowService`, an `Inactive` conclusion, and that explicit `StartCore` remains functional.
- The existing initial-startup timeout/queued-completion test must continue passing. It failed with an initial overbroad gate and passes after preserving the special first-startup semantics.
- Run complete `chimera-core` and Tauri Rust library regression suites, rustfmt, Clippy and Git diff checks before delivering signed commits.
- These are injected-ports Actor tests; no installed macOS/Windows/Linux Core, Service, TUN or DNS E2E has been completed in this batch. A command already active before Stop is serialized and finishes before the later Stop; this fix addresses commands **admitted after** Stop intent is latched. It does not claim the independent upstream RuntimeReconciler actor has been fully migrated.

Do not change `ref`, `backend/chimera-runtime`, persisted schema, UI or any unrelated branch. Revert this scoped code commit if an integration environment uncovers an incompatible startup-order assumption.
