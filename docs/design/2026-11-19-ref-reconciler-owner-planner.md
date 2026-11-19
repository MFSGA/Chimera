# Ref RuntimeReconciler ownership planner — production startup wiring

## Upstream baseline and goal

This change is based on Chimera `master` at `1d020e0f` and the pinned
`ref` submodule at `c4b35e04`. The upstream
`client/reconciler/plan.rs` is the single decision table for deciding whether
a runtime stays on the current owner, adopts Service, hands back to Local, or
waits because ownership is unproven.

Chimera already carried a copy of that pure planner in
`chimera-core/src/client/reconciler/plan.rs`, but production startup still
executed a separately maintained copy in
`tauri/src/client/application_workflow/startup.rs`. This change replaces
that duplicate production rule table with a call to the migrated upstream
planner. **It is production wiring, not a full migration of the
RuntimeReconciler actor.**

## Changes and safety invariants

1. Export the reconciler's `plan_owner` and `OwnerPlan` from
   `chimera-core`. Startup's `plan_owner` now projects a single observation
   into the upstream planner and returns its decision.
2. Preserve Chimera's extra `CoreStoppedWithReason` evidence. A daemon that
   reports an error stop may still own quarantined Core processes. Such a
   report is projected to `Unreadable`, **not** `CoreStopped`, until the
   manager recovers and verifies the quarantine. This keeps the existing
   fail-closed behavior and prevents a second local Core starting over an
   unproven service owner.
3. Remove the duplicate `ServicePhase` enum and `readiness` implementation
   from `service_policy`. Existing code continues to import that module,
   but the type now re-exports the **ServiceActor-owned phase**. No second phase
   type can silently diverge from the actor. This also eliminates the manual
   11-variant phase-conversion table in the Tauri status projection.
4. Keep installation ownership checks and conditional-stop tokens in the
   existing startup host adapter. Foreign Core ownership still cannot be
   adopted merely because ServiceActor reports the daemon ready; confirmation
   must prove it is this installation's runtime.

## Validation

- `chimera-core --lib`: **384 passed**, 0 failed.
- `chimera --lib`: **997 passed**, 0 failed, 1 ignored; TypeScript bindings freshness passed.
- `pnpm lint:rustfmt` and `pnpm lint:clippy`: passed (existing unrelated warnings remain).
- `pnpm typecheck:interface` and `pnpm typecheck:chimera`: passed.

## Remaining architecture work

Chimera still executes startup, restart, Core mutation, service handoff, and
Profiles transactions through the compatibility `CoreLifecycle` /
`ApplicationWorkflow`. The upstream single RuntimeReconciler actor and its
shared `RuntimeGate` have **not** been instantiated as production owners.
Future migrations must move the actor's command loop and candidate-check
boundary together, with rollback-style Profile participants replaced by
source-first commit notifications. Avoid adding another parallel
runtime-mutation loop in the interim.
