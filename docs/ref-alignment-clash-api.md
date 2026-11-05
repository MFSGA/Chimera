# Clash API crate ref-alignment record

## Scope and decision

- Objective: align Chimera's application-side Mihomo controller API crate with
  the equivalent implementation in the pinned local reference.
- Non-goals: do not delete the API crate, change or advance the runtime
  submodule, change stored user data, or alter host networking.
- Reference: `ref/` commit
  `5ed3c019e13015fc5b06fc8d7069b330607d9036` (clean worktree at inspection).
- Risk: L2. The crate defines controller request/response models, WebSocket
  connection decoding and types exported through Tauri IPC to both UIs.
- Decision: retain and align `backend/chimera-clash-api`. Ref contains the
  corresponding `clash-api` crate, and the Tauri application imports the local
  crate directly. Deleting it without first replacing all of those imports and
  contracts would both diverge from ref and break the application build.
- Status: **production REST/WebSocket path migrated; platform/runtime
  verification remains pending**. Production calls now use
  `chimera_clash_api::Client` through the existing IPC-DTO adapter. A cached,
  instance-bound capability is owned by `CoreFacade`; endpoint change events
  and periodic authority checks revoke it and cancel active requests/streams.

## Reference mapping

| Reference                                                                              | Chimera                                                                               | Alignment                                                                                                                                                                                                                                  |
| -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ref/backend/nyanpasu-runtime/crates/clash-api/Cargo.toml`                             | `backend/chimera-clash-api/Cargo.toml`                                                | Keep the Chimera package identity/version and required workspace configuration; carry over the reference API dependencies, test structure, and feature-gated allocation examples.                                                          |
| `ref/backend/nyanpasu-runtime/crates/clash-api/src/**`                                 | `backend/chimera-clash-api/src/**`                                                    | Port the complete API crate source and preserve the reference module/file and non-brand symbol structure. Brand map: `nyanpasu_serde` → `chimera_serde`, `nyanpasu_utils` → `chimera_utils`.                                               |
| `ref/backend/nyanpasu-runtime/crates/clash-api/tests/**` and `examples/**`             | `backend/chimera-clash-api/tests/**` and `examples/**`                                | Port the associated decoding, REST, stream, transport, and Specta contract tests and examples.                                                                                                                                             |
| `ref/backend/nyanpasu-core/src/actor_v2/api.rs::ApiClient, ApiLease`                   | `backend/tauri/src/core/clash/api.rs::ApiClient, ApiLease`                            | Use the shared typed client, exact `CoreApiConnection` binding, pre/post request validation, event + periodic revocation, and bounded cancellation. The Chimera module retains DTO conversion as a temporary compatibility adapter.        |
| `ref/backend/nyanpasu-core/src/client/clash_api.rs`                                    | `backend/tauri/src/client/clash_api.rs`, `core_lifecycle/ports.rs`                    | Acquire the bound client through `CoreLifecyclePort`; `LegacyCoreBridge` delegates to `CoreFacade`, with a manager-only fallback retained for tests.                                                                                       |
| `ref/backend/nyanpasu-runtime/crates/nyanpasu-serde/**` and `nyanpasu-serde-derive/**` | `backend/chimera-serde/**` and `backend/chimera-serde-derive/**`                      | Add the upstream generic serde derives required by the API models, applying only the package/crate brand mapping and retaining license notices.                                                                                            |
| Reference workspace `[patch.crates-io]` entry for `reqwest-websocket`                  | `backend/vendor/reqwest-websocket/**` and `[patch.crates-io]` in `backend/Cargo.toml` | Use the reference's vendored implementation: API streaming calls `into_native_parts()`, which is absent from the published `reqwest-websocket 0.6.0` dependency. Preserve the upstream license and record the local patch in `PATCHES.md`. |
| `ref/backend/tauri/src/core/traffic/{geo,source}.rs` and `core/clash/ws.rs` consumers  | Same files under `backend/tauri/src/`                                                 | Read known metadata through the reference `ConnectionMetadata.known` shape. Keep consumers on the shared typed API model.                                                                                                                  |
| Reference Specta export of API models                                                  | `backend/tauri/src/specta_export.rs` → `frontend/interface/src/ipc/bindings.ts`       | Regenerate with the existing ignored generator test; check freshness separately. Do not hand-edit generated bindings.                                                                                                                      |

## Interfaces and differences

- The Tauri application directly depends on `chimera-clash-api`; production
  REST and WebSocket traffic now uses its shared `Client`. The adapter in
  `backend/tauri/src/core/clash/api.rs` retains existing IPC-facing DTOs and
  response projections, while `CoreFacade` caches the client against the exact
  Local/Service `CoreApiConnection`. A changed/unavailable binding revokes the
  shared capability; lifecycle notifications and a two-second authority check
  also cancel in-flight bodies and idle streams. Both Main and Legacy UI
  continue to use the same generated application IPC contract.
- Connection metadata's known fields now live under `ConnectionMetadata.known`,
  as in ref. Unknown Mihomo fields continue to be decoded from the flattened
  wire payload and represented by the reference `extra` maps; serialization for
  application IPC follows the reference `_extra` representation.
- Necessary product differences are the `chimera-*` crate/package identities,
  application workspace paths, and the corresponding Chimera utility/serde
  names. No protocol or persisted preference keys were intentionally renamed.
- The runtime submodule is a separate workspace and remains at its existing
  gitlink. This slice aligns the root application's API crate and typed
  connection-model consumers only; it does not synchronize or rewrite the
  Service/runtime copy.
- Remaining convergence: move the instance-bound capability wrapper into the
  ref-matching `core/actor_v2/api.rs` path and remove the temporary DTO adapter
  only after existing IPC callers consume shared typed models without changing
  generated contracts. The Chimera-only opt-in WebSocket `token` query remains
  necessary for the existing Clash RS compatibility path; standard bearer-header
  behavior stays the API crate default. Real-core, cross-platform transport,
  and ServiceEndpoint lifecycle-monitor integration checks remain unverified.

## Acceptance and rollback

- Acceptance for the crate and production-request slice: app-side crate source
  and tests match the ref file layout; production REST and WebSocket requests
  use the new client under an instance-bound lease; existing IPC DTOs remain
  stable; generated bindings are fresh. Platform-specific, real-core, and
  desktop E2E coverage still needs separate verification.
- Rollback: revert this slice's API, serde helper, vendor patch, Cargo workspace
  and lockfile, Tauri consumer, generated binding, and this record together.
  No runtime state or user data is migrated by this change.
- Remaining coverage: the Mihomo and multi-core integration cases requiring
  externally supplied core binaries are not covered by the default crate test
  run. Platform/runtime verification beyond the current Windows host remains
  unverified.

## Verification evidence

- `cargo test --offline --locked --manifest-path backend/Cargo.toml -p
chimera-serde -p chimera-serde-derive -p chimera-clash-api`: 77 passed, 0
  failed, 2 ignored. The ignored tests require the real Mihomo binary or the
  Mihomo/Meow/Clash RS core matrix.
- Tauri Specta `update_typescript_bindings`: 1 passed. The separate
  `typescript_bindings_are_fresh` check: 1 passed; bindings were generated by
  the existing Rust generator.
- Tauri connection-detail test
  `detail_rates_use_elapsed_seconds_and_never_keep_closed_connection_counters`:
  1 passed, including the updated `_extra` serialization assertions.
- `pnpm typecheck`: passed. `pnpm lint:frontend-boundaries`: passed.
- `cargo metadata --offline --locked --filter-platform
x86_64-pc-windows-msvc --manifest-path backend/Cargo.toml --format-version 1`:
  passed. The unfiltered offline metadata command could not complete because
  `android_system_properties v0.1.6` is not cached on this machine; non-Windows
  dependency metadata remains unverified here.
- Final integration verification on this Windows host: `cargo check
--manifest-path backend/tauri/Cargo.toml --tests` passed;
  `cargo test --manifest-path backend/tauri/Cargo.toml --lib
core::clash::api::tests:: -- --nocapture` passed (6 passed, 0 failed),
  including in-flight controller revocation, upstream stream drop on receiver
  closure, and revocation during backpressured stream delivery.
  `cargo test --manifest-path backend/Cargo.toml -p
chimera-clash-api` passed (62 passed, 2 ignored, 0 failed); ignored tests
  require externally supplied real core binaries.
- Independent review against the pinned reference found no remaining concrete
  production issue after verifying both receiver-close and backpressured-send
  cancellation. ServiceEndpoint lifecycle monitoring and real-core/cross-platform
  runtime behavior remain unverified.
- `pnpm lint:rustfmt`: passed. `pnpm lint:clippy`: passed for all targets and
  features on Windows (existing workspace warnings only). `git diff --check`:
  passed.
- Rust test builds emitted existing workspace warnings; no warning was treated
  as a test failure. No real-core, main/legacy desktop E2E, or cross-platform
  runtime test was run in this slice.
