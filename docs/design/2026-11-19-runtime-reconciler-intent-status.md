# Ref RuntimeReconciler: run intent and round-status bridge

## Baselines and ownership

- Chimera baseline: `2b2e8d6c` on `master`.
- Fixed `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (not advanced).
- One runtime owner: existing `backend/tauri/src/client/core_lifecycle` actor. This batch does not spawn a second reconciler or change the ServiceActor.
- The upstream source of truth is `ref/backend/nyanpasu-core/src/client/reconciler/{workflow,mod,status,plan}.rs`. Preserve upstream semantics and adapt source/event shapes only at the Chimera boundary.

## Migrated semantics

1. **Explicit RunIntent:** use the existing pure `chimera-core::client::reconciler::plan::RunIntent` type. `StopCore` latches `Stopped` and `StartCore` is the sole explicit command that resumes `Running`. SourceCommitted, runtime-dirty events, deferred retries, crash notices and Service readiness must not auto-restart a deliberately stopped runtime. Existing startup recovery and Service handoff remain in the single lifecycle actor.
2. **SourceCommitted as a distinct trigger:** a runtime-affecting source event schedules a coalesced `ReconcileSource` round. Unlike a manual `Reconcile`/retry, the round is labeled `SourceCommitted` in recent history. The runtime continues reading the latest committed source, not the event payload. Source-only commits update the observed version without Core application.
3. **Ref-shaped ReconcilerStatus:** one actor-owned watch carries `seq`, `active`, sampled `seen` source versions, typed `Conclusion`, degradations, maintenance placeholder and bounded recent round records. `settled(domain,version)` waits until the round sampled the requested version and is no longer active. Notifications alone do not advance the sampled version for an outstanding runtime apply.
4. **Evidence before conclusions:** `Converged` requires an observed `Running` Core after a successful reconcile; `Inactive` requires observed `Stopped` after explicit Stop. A successful command without corresponding observation is `Waiting`. Core errors are `NotInEffect` or `Unknown` if execution is uncertain. A source advancing mid-apply produces `Waiting` and another latest-source round.

## Regression contracts

- Explicit Stop → Profiles source commit + RuntimeDirty → no Core rebuild → explicit Start → Core applies latest source.
- Service readiness after explicit Stop must not resume the Core. An externally stopped runtime without explicit Stop continues following Service readiness.
- Ref-style status `settled` returns an inactive conclusion for a stopped runtime with a newer durable source version.
- Source-triggered rounds are labeled `SourceCommitted`, distinct from `Retry`.
- Stop returning success without an observed Stopped Core remains `Waiting`.
- Source version races, postcommit failures and previous candidate checks remain covered by existing tests.

## Intentional remaining gaps

This bridge is **not** the completed upstream standalone `RuntimeReconcilerActor`. The existing ApplicationWorkflow/CoreLifecycle mailbox is still the sole mutating owner. Source selection and typed committed-target capture exist, but ref's complete per-target attempt policy, Service handoff planner, maintenance/degradation projection and user-facing ReconcilerStatus IPC have not been copied. These require replacing the old lifecycle orchestration rather than adding a second competing Core controller. Explicit Start/Stop are now available through the typed internal client, but there is no new UI button or IPC command in this batch. Real installed-app Core/Service/E2E validation remains outstanding.

## Verification and rollback

Run `cargo fmt --manifest-path backend/Cargo.toml -p chimera-core -p chimera -- --check`, `cargo test --manifest-path backend/Cargo.toml -p chimera-core --lib`, `cargo test --manifest-path backend/Cargo.toml -p chimera --lib`, `cargo clippy --manifest-path backend/Cargo.toml -p chimera-core -p chimera --lib --tests`, and `git diff --check`. Record actual outcomes; mocks do not prove a real Core/Service E2E.

No persistence schema, UI wire format or gitlink migration was introduced. If reverting, undo the status-watch commit first, then the RunIntent/SourceCommitted trigger commit, preserving unrelated remote commits.
