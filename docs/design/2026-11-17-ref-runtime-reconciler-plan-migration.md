# B2a: latest ref single-Reconciler decision table migration

## Exact source and goal

- Chimera `master` baseline `74949d00`, ref pinned to `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Ref: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs` (~871 lines), added during the new single-runtime reconciliation design. Chimera: `backend/chimera-core/src/client/reconciler/plan.rs`, with all seven upstream decision/host-evidence tests retained. The decision ordering, retry/fallback table, reason messages, unknown state handling, service-instance identity guard and trigger meanings follow the upstream file.
- This is the **pure decision engine**, with no process IO, disk IO, actor mailbox, or Tauri API. It does not create a competing runtime writer.

## Mapping notes

- Ref `nyanpasu-core-manager` -> Chimera's pinned `chimera-core-manager`; ref `nyanpasu-ipc` -> `chimera-ipc`.
- Ref `ExecutionHost` -> Chimera's `actor_v2::endpoint::ExecutionHost`. Ref `ServicePhase`/`ServiceHostStatus` -> Chimera's real `actor_v2::service_actor` types; Chimera's service status has a different UI representation and no upstream `log_query_version`/`logs` fields.
- `Ownership` and the minimal `RuntimeError` cause that the decision engine uses are local to the planner for this batch. Their conversions to the unique future `RuntimeReconciler` actor's error/status ports belong to B2b; do **not** publish a separate status watch or keep a duplicate authority in `CoreLifecycle`.
- The daemon config-dir match deliberately retains the protocol name `nyanpasu_config_dir` because this field is present in Chimera's service IPC wire. It is not an indication that Chimera should rename stored service fields without migrating the IPC protocol.

## Decision invariants copied from latest ref

1. A service endpoint with unreadable/unknown status never proves that its Core is stopped. A foreign-instance running Core is **not retired** by this app's local owner.
2. If service endpoint recovery is armed, the next step is `Waiting`; it does not force a host handoff. Host ownership must be proven before any `Apply`.
3. A read-only status that is transitional or absent blocks applying the committed target; a running core with no verified receipt is stopped before a new apply.
4. `RunIntent::Stopped` wins over forced Start/Retry, returning `Inactive` when already stopped, or `Stop` when running. Only an explicit stop carries the stopped intent.
5. A confirmed running target converges without a second apply, except under explicit Start/Retry/restart binary replacement.
6. Refused apply for the **same target and host** is `NotInEffect` until a forced retry or a genuinely different target/host. A refused Service attempt can fall back to Local; no generic automatic refusal retry loop.
7. No action mutates Core as a side effect of planning. Plan returns one typed step (`Idle`, `Owner`, `FallBackToLocal`, `Stop`, `Apply`); the single owner will execute it.

## Why this batch does not migrate Profiles yet

The current Chimera `ProfilesActor` still executes `replace_if_version_with_participant`, which may try a live apply _before_ source persistence through the old `ApplicationWorkflow`. The ref latest source owner uses independent candidate checks, `replace_if_version` / `replace_if_version_with_local_write`, and a post-commit notification to the **single** Reconciler. Replacing only the Profiles manager would turn an acknowledged live core mutation into a nontransactional side effect. Likewise, holding the ref `ProfileContentLock` across a legacy participant call would block that participant's own profile reads and introduce a deadlock. Neither unsafe change is made in B2a.

## Next step (B2b, required before retirement)

Move the source-owned `RuntimeGate` + `RuntimeValidatorPort` + `RuntimeBuildPort` connection from upstream `state/mutation/ports.rs`, `reconciler/check.rs` and `reconciler/preparation.rs`. Connect this to the **one** `RuntimeReconciler` actor mailbox and its watch conclusion. Only after an independent check can Profiles switch to ref `commit_with_resources` with `ProfileContentLock` spanning promotion, YAML persistence and recovery. Then remove the legacy participant/live-before-commit path in the same integration stage so there is never a second live writer.

## Acceptance gates

- All seven upstream plan and service-evidence tests pass, covering decision order, desired vs actual host, foreign owner, failed probe, forced retry, unknown receipt and recovery fences.
- Full `chimera-core` and Tauri test suites, TypeScript bindings, Rustfmt, Clippy and `git diff --check` must stay green. The source `ref` and Chimera runtime gitlinks remain pinned.
- The **pure planner is not yet used for production reconciliation**; complete alignment requires the single mailbox, commit-first gates and actual running-core acceptance tests. Do not claim this slice completes B2/B3.
