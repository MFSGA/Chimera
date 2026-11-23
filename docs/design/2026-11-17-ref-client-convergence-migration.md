# Ref Client D1: Convergence policy into shared Core

Baseline: `ref` `59724e85c34abb6995c22d101c876cc06dc16576`.

## Direct source mapping

- `ref/backend/nyanpasu-core/src/client/convergence.rs` → `backend/chimera-core/src/client/convergence.rs`.
- The entire upstream health vocabulary, retry-delay table, retry budget, outcome classification, dependency backoff and upstream test are copied to the shared Core module.
- Chimera-only extension: `RetryBudget::record_automatic_attempt` preserves legacy effects and Profile mutation retry accounting. This method and its regression test are kept adjacent to the upstream budget, rather than maintaining a divergent Tauri implementation.
- `backend/tauri/src/client/convergence.rs` now re-exports the Core policy. Consumers in Startup, Profile TCC and Effects keep their existing import path and the same serialized `ConvergenceHealth` values.

## Safety boundary

This is the first **pure business-policy** slice of Client D1, not the ownership migration of `ApplicationWorkflow`, `CoreLifecycleWorkflow` or the stateful `CoreFacade`. No shell IPC, OS effects, Core/Service control, configuration files, UI contract, or submodule gitlinks are changed.

## Verification

- Shared Core unit tests: **347 passed, 0 failed** (includes the two convergence tests).
- Tauri unit tests: **996 passed, 0 failed, 1 ignored** (two convergence tests have moved from Tauri into shared Core).
- The TypeScript binding freshness test passed with the repository's required pnpm toolchain.
- Desktop E2E and real Local/Service daemon operations were not run.

## Next slice

Preserve the pinned ref structure. Move cohesive Client business modules to `chimera-core`, one owner at a time, and replace corresponding Tauri logic with adapters. Treat ref `CoreFacade::PendingAction`, explicit user-stop intent and handoff as one stateful integration task; avoid constructing a second mutable facade or bypassing the serialized Workflow actor.
