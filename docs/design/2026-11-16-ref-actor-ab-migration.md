# CoreActor / ServiceActor reference alignment (A + B)

## Scope and baseline

- Chimera base: `1f06b55ceb4b68048305ca85465cd97c991d8e71` (`master` / `origin/master` before this uncommitted migration).
- Pinned read-only ref: `59724e85c34abb6995c22d101c876cc06dc16576`.
- Risk: L2 — runtime actor ownership and API compatibility.
- Goal: align the **shared actor implementations and their source paths** with the pinned ref before the separate Client/Workflow composition migration.
- Non-goals: changing Startup/Workflow, altering user configuration, migrating the Tauri client facade, changing external RPC, or updating the ref gitlink.

## File and symbol correspondence

| Pinned ref | Chimera | Decision |
|---|---|---|
| `backend/nyanpasu-core/src/actor_v2/mod.rs` (`CoreActor`, `CoreClient`, projections, handoff) | `backend/chimera-core/src/actor_v2/mod.rs` | Keep the already-aligned core router body at its upstream module path; `router.rs` re-exports the public API for current callers |
| `backend/nyanpasu-core/src/actor_v2/service_actor.rs` (`ServiceActor`, `ServiceClient`, ready generation, bounded restart) | `backend/chimera-core/src/actor_v2/service_actor.rs` | Keep the already-aligned actor implementation at its upstream path; `service_router.rs` re-exports it for current callers |
| `backend/nyanpasu-core/src/actor_v2/facade.rs` (`CoreFacade`, reconcile, handoff, pending actions, reports) | `backend/chimera-core/src/actor_v2/facade.rs` | Copy the full ref implementation and tests, map crate brands, and adapt only the Chimera-specific test wire model, service version gate, and `StopIfOwner` |
| No extra privileged service command actor in ref | `backend/chimera-core/src/actor_v2/service_command_actor.rs` | Move the existing legacy service command lane away from the canonical ServiceActor path; remain only for current Tauri compatibility |
| Ref `api.rs`, `endpoint.rs`, `local_host.rs` | Chimera core `api.rs`, `endpoint.rs` and Tauri local host adapters | Not yet structurally identical: retain the existing protocol and OS adapters pending the cross-crate Client/Core migration |

## Intentionally retained differences and convergence

- **Chimera transparent proxy:** the CoreActor still admits the Chimera-specific, serialized network mutation and retains the fail-closed uncertain-result handling. Preserve this capability until it has a reviewed reference-compatible port or contract.
- **Chimera service restart:** the canonical ServiceActor retains `RestartDaemon`, because the product's service command supports one native restart. It must remain serialized and probed; do not emulate it with an unguarded stop/start pair.
- **API/endpoint compatibility:** the existing `CoreApiEndpoint`, instance-bound authority checks, and `CoreStatusSnapshot.instance_id` differ from pinned ref. No API semantics or persisted wire values were silently rewritten.
- **Legacy Tauri facade:** `backend/tauri/src/core/actor_v2/facade.rs`, `ServiceCommandPort`, and `LegacyServiceBridge` remain live. The new core `CoreFacade` is available for the subsequent ApplicationWorkflow composition step, but is **not** proof production has switched owners. C/D must replace the legacy path as one reviewed end-to-end change, after verifying both main/legacy UI and agent.
- **Compatibility imports:** `router.rs` and `service_router.rs` are temporary public-path reexports; remove only after consumers migrate to canonical actor paths.

## Acceptance and verification

- Actor module references and all in-crate tests compile against the canonical `mod.rs` and `service_actor.rs`.
- Ref `CoreFacade` tests run against the same Chimera `CoreClient` and `ServiceClient` implementations.
- Test fixture changes are **only** for Chimera's status DTO / service version and the added `StopIfOwner` command; the ref facade's production state machine remains copied.
- Validate `chimera-core` unit tests, the Tauri crate compile, and any affected Tauri actor tests. Rust unit tests cannot prove privileged daemon operations, desktop host effects, or real TUN.
- No edits to the five pre-existing Startup/Workflow/LESSONS.md modifications. No `ref/` or runtime submodule update.

## Executed verification (2026-11-16, macOS arm64)

- `cargo test --manifest-path backend/Cargo.toml -p chimera-core --lib -- --test-threads=4`: **345 passed, 0 failed**. Includes copied ref facade tests and the Chimera-only native Service restart command-boundary test.
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib --no-run`: compiled successfully.
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib core::actor_v2 -- --test-threads=4`: **41 passed**.
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib setup::actor_tests -- --test-threads=4`: **6 passed**.
- Scoped `rustfmt --check` on modified Rust files and `git diff --check`: passed.
- No full desktop E2E, privileged daemon / TUN test, independent review, or remote CI has been claimed.

## Integration and rollback

This is the **A/B actor implementation** phase, not final lifecycle or application-core adoption. The next composition phase should wire the copied `CoreFacade` into the application workflow, replace the Tauri-owned facade, and remove the old command lane only after its production consumers are gone. Keep a genuine test for service readiness, manual stop intent, generation changes, and shutdown under the new owner.

Changes are local and not pushed. Rollback is the reversal of the A/B-scoped source and documentation changes only. Do not reset or clean unrelated Startup/Workflow work.
