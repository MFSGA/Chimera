# Commit-version fence for Clash and Application runtime apply

## Context

Baseline: Chimera `master` at `b22ea5eb`, pinned upstream `ref` at
`c4b35e04`. Upstream's `RuntimeGate::committed(domain, version)` wakes a
single `RuntimeReconciler` actor, which reads the **latest** committed target.
Chimera still applies some committed Clash/Application mutations through the
existing CoreLifecycle compatibility owner. That synchronous apply previously
checked for a competing source commit **only after** touching the Core.

## Implementation

The new `backend/tauri/src/client/reconciler/committed.rs` provides a single
commit-version fence for the two existing source-first paths. After the source
owner has committed its version:

1. Read that source's current version before the Core apply. If a second
   commit already superseded it, skip the obsolete apply, return a retryable
   `{domain}_runtime_deferred` degradation, and let the existing CoreLifecycle
   rebuild request reconcile the latest target.
2. Apply the committed target through the **existing** CoreLifecycle owner if
   the source version is still current.
3. Read the source version again after the apply. A competing commit while the
   endpoint operation was in flight still yields a retryable deferred status,
   never an incorrect claim of convergence.
4. Treat version-reader errors after a successful source commit as deferred
   convergence, not a failed source write or a purported rollback.
5. Preserve Core apply errors as runtime degradations, with their existing
   retryability classification; source persistence remains authoritative.

This does not create a new Core writer, does not alter the application's
explicit stop intent, and does not change Profiles' still-legacy transaction.
It reduces the window for obsolete Core applications without claiming to
eliminate all concurrency races; **the final invariant requires the upstream
single RuntimeReconciler actor**.

## Regression

Focused tests verify that a superseded candidate never calls Core apply,
a newer version during apply requires convergence, unreadable versions fail
closed, Core errors remain postcommit degradations, and a current target
applies exactly once.

The compatibility-only static Application and Clash test sources previously
returned version zero after every write while reporting `expected + 1` as
the committed version. They now hold document and version under the same
lock, with atomic compare-and-replace semantics and monotonic revisions.
This is necessary to make those tests model the production source actor,
rather than bypassing or relaxing the committed-version fence.
