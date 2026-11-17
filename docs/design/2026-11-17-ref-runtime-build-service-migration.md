# Ref runtime build: pure builder and blocking candidate service

## Baseline and source mapping

- Chimera base `master`: `1bf80c97`. Fixed ref: `59724e85c34abb6995c22d101c876cc06dc16576`; neither ref nor runtime gitlink is modified.
- Reference split: `ref/backend/nyanpasu-profile-transform/src/runtime_builder.rs` owns pure validation/execution, while `ref/backend/nyanpasu-core/src/client/runtime_build.rs` owns blocking candidate orchestration via injected script/content ports.
- Chimera previously combined both in `backend/tauri/src/enhance/runtime_builder.rs`.
- The pure `RuntimeBuildInput`, `RuntimeBuildError`, `RuntimeBuildLog`, `derive_tun_flavor`, transform-failure diagnostics and the executor invocation now live in `backend/chimera-core/src/client/runtime_build.rs`.
- The same shared module exposes `RuntimeBuildService::build_candidate`, which creates and drops its injected `ScriptRunner` on one `spawn_blocking` thread, executes the typed candidate, validates transforms when strict, then maps to `ArtifactSnapshotData` through `client/artifact_snapshot.rs`.
- Tauri's `build_from_profiles_with_inspection` **actually calls** the shared service. The `enhance::RuntimeBuilder` shell wrapper only resolves the existing `BuiltinTransform` table and delegates to the shared implementation for legacy callers and existing tests. There is no duplicated business-policy implementation.

## Invariants preserved from Chimera

- `Profiles::validate()` happens before pipeline execution; the existing typed `RuntimeBuildError` variants, particularly `TransformsFailed` with per-node diagnostics, remain available to the existing `CoreLifecycle` error downcast.
- Transparent proxy capture still requires Service Mode. Transparent proxy validation, Linux `routing-mark` generation, port resolution inputs, Chimera Client MMDB support, include-all expansion and TUN stack behavior are unchanged.
- The built-in source table remains in Tauri, including `config_fixer` and Chimera Client variants; the pinned ref has a different built-in selection table and target core set. No scripts or platform assets are duplicated in shared Core.
- Runtime scripts are instantiated **inside** `spawn_blocking`, not on the Tokio worker; the runner has no cross-thread `Send` requirement. Staged profile content overlays disk for a candidate and does not rewrite the materialized source.
- `RuntimeBuildService` does **not** write the product, apply to an endpoint, mutate a configuration draft or publish a runtime receipt. Those side effects remain with the existing Tauri application/lifecycle workflow. This isolates the functional candidate build without introducing another state owner.
- `ArtifactSnapshotData` keeps Chimera's current YAML `Mapping`, ordered applied-field keys, old `PostProcessingOutput` wire shape and in-memory inspection graph, as explained by the preceding D1 migrations.

## Regression tests

- New shared Core tests confirm blocking-thread runner initialization, the typed transparent-proxy/service-mode error, and that strict transform failures are rejected while lenient candidates keep diagnostic data.
- New Tauri test uses a real temporary managed profile file, overlays staged YAML, inspects a non-managed `extra-key`, and verifies the original file remains unchanged. The `mode` field is intentionally **not** used for this test because managed Clash settings overwrite it during the pipeline.
- Existing `enhance::runtime_builder` tests remain unchanged for MMDB, transparent proxy, TUN flavor, include-all, builtins and actual JS script transformations.
- Before shipping: run both complete crate test suites, TypeScript binding freshness, Rustfmt, Clippy, Git diff check, GPG-signed commit and fast-forward push. Full desktop/Service/TUN E2E is a separate gate.

## Outstanding ref alignment

This slice migrates pure candidate construction, not the entire upstream `RuntimeBuildPort` lifecycle or its `CoreSpecResolver`. Content-source capture still uses Chimera's `CandidateProfileContentSource` and its configured materialized directory; port selection/identity and publication still occur in the existing Tauri/Actor workflow. A subsequent integration should move the complete injected Port facade only together with the sole owning Workflow, and explicitly preserve manual Stop, pending-action, handoff and endpoint-generation fences.
