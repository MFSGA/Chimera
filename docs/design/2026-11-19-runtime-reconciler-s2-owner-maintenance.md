# RuntimeReconciler S2 owner follow-up and Service maintenance bridge

## Baselines and scope

- Chimera baseline: `07df3558f4d67ddabb436b0731a8b8e0d74cd70d` on `master`.
- Pinned ref: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`, unchanged.
- Upstream decision: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs::plan_owner`, corresponding S2 ownership actions in `ref/backend/nyanpasu-core/src/client/reconciler/workflow.rs`, and `pending_release` maintenance in `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs`.
- Chimera bridge: `backend/tauri/src/client/application_workflow/startup.rs::service_host_change_due`, `backend/tauri/src/client/core_lifecycle/mod.rs`, and `backend/tauri/src/client/reconciler/status.rs`.

## Migrated behavior

**S2 ownership follow-up.** Service readiness no longer merely compares `desired_host != selected_host`. It asks the existing ref-aligned `plan_owner` for a _concrete ownership action_. `RetireServiceThenLocal`, `HandBackToLocal`, and `AdoptService` warrant the single CoreLifecycle actor's transition request; `Owned`, `WaitForService`, and `Unproven` do not. This catches an important double-owner case: the UI still selects Local but a **verified, locally owned** Service Core appears and must be conditionally retired before any new Local application. No action is taken for a foreign or unreadable Service Core. The decision remains enabled when Service Mode is not preferred, so returning to Local can still require Service release.

**Execution and safety.** The ownership action stays in the existing `ApplicationWorkflow::startup_reconcile_once` and the CoreFacade with a Service transition lease. `retire_service_startup_owner` uses the expected Core owner token and re-probes after stop. The one lifecycle mailbox still enforces user `Stopped` intent, startup owner proof, uncertain-result barriers and bounded request admission. No second owner, new RPC, source writer or explicit Service start is introduced.

**Maintenance.** The single Actor publishes two independent pieces of maintenance debt through the existing ref-shaped `ReconcilerStatus.maintenance` string: (1) a Service owner handoff waiting for a lease/queue or failed handoff, and (2) Service endpoint recovery that is running, waiting for a lease or failed. It clears each debt only after its corresponding successful operation. The projection does not alter `Conclusion` or `seen` source versions, and identical messages do not increment the publication sequence. No new automatic Service retry loop is created by this status change: the existing readiness and endpoint-recovery scheduling remain the only triggers.

## Regressions

- Planner unit: Local/Local with an owned, running, ready Service Core **does** owe `RetireServiceThenLocal` even though the selected and desired host match. Absent, stopped, foreign and unreadable Service Cores do not trigger retirement. An unavailable Service is not force-adopted.
- Actor: the second owned Service Core is retired before an optional Local rebuild, leaving Local selected.
- Actor: one failed handoff lease is reported in maintenance without stopping the Service Core; a subsequent successful lease and owner release settles maintenance.
- Status unit: maintenance updates preserve last runtime conclusion and observed source versions; duplicate updates are coalesced.
- Existing service-readiness forward/backward handoff, stop-intent, retry, candidate validation and source-race tests must continue passing.

## Remaining ref architecture work

The reference's complete `plan(Facts)` rounds, including S3/S4 applied-target selection, automatic Service-failure fallback during ordinary post-commit runtime rounds, release retry policy, pending product publication and independently owned RuntimeReconciler Actor, have not been copied wholesale. Chimera's existing StartupWorkflow still performs Service→Local fallback after a known Service reconcile failure, and remains the sole executor until that transition is migrated coherently. This batch does not claim production Core/Service E2E or UI IPC status coverage.

## Acceptance and rollback

Run full `chimera-core` and Tauri library tests, `cargo fmt --check`, `cargo clippy`, and `git diff --check`. Rust tests use injected ports, not installed OS daemons; no system proxy/TUN behavior was directly exercised. Keep commits signed and non-squashed. To roll back, revert the scoped S2 owner/maintenance code commit; the design note can be reverted separately. No schema, submodule, existing API or worktree change is introduced.
