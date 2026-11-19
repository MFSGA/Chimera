# RuntimeReconciler: production S2 `plan(Facts)` ownership gate

## Baselines

- Chimera starting `master`: `fc50370df58e16c23f72f49f66647354c71b7aef`.
- Fixed `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (no gitlink update).
- Authoritative code: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs::plan(Facts)` and S2 `plan_owner`.
- Chimera integration: `backend/tauri/src/client/application_workflow/startup.rs`; existing single `CoreLifecycle` runtime owner and Service transition lease remain unchanged.

## What changed

Previously StartupWorkflow used the ref-aligned `plan_owner` directly, and its Service readiness follower separately checked `service_recovery_blocks_handoff`. This creates two independent policy branches even though a ref-shaped full planner already exists.

The S2 startup owner decision and the readiness-driven follow-up now first construct a **full ref Facts input** and call `plan(Facts)` with:

- `Trigger::ServiceReadiness`;
- `Ownership::Unproven`, because the stage's purpose is to prove or transition ownership;
- latest desired and currently selected `ExecutionHost`;
- fresh Service ownership evidence, with Chimera's error-stopped/quarantined daemon represented as **Unreadable** rather than ordinary stopped;
- `recovering` from the existing Service recovery gate for follow-up work; and
- `CoreFact::Unreadable`, explicitly restricting this partial-stage adapter to S2. It never interprets a subsequent S3/S4 Apply or Stop result as authorization.

Concrete `Step::Owner(OwnerPlan::RetireServiceThenLocal | HandBackToLocal | AdoptService)` decisions authorize the existing single-host transition procedure. `Step::Idle(Waiting)` prevents handoff, including for pending endpoint recovery, foreign/unreadable owners, and absent/unready Service.

The startup report API still distinguishes `OwnerPlan::WaitForService` and `OwnerPlan::Unproven` for accurate diagnostics, so those _waiting-only_ cells retain a typed `plan_owner` classification after the full planner returns `Idle`. They do not separately authorize a transition. Source state, run-intent policy, and Core mutation ownership remain unchanged.

## Tests

- One S2 matrix test asserts that the **full plan** selects each owner transition, and that absent/unreadable evidence results in `Idle(Waiting)` rather than Apply or Stop.
- The same test confirms that active Service endpoint recovery takes precedence over an otherwise eligible handoff.
- Existing S2 owner table, quarantine handling, stopped intent, Service readiness, and transition-lease tests must continue to pass.
- Full `chimera-core` and Tauri library tests, Rustfmt, Clippy and `git diff --check` are required before any push.

## Remaining alignment

This is a **production S2 planning gate**, not the full ref RuntimeReconciler Actor. StartupWorkflow continues executing transitions under the existing single CoreLifecycle/ServiceActor lease, and the S3/S4 committed-target path independently calls `plan(Facts)` only for stable proven hosts. The ref actor's unified fact collection and execution loop, ownership handoff admission after source commits, and real daemon/Core/TUN E2E remain to be migrated or verified. No new worktree, public IPC, source schema, UI or submodule version is introduced.
