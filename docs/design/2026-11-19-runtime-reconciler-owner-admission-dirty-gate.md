# RuntimeReconciler: shared S2 owner admission for deferred source work

## Baseline and reference

- Starting Chimera `master`: `f3c1af9831c79ce3536166fd8cb64d24f77b1301`. Preserve newer remote Providers/UI commits on integration.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; runtime gitlink remains `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Ref intent: `ref/backend/nyanpasu-core/src/client/reconciler/{plan.rs,workflow.rs,startup.rs}` treats S2 ownership recovery as a prerequisite to S3/S4 Core application.
- Chimera adaptation is scoped to the existing CoreLifecycle Actor. It does not create another Core writer, change public IPC, edit UI, or add a polling task.

## Two reproduced admission races

1. A source is retained while `startup_recovery_pending` is true. A subsequent `ServiceReadiness` event, or an older successful `StartupRecoveryFinished` message, used to call the source replan helper while the independent Startup worker still owned an unresolved lease. The helper checked owner-uncertain state but not the pending worker gate. An Actor test reproduced an unwanted second Core submit before the lease cleared.
2. The inverse ordering is also possible: a source notification schedules `DirtyTick`, then Startup ownership recovery acquires its lease during the 500 ms dirty window. Without a final admission check, the scheduled tick consumes `source_dirty` and submits a Core round while Startup recovery is still pending. A second Actor test reproduced that early submission.

## Unified admission barrier

`ApplicationWorkflowState::owner_transition_pending` is the shared S2 gate. It checks the asynchronous `startup_recovery_pending` lease flag **and** queued `FollowService`, `StartupReconcile`, `RecoverStartup`, and `RetryStartup` operations. Existing active-operation, shutdown, uncertain-outcome, stopped-intent, and startup-owner-proof restrictions remain in place.

- A `SourceCommitted` arriving during any pending owner transition retains `source_after_handoff`; it does not request an immediate runtime apply. The source is still durable and remains visible in `seen_sources`.
- The Service readiness follower, retained-source replan and independent Service release-owed maintenance consult the same owner-transition barrier, so no second Service lease or daemon cleanup is admitted while the Startup owner is unresolved.
- `DirtyTick` now checks the same owner barrier at dispatch time. If it is busy, `dirty` and `source_dirty` stay intact; no Core submit is queued and no timer is spun. A successful Startup recovery, settled lifecycle action, or later Service evidence re-arms **one** coalesced dirty tick once the owner gate is open.
- A failed Startup recovery completion cannot authorize resumption. Explicit Stop or shutdown still cancels pending continuation, and a later Start cannot resurrect canceled source debt.

## Test and delivery gates

- Existing integration test now injects both a Service readiness event and an out-of-order successful recovery-completion notification while the Startup gate is still pending. The test failed before the fix (Core submissions 2 instead of 1) and passed after.
- New integration test queues a real source dirty notification, uses the Actor mailbox to prove it was processed, then enables the startup-recovery gate before `DirtyTick` fires. Before the fix the Core was prematurely resubmitted; after the fix the dirty request stays dormant and completes the latest target **once** after a successful recovery notification.
- Full `chimera-core` and Tauri Rust library suites, Rustfmt, Clippy and `git diff --check` are mandatory before two signed, nonsquashed commits and a fast-forward push.

## Remaining differences

This is command-admission hardening in the existing single CoreLifecycle actor; StartupWorkflow still owns the actual leased S2 transitions. It is not the complete independent RuntimeReconciler actor or a substitute for installed desktop Core/Service/TUN/DNS E2E. No automatic repeated lease retries are introduced; failed recovery waits for a separate valid event or explicit action.
