# RuntimeReconciler: independent Service release-owed maintenance

## Fixed upstream baseline

- Chimera starting point: `ac0d4e045ddad09df0d7ceb2607de7dde53b0bcf`.
- Pinned `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`, unchanged.
- Ref source: `ref/backend/nyanpasu-core/src/client/reconciler/committed.rs::maintain_service_release`, `apply.rs::release_service` and `startup.rs::maintain`.
- Chimera mapping: `client/reconciler/release.rs` policy, `client/core_lifecycle/workflow.rs::maintain_service_release` execution, and the existing serialized `client/core_lifecycle/mod.rs` actor.

## Ownership and safety boundaries

A Service daemon is eligible for independent release only after the current **Local** Core has been verified against its exact accepted runtime receipt, target and selected instance, Service Mode is not preferred, and fresh Service evidence shows a **running daemon with a stopped Core owned by this installation**.

The maintenance workflow acquires the existing nonblocking Service transition lease, checks ownership evidence again while holding it, stops the daemon, confirms that it stopped, and re-probes to require an `Absent` daemon status. It also checks that the Local Core instance, run type and state-change timestamp did not change across the stop. The local runtime is not stopped, restarted or given a new Core config as part of cleanup.

The workflow refuses to stop a foreign Service, an unreadable Service, a still-running Service Core, a quarantined stopped Core, or an unproven Local Core. Running Service Core retirement remains the separate existing S2 owner-handoff flow, which uses conditional Core owner tokens rather than this daemon-only cleanup.

The Service endpoint automatic-recovery policy is suppressed when intentional release begins, as it is for an explicit StopService command. A failed stop is a maintenance debt, not a reason to roll back the committed source or invalidate a separately verified Local runtime.

## Debt and retry semantics

A dedicated `ServiceReleaseDebt` tracks `owed`, `in_flight`, and `pending_failure`. After Local convergence, and upon relevant Service observations, the single actor can schedule exactly one background `MaintainServiceRelease` command. It is a distinct CoreLifecycle mailbox operation, with a distinct `ServiceRelease` history trigger and no Core apply.

A failed stop retains the debt and publishes the error under `ReconcilerStatus.maintenance`. Ordinary Service readiness or source notifications cannot hammer the daemon: `pending_failure` latches the failure. An explicit user Retry can clear that latch and schedule another release attempt after the regular latest-source reconciliation. A successful stop clears the debt. Changing the selected preference back to Service or observing a foreign/absent daemon clears a stale debt without stopping anything.

The `ServiceRelease` round's outcome updates maintenance but does not change an independently verified `Converged` conclusion, nor does it advance observed source versions. No automatic unbounded retry timer or second runtime actor is introduced.

## Tests and acceptance

- Pure policy tests prove owned/stopped eligibility, foreign/running/unreadable exclusion, no duplicate in-flight attempt, failure latch, explicit retry and cancellation of stale debt.
- Actor integration tests verify initial automatic daemon release after Local convergence; injected stop failure leaves the daemon running and records maintenance; ordinary Service observation does not replay the failure; explicit Retry successfully stops the daemon and clears maintenance; the Local Core remains converged.
- A separate Actor test verifies an unowned Service daemon remains running and receives zero stop attempts.
- Full `chimera-core` and Tauri library regression suites, rustfmt, Clippy and `git diff --check` remain required before delivery.

## Known remaining work

This implements only the **release-owed daemon cleanup** case after Local convergence. It does not migrate the upstream independent RuntimeReconciler actor, its complete `plan(Facts)` round loop, intermediate Core ownership transitions, a persistent Service release queue, or filesystem promotion recovery before an accepted RuntimeProduct exists. Explicit Retry is the deliberate retry trigger after a failed release, not a new periodic poller. Real macOS/Windows/Linux Service and Core integration E2E remains unverified.

No persistence schema, public IPC, UI, submodule gitlink or new worktree is changed. A normal revert of the scoped code and design commits is sufficient for rollback.
