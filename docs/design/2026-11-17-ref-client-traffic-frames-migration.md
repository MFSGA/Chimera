# D1: Move traffic frame normalization and GeoIP attribution to shared Core

## Baseline and scope

- Base: `master` at `c5ae0e11`, pinned read-only `ref` `59724e85c34abb6995c22d101c876cc06dc16576`.
- Upstream source layout: `ref/backend/nyanpasu-core/src/client/traffic_frames.rs` and `client/traffic_frames/geo.rs`.
- Production Chimera source before this migration: `backend/tauri/src/core/traffic/{source,geo,ports}.rs`.
- New production owner: `backend/chimera-core/src/client/traffic_frames.rs` and `client/traffic_frames/geo.rs`.
- Tauri `TrafficActor`, local-address probe, country-index owner, storage, retention, UI projections and status subscriptions remain unchanged. The old Tauri source, geo and local source address locations now re-export the shared implementation and types.

## Intentional Chimera-specific changes from ref

- **Raw connections shape:** pinned ref's `ClashConnectionsFrame` provides typed `Connection` values, whereas Chimera's existing frame has JSON-valued records. Keep `serde_json::from_value::<Connection>` and skip malformed entries individually without discarding valid samples or frame totals. The shared Core test `malformed_json_connections_are_skipped_without_losing_valid_samples` protects this difference.
- **Counter shape:** Chimera's feed totals are already `u64`, so unlike upstream no signed-to-unsigned conversion is needed for snapshot totals. Individual connection counters are still clamped to zero if negative.
- **GeoIP and metadata model:** retain Chimera's `clash_api::ConnectionMetadata` `known` field and existing `chimera-geodata`/`chimera-traffic` types. The upstream country-index selection rules, DIRECT peer-vs-resolution distinction, local address handling and unknown-code fallback remain covered by the migrated tests.
- The `CountryLookup` trait and `LocalSourceIps` DTO are exported for the existing Tauri sampling adapter; there is no second data-policy implementation in Tauri.

## Verification and acceptance

- The shared Core migration carries the previous GeoIP and source-normalizer tests and an extra malformed-JSON regression.
- Tauri actor tests remain in place and must show that connection frames are sampled, persisted and fenced to the correct core/GeoIP generation.
- Required checks: targeted `chimera-core client::traffic_frames`, targeted Tauri `core::traffic`, full `chimera-core` and Tauri unit tests, staged Rustfmt/Clippy and Git diff checks.
- No live VPN, privileged daemon or desktop E2E tests are claimed. Neither ref nor runtime submodule gitlink is changed.

## Next slice

Continue extracting cohesive, reference-aligned Client policy/DTO modules from Tauri into `chimera-core`, with existing Tauri-specific I/O kept behind ports. Do not move the stateful `CoreFacade` mutable owner or explicit user-stop intent piecemeal; those require one workflow transaction boundary and follow-up host-switch tests.
