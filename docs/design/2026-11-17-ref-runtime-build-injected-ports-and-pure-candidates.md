# B2c — Ref-style RuntimeBuildService ports and side-effect-free candidates

## Baseline and exact ref correspondences

- Chimera starting `master`: `ee1dedfbc84cc638f8af34e54725e0e3999b95cc`; pinned ref: `dd42ab212bcab4c61601a8c6922b26231609ba49`.
- Ref files: `ref/backend/nyanpasu-core/src/client/runtime_build.rs`, `client/reconciler/{ports,preparation,check}.rs`, `actor_v2/local_host.rs` and `state/mutation/ports.rs`.
- Goal of this batch: prepare the **real production** CoreSpec and injected candidate build ports used by the independent check boundary. Preserve Chimera's transactional promotion until the single actor takes over publication. Do not create a second Core writer or claim the full RuntimeGate/Reconciler cutover is complete.

## Production changes

1. **CoreSpec policy now lives in the shared Core** (`backend/chimera-core/src/client/core_spec.rs`). It maps the typed `ClashCore` to `CoreKind` and an injected host binary finder, returning a typed `CoreSpecError` on missing or non-UTF-8 paths. The real `backend/tauri/src/core/actor_v2/local_host.rs` now delegates its `core_spec_with()` to that function, retaining the host's exact binary search and executable suffix rules. ChimeraClient and ChimeraClientAlpha use the ChimeraClient manager kind, not ClashRust; legacy ClashPremium remains explicitly supported until its schema migration.
2. **RuntimeBuildService now accepts host adapters** (`backend/chimera-core/src/client/runtime_build.rs`). Like ref, `new(content, scripts)` injects an `Arc<dyn ProfileContentSource + Send + Sync>` and an `Arc<ScriptRunnerFactory>`. A separate `capture_content` method freezes the full current dependency closure without touching a Core or runtime product. The production `build` captures managed paths before constructing the script runner, then executes using immutable content on one `spawn_blocking` worker. Its output is only `ArtifactSnapshotData` with typed transform errors and inspection data; it is never published during candidate validation.
3. **Tauri now uses this instance service in production** (`backend/tauri/src/enhance/runtime_builder.rs`), supplying `FsProfileContentSource` and `EnhanceScriptRunner::new`. The former static builder remains available for compatibility and existing controlled tests, but the real configuration build uses the injected instance instead of reconstructing input ports inside the build call. A test now exercises the injected path, deliberately deleting the file in the script-runner factory after capture and verifying the frozen bytes remain the ones used in the output.
4. **Candidate generation no longer writes Legacy Runtime drafts** (`backend/tauri/src/config/core.rs`). A candidate check or failed apply used to mutate `Config::runtime().draft()` before the runtime was confirmed. `generate_runtime_output_from_profiles` now returns immutable generated data only. For explicit legacy `Config::generate()`, the old "prepare a draft" behavior is retained. **Only after** `LocalRuntimeHost` verifies the Core receipt, promotes the exact candidate bytes and publishes the matching applied snapshot does it call `Config::publish_applied_runtime_legacy(&product.config)`. The identical rule now applies to runtime receipt restore; successful receipt restoration publishes the actual confirmed mapping rather than whatever stale draft happened to be present.

## Behavior and safety invariants

- No generated candidate, advisory check, transform failure, or failed Core apply mutates the Legacy Runtime draft. It may still collect diagnostic transform failures in the dedicated diagnostics channel, as before.
- The snapshot, CoreIntent bytes/digest, and confirmed receipt continue to be checked by Chimera's existing transaction before RuntimePaths promotion; this patch deliberately does **not** substitute the ref's raw `write_product` for its receipt-checked path.
- `ScriptRunnerFactory` creates and drops each runner on the blocking executor that uses it; source closure is frozen before its construction, so a mid-build managed-file change cannot change output bytes.
- CoreSpec's binary search remains a host responsibility. Shared Core resolution performs no filesystem reads on its own, no process mutation and no global state changes.
- Preserves IPC, frontend shapes, runtime gitlinks, persisted YAML encoding and MMDB/TUN/transparent-proxy specifics. The new `CoreSpecResolver` type is read-only and can be injected into the future ref-style RuntimeBuildPort.

## Acceptance and next integration

- CoreSpec: mapping for all seven current Chimera core variants, accurate alpha executable path, missing-binary and invalid UTF-8 errors; existing `local_host` tests must remain passing.
- RuntimeBuildService: injected factory once per candidate on a blocking worker; strict transform rejection and lenient diagnostics; frozen content remains unchanged if the backing file disappears after capture.
- Legacy mirror: a rejected/pending draft cannot become the applied snapshot; the promoted receipt's mapping replaces pending legacy content only after confirmed publication.
- Full `chimera-core` and Tauri unit tests, Specta binding freshness, Rustfmt, Clippy and `git diff --check`; GPG-signed meaningful commits on `master` with rebase before pushing.
- **Incomplete:** Ref `RuntimeBuildPort` also owns post-confirmation `publish`, and `RuntimeGate::check` forwards one immutable PreparedRuntime to the single `RuntimeReconciler`; Chimera still uses the legacy pre-commit participant/CoreLifecycle. The following integration batch must migrate those together with commit-first Profiles. Do not run two independent runtime mailboxes or publish through both ports at once.
