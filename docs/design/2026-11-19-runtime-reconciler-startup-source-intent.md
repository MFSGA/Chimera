# RuntimeReconciler: retain committed sources during Startup owner recovery

## Baseline and ref behavior

- Chimera starting `master`: `373df5a2b35ff4df267ebd0ef6b177fd4f221463`. Fetch and preserve newer origin commits before delivery.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`.
- Ref sources: `ref/backend/nyanpasu-core/src/client/reconciler/{startup.rs,workflow.rs,plan.rs}`: the one reconciler samples committed inputs and defers S3/S4 while S2 ownership is recovering. A durable source cannot be converted into a source-only notification just because Startup recovery was requested.
- Chimera change is scoped to `backend/tauri/src/client/core_lifecycle/mod.rs`. The existing CoreLifecycle Actor and Service transition lease remain the only mutation/ownership executors.

## Problem and repair

Before this change, `CoreLifecycleClient::source_committed` detected `startup_recovery_needed()`, scheduled recovery, then cast `Message::SourceCommitted` with `reconcile_requested=false`. The actor therefore recorded the version only; it never retained that source's runtime-convergence intent. A successful Startup recovery could apply current inputs but did not guarantee a fresh S2/S3/S4 `SourceCommitted` round after concurrent source updates.

The source notification now carries **both** the original `reconcile_requested` intent and a separate `startup_recovery_required` observation. Its serialized Actor path latches one `source_after_handoff` continuation when either the caller requests Startup recovery or the Actor observes unproven Startup ownership. Multiple notifications coalesce to a single latest-source continuation; the Actor never replays an old Candidate or applies a runtime while its owner is unproven.

The scheduling helper clears its shared `startup_recovery_pending` gate and emits a `StartupRecoveryFinished` Actor message once the asynchronous lease/recovery attempt settles. This closes the race where recovery finishes before the original `SourceCommitted` event reaches the mailbox. The existing `replan_retained_source_on_service_change` guard rechecks Running intent, no shutdown, no uncertain lower outcome, proven Startup owner and absence of a concurrent `FollowService`. If the recovery failed or is still pending, source debt stays retained for a later evidence-driven or explicit recovery; no busy loop or speculative Core stop is introduced.

A verified successful Startup, Start or FollowService operation can also consume the pending continuation and schedule a fresh committed-source round. A verified SourceCommitted round itself consumes it without enqueuing a duplicate. Explicit `StopCore` and shutdown cancel the continuation, and later Start does not resurrect cancelled source debt.

## Regression scope

- Deterministic Actor test: two Profiles generations arrive while the Startup recovery gate is held. No Stop/Apply runs before recovery settles; the gate-completion event triggers exactly one S2/S3/S4 latest-source round at version 2. The same test proves StopCore cancels the debt.
- Real `CoreLifecycleClient::source_committed` entry test: mock Service lease is held, the startup-owner-unproven signal requests recovery, and two durable Profiles commits arrive. After releasing the lease, Startup recovery and one fresh SourceCommitted pass converge the newest version. This test exercises the caller's formerly lossy `reconcile_requested && !recover_startup` transformation.
- Existing Service readiness, S2 handoff, Source CAS, accepted-receipt verification, RuntimeProduct repair, release-owed maintenance, Core status racing and Stop intent tests must continue to pass.
- Rustfmt, complete `chimera-core` and Tauri library tests, Clippy, diff validation, signed commits and a fast-forward push are required.

## Limitations and rollback

This is a focused source-intent and recovery-completion bridge, not a complete upstream independent RuntimeReconciler Actor. No new background polling interval, public IPC, data schema, branch or gitlink is added. Injected Rust tests do not prove installed macOS/Windows/Linux Core/Service/TUN behavior. To roll back, revert the scoped code commit, then its design note; preserve unrelated origin UI commits.
