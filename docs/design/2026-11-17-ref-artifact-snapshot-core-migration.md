# Client D1: Ref Artifact Snapshot mapping in shared Core

## Reference and ownership

- Current base: Chimera `master` at `236213b4`; pinned ref gitlink `59724e85c34abb6995c22d101c876cc06dc16576`.
- Upstream: `ref/backend/nyanpasu-core/src/client/artifact_snapshot.rs`.
- Prior Chimera mapping: `backend/tauri/src/enhance/artifact_bridge.rs` plus the legacy `PostProcessingOutput` DTO in `enhance/chain.rs`.
- New pure mapping: `backend/chimera-core/src/client/artifact_snapshot.rs`, returning `ArtifactSnapshotData` containing the YAML mapping, ordered applied field keys, postprocessing output, and inspection graph data.

## What moved

The shared module now owns `LogSpan`, `Logs`, `PostProcessingOutput` and the mapping of scoped/global/builtin `StepLog` records to those frontend output keys, along with the complete `RuntimeArtifact` -> `RuntimeInspectionData` conversion. `enhance/chain.rs` re-exports the same types, and `enhance/artifact_bridge.rs` delegates to the shared conversion after resolving the builtin names. `RuntimeBuilder` call sites and the Tauri IPC output contract remain unchanged.

## Intentional Chimera differences from pinned ref

- The pinned ref writes runtime config as `config_text` and uses a different stored graph model. Chimera currently expects `serde_yaml::Mapping` plus the in-memory `ConfigSnapshotsGraph`. This slice keeps both formats unchanged and rejects a graph with no Finalizing node, as before.
- Pinned ref `PostProcessingOutput` also has an `advice` collection; Chimera's pre-existing wire format has only `scopes` and `global`. Nontransform step log entries are not exposed through that DTO in this migration. Adding `advice` would be a **separate explicit API change** and is not silently mixed into an architecture migration.
- The old frontend profile UID keys are strings. The shared DTO preserves them as strings, rather than replacing JSON object keys with strongly typed `ProfileId` values.
- `builtin_transforms_for`, script execution, profile content capture, Chimera Client's MMDB extension, TUN flavor and transparent-proxy validation stay in `enhance/runtime_builder.rs`. The Tauri adapter supplies builtin names to the pure mapper; the shared Core does not depend on or load OS resources or built-in script files.

## Regression acceptance

The shared Core tests verify mapping of scoped/global/builtin logs and fallbacks, serialization without an extra advice field, stable applied field order, Finalizing graph selection, and refusal to publish a graph without Finalizing. Existing Tauri Runtime Builder tests guard transparent capture, MMDB, TUN and transform errors; the full Tauri suite verifies the TypeScript bindings and previous runtime consumers.

This work does **not** migrate the runtime builder/executor, the stateful CoreFacade, Service/Local handoff, persisted runtime snapshots or actual privileged Service/TUN E2E. Those require separate owner-boundary changes and live acceptance checks.
