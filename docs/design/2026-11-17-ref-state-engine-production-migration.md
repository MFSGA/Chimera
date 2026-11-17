# B1a — ref StateEngine / Persistence extraction into production typed config owners

## Source and goal

- Chimera baseline `fc0b3a34` (remote `master` after syncing concurrent UI commits).
- Fixed ref `dd42ab212bcab4c61601a8c6922b26231609ba49` (`main`, 2026-11-17).
- Upstream reference copied into `backend/chimera-state-engine/src`: `ref/backend/nyanpasu-state-engine/src/{lib,version,snapshot,error,manager,tests}`. The entire current upstream `PersistentStateManager` implementation, including its MVCC, conditional-write recovery, serialization-failure and snapshot-settlement tests, lives in this standalone crate.
- Upstream splits document serialization/recovery into `nyanpasu-persistence`. `backend/chimera-persistence` is the matching Chimera crate for **format ownership**. We moved `chimera-core/src/format.rs` unchanged into `chimera-persistence/src/format.rs` and retained the previous `chimera_core::format::*` export as a compatibility facade. This keeps old `_nyanpasu` stamp and YAML encoding byte-for-byte while preserving the pinned Chimera `serde_yaml_ng` backend. Ref `yaml_serde`, additional persistence utilities and stamped-file write ports are separate future dependencies; do not silently mix the YAML backend change into state ownership migration.

## Production wiring

Three typed config Actors now actually own the **new** `chimera_state_engine::PersistentStateManager<T>` and publish `chimera_state_engine::StateSnapshot<T>`:

1. `backend/tauri/src/state/application.rs` / `client/application.rs` (Application source document).
2. `backend/tauri/src/state/clash_config.rs` / `client/clash_config.rs` (Clash configuration source document).
3. `backend/tauri/src/state/session_state.rs` / `client/session_state.rs` (window/session source document).

Their source readers, actor message serialization, initial typed-vs-legacy selection, release channel validation, staged legacy mirror preparation, mirror-after-commit ordering, and public IPC remain the same. `PersistentStateManagerSetup::from_state` is synchronous in ref: startup now directly creates an initial version-0 store from its already-validated seed instead of invoking the old coordinator setup. Subsequent `upsert` and `replace_if_version` perform atomic persistence and publish a versioned snapshot before reporting success. No new second writer has been added to these domains.

## Ownership boundary / intentionally not yet migrated

- The old `chimera_core::state` manager, coordinator and mutation-participant machinery remain **only for legacy consumers still using them**, especially `ProfilesActor` and CoreLifecycle/ApplicationWorkflow. Profiles has a file-first transaction and recovery journal; migrating only its manager type would bypass that protocol, so this requires the subsequent upstream **commit-first / RuntimeReconciler** migration.
- This B1a slice does **not** replace the old runtime lifecycle, satisfy the full ref single-reconciler contract, or change when runtime effects are applied. The production source engine foundation is now isolated and owned by typed Actors, but the live runtime still has an older workflow requiring replacement.
- Ref has removed `ClashPremium` and Script proxy mode. Do not silently remove Chimera configuration variants as a side effect of changing persistence state representation; those are separately scheduled breaking schema migrations.
- Keep `ref` and `backend/chimera-runtime` gitlinks unchanged in this batch.

## Acceptance gates

- Standalone `chimera-persistence` format tests: stamped and unstamped parsing, round-trip, expected schema and reserved-key errors.
- Standalone `chimera-state-engine` tests: upstream snapshot MVCC, version monotonicity, stale-CAS conflict, atomic file preservation on serialization failure, conditional resource recovery, lost publication error and nested write recovery.
- Tauri domain tests: Application nightly release/update source policy, Clash and Session mirror-prepare failures (must keep old state/version), queued durable window updates.
- Full shared `chimera-core` and `chimera` unit suites, Specta binding freshness, Rustfmt/Clippy and Git diff check. All changes must be GPG-signed; preserve remote commits via rebase/fast-forward and push only on validated integration.

## Next milestone B1b/B2

1. Copy and integrate ref `reconciler/check.rs` and its `RuntimeValidatorPort` in the actual source mutation path. Do **not** apply the candidate to the running core before source persistence.
2. Move `ProfilesActor` source transaction to the ref versioned state owner while retaining staged local resource recovery. Ensure a single read-only state snapshot and domain-commit notification per source version.
3. Copy the ref `reconciler/plan`, `workflow`, `startup`, `apply` and actor mailbox and migrate all runtime-triggering paths to it, then retire old `CoreLifecycle`/`ApplicationWorkflow` production writers. Only then can B2/B3 claim complete architecture alignment.
