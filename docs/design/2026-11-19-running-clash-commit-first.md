# B2h — Running Clash overrides: source-first commit

## Basis

Base: Chimera `master` at `2eff6880` and pinned upstream `ref` at `c4b35e04`.
This batch follows the upstream source-owned `RuntimeGate` direction (candidate
check, source commit, committed-target reconcile), while using Chimera's existing
typed Clash actor and CoreLifecycle bridge. It **does not** claim that the
upstream single `RuntimeReconciler` actor has been fully ported.

## Behavior

The IPC, hotkey, and Agent path `patch_running_clash_overrides` no longer
applies a live `/configs` patch before persisting it. The path shares
`ClashConfigClient::patch_overrides` with persistent source mutations:

1. Project the requested overrides into the typed Clash candidate.
2. Check runtime-affecting candidates without changing the running core.
3. Commit the prepared source using versioned compare-and-replace.
4. Reconcile the committed version via CoreLifecycle and report degraded
   follow-up without reverting the source.
5. Publish the legacy mirror as a secondary effect. The applied runtime
   projection remains owned by RuntimeLifecycle.

The live-patch coordinator and its rollback implementation have been deleted
rather than kept as a second mutation owner. The optional boolean that allowed
skipping the independent runtime check and Core reconciliation after an
already-live patch has also been removed.

`TransactionOutcome` remains an IPC/Agent compatibility name, but now has
`Committed`, `CommittedDeferred`, and `Rejected` states. A deferred
runtime is still a successful _source commit_, so the IPC command does not
claim persistence failed; the Agent treats it as partially applied because
its stronger postcondition requires observed runtime convergence. A rejected
candidate is an error with no committed source mutation.

## Validation and follow-ups

The existing Clash actor tests cover rejection before commit, ordering,
version conflict, runtime apply failure after commit, and mirror degradation.
The routing Agent tests cover the new committed/deferred distinction.

Validation: `chimera --lib` passed **997 tests** (0 failed, 1 ignored);
`pnpm lint:rustfmt`, `pnpm lint:clippy`, `pnpm typecheck:interface`,
and `pnpm typecheck:chimera` passed. A temporary `pnpm` wrapper for
Corepack was required by the TypeScript-bindings test and nested scripts.

Remaining upstream alignment: migrate the production CoreLifecycle and
startup/retry/host execution ownership into the single RuntimeReconciler
actor; connect source actors to a common RuntimeGate and committed-version
notifications; migrate Profiles' old participant transaction to the same
source-first path. The current CoreLifecycle is still the compatibility
bridge for applying a committed Clash target.
