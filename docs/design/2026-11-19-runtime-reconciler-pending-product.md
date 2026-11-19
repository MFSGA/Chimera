# RuntimeReconciler: pending confirmed RuntimeProduct publication

## Baseline and upstream mapping

- Chimera base: `bc97e3e39c8091bde31053fc1ef3d6ac6b0482e9` on `master`.
- Pinned upstream ref: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (unchanged).
- Authoritative reference: `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs::publish_committed_product`, `startup.rs::maintain` and `workflow.rs::RuntimeReconciler::pending_product`.
- Chimera adaptation: `client/runtime.rs::RuntimeLifecycle`, `core/actor_v2/local_runtime.rs`, `core/actor_v2/facade.rs`, `client/core_lifecycle/{ports,adapters,workflow,mod}.rs`.

## Product ownership and retry contract

Unlike ref's independent Reconciler, Chimera already stores the promoted and applied snapshots in one `RuntimeLifecycle` owned by `LocalRuntimeHost`. This batch retains that owner rather than copying the same mutable product into the compatibility CoreLifecycle actor.

A product is **pending** only when Promoted exists, Applied is not already identical, and the accepted, still-available confirmed receipt agrees with the product's runtime revision, digest and core kind. A copied receipt allocation, invalidated receipt, or newer promotion cannot borrow an older confirmation to publish itself.

Production `CoreFacade::republish_pending_runtime_product(target)` samples its selected host's authoritative active runtime receipt, checks the target and the accepted receipt identity, republishes only the derived Applied/legacy configuration snapshot, then rechecks the active receipt. It does **not** submit runtime bytes, start or stop Core, mutate Profiles, or create another runtime executor. If the active receipt changed during publication, the attempt reports a failure rather than claiming convergence.

On **explicit Retry** only, CoreLifecycle first tries this repair for the latest committed target. If it succeeds, the runtime round completes without calling Core reconcile again. Regular SourceCommitted notifications and Service ticks never replay an old pending product; changed source targets follow the normal latest-target reconciliation path instead. The existing CoreLifecycle status still requires proof that the accepted runtime target, host and running instance match before reporting `Converged`.

ReconcilerStatus maintenance includes a separate RuntimeProduct publication debt, keyed by the pending target. The actor samples the existing runtime owner after each completed round, so successful publication clears the debt without altering the last seen source versions or erasing unrelated Service maintenance.

## Acceptance tests

- Unit test: publish a promoted product with an accepted receipt, reject a cloned/foreign receipt, publish exactly once, and reject stale revisions and invalidated confirmation.
- Actor test: Core accepts a committed target but fails to publish its derived RuntimeProduct; maintenance is present. Explicit Retry republishes the same accepted product, Core submission count remains exactly one, and maintenance is cleared.
- Existing source version races, Service fallback, explicit Stop, Core receipt proof, candidate validation and transaction compensation tests continue to pass.
- Run Rustfmt, full chimera-core and Tauri library tests, Clippy, and git diff check.

## Remaining differences

The scope is a **confirmed, promoted** product whose Applied publication did not complete. Filesystem product promotion failure before Promoted exists and daemon-hosted products require a separate recovery design. Chimera does not yet have ref's complete pending RuntimeProduct and Service release-owed maintenance lifecycle, independent RuntimeReconciler Actor, or full `plan(Facts)` orchestration. Explicit Retry is the recovery trigger; this batch does not add an automatic retry timer or another Core writer.

Mocked Rust tests do not prove installed macOS, Windows or Linux Core/Service E2E. No persisted source schema, UI RPC, gitlink or worktree change is included. Revert the scoped code commit before reverting this note if rollback is needed.
