# D1 — Runtime Inspection read-only projection in shared Core

## Reference and migration scope

- Base: `master` `0491cc94`; pinned read-only ref gitlink `59724e85c34abb6995c22d101c876cc06dc16576`.
- Ref module: `ref/backend/nyanpasu-core/src/client/runtime_inspection.rs`.
- Previous Chimera owner: `backend/tauri/src/client/runtime_inspection.rs`.
- New shared business owner: `backend/chimera-core/src/client/runtime_inspection.rs`.
- Chimera's current `ConfigSnapshotsGraph` is an **in-memory** graph, while this pinned ref uses `StoredConfigSnapshotsGraph`. Their storage and restoration semantics are not interchangeable. This slice follows the ref module boundary while preserving Chimera's exact graph representation and current inspection contract. Do not silently substitute the upstream stored graph.

## What moved

- `RuntimeInspectionData`, `EffectiveInspection`, `RuntimeInspection`, `RuntimeInspectionNode`, `RuntimeInspectionContent`, `RuntimeInspectionDiff` and their existing Specta/Serde wire shapes.
- Graph root creation, effective-view projection, selected-profile labels, changed-field path extraction, maximum log severity, graph navigation and per-node content/diff rendering.
- All emitted YAML and diff content uses `redact_config` before being returned to the UI. The change-path comparison deliberately occurs on unredacted inputs so secret-only changes remain visible by **path**, never value.
- Pure projection accepts explicit snapshot IDs, revision, selected core, and optional authenticated effective view instead of capturing a live Tauri `RuntimeSnapshot` or actor.

## Tauri adapter retained

- `RuntimeSnapshot` remains Tauri-owned for now. Its three adapter methods delegate to the shared Core projection and preserve the existing deterministic `:effective:epoch:generation:hash` derived ID semantics.
- `ChimeraClient::active_effective_inspection_config` still uses two authoritative CoreActor status observations and the active runtime receipt to fence host, generation, instance and applied revision. It does not expose an effective document from an unproven host.
- Existing `crate::client::runtime_inspection::*` paths and IPC exports remain compatible through type re-exports.
- No change to runtime building, product files, persisted configuration, service mutation, profile TCC, OS effects, submodule pointers or UI source is included.

## Verification

- Shared Core tests explicitly cover bare graph projection, stale snapshot IDs, missing nodes, effective-view attachment and sensitive content/diff redaction.
- Tauri's existing 7 Runtime Inspection tests cover CoreActor generation fencing, log severity, branch links, effective reports and stale revision rejection.
- Run both crate-wide test suites, TypeScript binding freshness, the staged Rustfmt/Clippy hooks and `git diff --check` before committing.
- Desktop E2E, privileged Service/TUN and real host handoff remain separate release gates; this extraction does not prove them.

## Next task

Evaluate a cohesive `RuntimeBuild` move only after comparing Chimera's `enhance/runtime_builder.rs` and `enhance/artifact_bridge.rs` with the ref's `runtime_build.rs` plus artifact/snapshot pipeline. Preserve Chimera's transparent-proxy constraints and build receipts. Stateful `CoreFacade`/Stop/Handoff migration requires an independent serialized Workflow change, not another partially mutable facade.
