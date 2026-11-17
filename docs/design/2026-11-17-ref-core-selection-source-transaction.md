# B2e — Core selection through the Application source owner

## Source, scope and baseline

- Baseline: Chimera `master` `b94079bf05c3e9c115d566dcc6b37eb57530c99b`, pinned latest ref `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Upstream exact contract: `ref/backend/nyanpasu-core/src/client/mod.rs`, `update_core(core)`, which builds an Application config patch and calls `patch_app_config`; its eventual `RuntimeReconciler` is notified of the committed source rather than receiving a separate core-switch command.
- Chimera was still exposing `change_clash_core → ChimeraClient::change_core → CoreLifecycleClient::select_core → CoreLifecycleWorkflow::select_core`, which tried the new Core **before** saving the chosen core and compensated by attempting another Core reconcile when source commit failed. This is incompatible with upstream commit-first semantics.

## Production migration

1. `ChimeraClient::change_core` now builds `ChimeraAppConfigPatch { core: Some(core) }` and calls the **same production** `patch_app_config` source mutation that other Application changes use. This reuses the independent advisory check, conditional source commit, and only-then Core apply wired in B2d. It reuses existing transparent proxy validation, source publication, legacy mirror, UI refresh, effect notifications and coalesced retry handling; it creates **no second runtime actor**.
2. The old `CoreLifecycleClient::SelectCore`, the previous direct before-commit `CoreLifecycleWorkflow::select_core`, and `ApplicationClient::patch_core` are gated under `#[cfg(test)]` for the remaining legacy mailbox/adapter tests. They are **absent in non-test production builds**. Delete these test fixtures once test cases are migrated to source-driven `change_core` commands; do not reintroduce the runtime write route.
3. IPC `change_clash_core` now returns `MutationOutcome<()>` instead of an ambiguous unit response. The generated Specta binding returns `MutationOutcome<null>`, preserving the ref `Committed / CommittedDegraded` discriminant and per-domain `CommitReceipt` so callers can tell "saved, running application deferred" from an admission/check/source-persistence failure. **Do not** turn committed but degraded into an `Err`, which would falsely imply that the new core selection was rolled back.
4. The current settings UI consumes the typed receipt. On `CommittedDegraded` it shows a non-success notification and distinguishes `runtime=deferred` (source saved, Core not applied) from a peripheral degradation such as Legacy mirror write failure. On success it retains its existing success notification. It **no longer unconditionally deletes active connections before candidate check**; the runtime Core owner/selected policy handles connection interruption after a successful commit.
5. `useClashCores.upsert` continues to refresh the selected core/query and runtime status on any settled outcome. Specta bindings were regenerated with the repository's own ignored export test, not handwritten.

## Regression requirements

- `change_core` uses `rebuild`/CoreLifecycle reconcile rather than the obsolete direct `change-core` adapter, with the typed Application source containing the newly selected core.
- Injected post-commit Core application failure produces a `CommittedDegraded` result, carries `application_runtime_deferred` and leaves selected core saved.
- Core selection tests use an isolated **real persistent Application Actor**, not the process-global Legacy config, so concurrent unrelated tests cannot produce false version-advance reports. Separate peripheral mirror degradation does not imply that Core application failed.
- Separate B2d tests prove failed check and disk-write failures never mutate Core, and the version fence prevents stale source convergence reports.
- All CoreLifecycle tests, full `chimera-core` and Tauri suites, TypeScript binding freshness, frontend interface/Chimera typechecks, formatting and Clippy remain green. Preserve the ref/runtime submodule SHA and commit directly on `master` with verified signatures.

## Not yet completed

The single `RuntimeReconciler` Actor, upstream RuntimeGate/SourceCommitted actor notifications, independent Profiles commit-first file promotion and Clash source owner still require migration. This batch replaces the **core selection** production write path, not all Core/Service operations. Host ownership, startup retries and Profile transactional semantics remain under the existing single CoreLifecycle until the one-owner cutover. Core executable smoke tests and Service/TUN host handoff on actual hardware remain unverified by these unit tests.
