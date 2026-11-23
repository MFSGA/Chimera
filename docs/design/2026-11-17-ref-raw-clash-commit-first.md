# B2g — Raw Clash Mapping and persistent Overrides commit-first migration

## Upstream alignment

This batch extends the typed Clash commit-first transition to the remaining persistent source entrypoints:

- Legacy `Mapping` input used by `patch_clash_core_config` (`mixed-port`, `secret`, `external-controller`, and supported override keys).
- Persistent `ClashConfigOverrides` updates used by hotkeys and other source-owned callers.

The policy follows the current `ref` source owner and mutation model: derive a candidate from the current version; determine whether the candidate changes a runtime projection; independently check runtime-affecting candidates before persistence; commit with optimistic versioning; then reconcile the committed target. A postcommit apply or compatibility-mirror failure does not roll back the authoritative source document.

## Production changes

1. `ClashCommitReport` is now shared by typed, raw Mapping, and persistent Overrides entrypoints. The common `commit_candidate_with` helper owns prepare, preflight, conditional source publication, postcommit runtime apply, latest-version verification, and compatibility-mirror degradation reporting.
2. Raw Mapping is projected into a typed `ClashConfig` candidate first. The existing mixed-port availability and external-controller port checks remain before candidate validation and source publication. External-controller comparisons and port validation now use the typed candidate endpoint, including IPv6-safe parsed values, instead of splitting the raw address string.
3. Runtime impact is classified from the actual before/after typed runtime projection (`application_workflow::impact::classify_clash`). UI/policy-only or unchanged values do not invoke the runtime checker or Core rebuild. The old hand-maintained `requires_restart` whitelist was removed; every actual runtime input change follows the same candidate-check and CoreLifecycle reconcile path.
4. Mode-change detection compares the actual typed `Mode` before and after projection, not merely whether a `mode` key appeared. Connection interruption occurs after successful reconciliation.
5. After a successful source commit, the existing CoreLifecycle rebuild consumes the committed typed Clash source. The `RuntimeLifecycle` publishes the generated runtime mapping only after the candidate has been accepted and applied. The previous pre-build mutation of `Config::runtime()` was removed from normal source updates; the runtime builder already consumes `ClashConfig.overrides` directly.
6. `Config::clash()` and `Config::verge()` remain compatibility mirrors. Their save failures are reported as `legacy_clash_mirror_write_failed` while preserving the committed typed source. Retryable runtime failures request the existing coalesced CoreLifecycle rebuild.
7. The `ClashConfigOverrides` persistence callback used by the old `RuntimePatchCoordinator` is explicitly separated as `patch_overrides_after_running_apply`. That coordinator has already patched the running Core through its API, so the callback commits the typed source without checking/applying the same Core target a second time. It still publishes the compatibility runtime projection after source commit, retains the mode interruption side effect, and logs mirror degradations rather than returning a postcommit error that would incorrectly roll back the already committed Core/source pair.

## Regression tests

The Clash client suite now verifies:

- Raw Mapping mode candidate is checked while the previous typed source remains visible; Core apply observes the committed candidate.
- A postcommit Core failure returns a deferred degradation and leaves the source committed.
- A candidate rejection leaves source version/state unchanged and invokes neither apply nor mirror save.
- Unchanged raw mode values skip runtime checking and applying.
- Mode-change side-effect planning distinguishes an actual mode transition from a present-but-unchanged field.
- The already-live-applied compatibility callback commits source state without a second candidate check or Core apply.
- Existing typed-path tests continue covering version conflicts, source persistence failures, concurrent source advancement, and legacy mirror failures.

Validation completed:

- Focused Clash client tests: **20 passed, 0 failed**.
- `chimera-core` library: **384 passed, 0 failed**.
- `chimera` Tauri library: **1005 passed, 0 failed, 1 ignored**; Specta bindings freshness included.
- `pnpm typecheck:interface`: passed.
- `pnpm typecheck:chimera`: passed.
- `pnpm lint:rustfmt`: passed.
- `pnpm lint:clippy`: passed. Existing unrelated manifest/dependency warnings remain.

## Explicitly deferred path

The old `RuntimePatchCoordinator` still performs a live Clash API mutation before its persistence callback. This batch only prevents that callback from causing a second rebuild and ensures a postcommit mirror error cannot trigger a false rollback. Migrating the live runtime patch itself to source-check → commit → apply is a separate architectural step, because it requires moving the coordinator behind the single RuntimeReconciler/RuntimeGate rather than preserving its current apply-first transaction.

## Runtime repository

No `backend/chimera-runtime` or `Chimera_Service` change is needed in this batch. The existing `chimera-core` RuntimeBuilder already reads the typed `ClashConfig.overrides`, and RuntimeLifecycle publishes the generated applied product. No IPC/schema or runtime receipt change was introduced, so the runtime submodule gitlink is intentionally unchanged.
