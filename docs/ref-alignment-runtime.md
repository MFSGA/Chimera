# Runtime config path and rollback alignment

Checked against `ref` commit `0c7b15b3c61db390db21cffb63d29805abd380a1` on 2026-10-13; the reference worktree was clean.

The shared path and candidate flow maps to `ref/backend/tauri/src/client/runtime.rs::RuntimePaths`, `create_candidate`, and `promote_candidate`. Chimera keeps those paths and names: the runtime product is `<config>/runtime/clash-config.yaml`, while candidate files live under `<config>/runtime/.candidates`.

Chimera additionally has `RuntimeTransactionSnapshot`, `capture_runtime_transaction`, and `restore_failed_apply` in `backend/tauri/src/client/runtime.rs` to preserve a prior runtime product and lifecycle state during a failed core restart. The matching restart recovery path in `backend/tauri/src/core/clash/core.rs` is incomplete and currently has `todo!()` branches, so this transaction helper has no active production caller. This remains a partial migration; it is not evidence that core restart rollback is complete.

The typed user settings file at `<config>/clash-config.yaml` is owned by `PathResolver::clash_config_path()` and the typed-config migrator. It must never be treated as a runtime-product backup. If the runtime product is absent, a transaction snapshot records `None`; rollback removes a failed new runtime product and leaves the typed settings file byte-for-byte unchanged.

Recheck this difference when the core restart recovery flow is completed or the reference changes its runtime transaction model. At that point, preserve the reference candidate/promote sequence and document only the smallest compatibility behavior needed for Chimera's lifecycle recovery.

Validation on 2026-10-13: the targeted rollback regression passed. The first full `pnpm test:backend` run exposed a timing race in `chimera-utils/tests/process_supervisor.rs::readiness_is_not_starved_by_continuous_output`: its finite-output child could exit before the readiness event under workspace load. The helper now streams until the test stops it; the focused process-supervisor test and a second full `pnpm test:backend` run both passed.
