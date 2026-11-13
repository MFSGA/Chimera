# Window first-render and startup ref alignment

## Baseline and scope

- Pinned local reference: `ref` commit `59724e85c34abb6995c22d101c876cc06dc16576` (examined without updating it); its nested `backend/nyanpasu-runtime` is already dirty.
- Scope: the first window reveal and lifecycle chain, background core-initialization separation, and the upstream typed window-close configuration. This is an L2 architectural migration, not a redesign of the rule page, core API or Service protocol.
- Acceptance: main, retained legacy, and editor windows share one window manager; a webview reports its own readiness; an absent report is bounded by a 10-second watchdog; close/reopen and stale-apply ordering follow the reference table.
- Excluded: silently enabling system proxy/TUN, changing host configuration, committing or pushing without a dedicated instruction, or changing existing legacy settings keys.

## Reference-to-Chimera mapping

| Pinned ref | Chimera | Result |
| --- | --- | --- |
| `backend/tauri/src/window/table.rs` | `backend/tauri/src/window/table.rs` | Exact source copy; 12 table tests |
| `backend/tauri/src/window/manager.rs` | `backend/tauri/src/window/manager.rs` | Central create/build/watch/ready/close/apply/watchdog with `WindowTable<Arc<dyn WindowHooks>>` |
| `backend/tauri/src/window/kinds.rs` | `backend/tauri/src/window/kinds.rs` | Main and Editor plus required Legacy kind; retain original labels, paths and dimensions |
| `backend/tauri/src/window/mod.rs` | `backend/tauri/src/window.rs` | `AppWindow`, `WindowHooks`, URL parameter builder, window state capture |
| `backend/tauri/src/window/engine.rs` | `backend/tauri/src/window/engine.rs` | WebView2 builder, first-ready settings and visibility control |
| `backend/nyanpasu-config/src/application/window_close.rs` | `backend/chimera-config/src/application/window_close.rs` | Upstream enums, nested patch model and default destroy, with the typed app config as owner |
| `frontend/nyanpasu/src/pages/(main)/main/settings/nyanpasu/tray/_modules/window-close-behavior.tsx` | `frontend/chimera/src/pages/(main)/main/settings/chimera/_modules/window-close-behavior.tsx` | Per-window override UI with Chimera main/legacy and editor kinds; tray row deliberately omitted |
| `backend/tauri/src/utils/resolve.rs` | `backend/tauri/src/utils/resolve.rs` | Main/legacy selection delegates to manager; startup Service/port/core work moves to a worker |
| Upstream `reportWindowReady` RPC and frontend route mount | `ipc.rs`, `specta_export.rs`, `frontend/interface/src/ipc/bindings.ts`, `frontend/chimera/src/pages/__root.tsx` | Authenticated per-webview readiness, typed IPC and startup feedback |

## Retained differences / convergence criteria

1. **Startup reconcile:** pinned ref still blocks `resolve_setup` with `startup_reconcile` and documents an unfinished TODO. Chimera's legacy Service → port → `init_core` path is spawned after window creation; pre-window `init_resources`, storage setup, and outer Tauri application setup remain synchronous. Remove that residual blocking only after verifying construction dependencies and real cold-start timing.
2. **Close settings (updated in the next iteration):** the ref model is now stored in Chimera's typed `application.yaml` and read by `WindowManager`. Missing fields default to `Destroy` to preserve existing behavior; the typed nested patch API supports `global`, `main`, `editor`, and `tray_menu`. Chimera keeps its previous `tray_menu_close_behavior` compatibility setting, and does **not** expose the new `tray_menu` override in UI until the WebView tray window is owned by `WindowManager`. `backend/tauri/src/window/close.rs` is a temporary re-export shim to remove when Chimera's `window.rs` becomes ref-style `window/mod.rs`.
3. **Geometry:** ref queues saves through its SessionState actor. Chimera's existing `RunEvent::WindowEvent::CloseRequested` path still invokes `save_main_window_state` before destruction. Do not replace it with a fire-and-forget task until an ordered, shutdown-aware queue is available; otherwise geometry may be lost.
4. **Legacy UI:** intentionally retained as a distinct window kind but uses the same lifecycle manager and business application APIs. Editor labels and routes remain stable (including percent-encoded UID).
5. **Platform details:** Chimera retains its existing macOS traffic-light placement and Windows transparent window handling where the pinned ref differs. Verify these visually on each target platform before removing compatibility code.
6. **Startup status:** `AppStartupStatus` represents only the initial GUI/core initialization task; it is not another authority for live core lifecycle. Runtime status still comes from the existing client.
7. **Deep links:** current process-local readiness tracking is kept for activation dispatch and remains separate from the manager's display watchdog. Reconcile it with upstream event delivery after a reload/secondary-activation regression test.

## Verification evidence (local, non-destructive)

- `rustfmt --check --edition 2024 --config skip_children=true` on changed Rust files: passed.
- `git diff --check`: passed.
- `cargo check --manifest-path backend/Cargo.toml -p chimera --lib -q`: passed with existing warnings.
- `pnpm typecheck`: passed.
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib window:: -- --nocapture`: 20 passed, 0 failed (including reference state-table races and editor URL encoding).
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib specta_export::tests::typescript_bindings_are_fresh -- --nocapture`: passed.
- `pnpm --filter @chimera/tauri-e2e test:unit`: 44 passed, 0 failed.
- The additional real Windows main/legacy visibility E2E assertion has been authored **but has not run**. None of these unit tests alone proves actual first paint, fallback presentation after a hung frontend, cold-start time, or graceful close persistence.

## Follow-up iteration (2026-11-13, no tests requested)

- Added `WindowCloseSettings` to `ChimeraAppConfig` with nested patches, preserved legacy fields and default `destroy` semantics, and wired it into the manager's close decision via the typed `ChimeraClient` config snapshot.
- Added main UI controls and translations for the default, main/legacy, and editor close behaviors. TypeScript binding declarations were **updated manually** because the user requested no tests, so the Specta generator/freshness check was **not run** during this iteration.
- Regenerated Paraglide messages and ran compile/type checks (`cargo check -p chimera --lib`, `pnpm typecheck`), both passed. **No unit, integration, or desktop E2E tests were run in this iteration.** Earlier test evidence above applies to the preceding iteration only, not the new close-policy feature.
- Before release, regenerate TypeScript bindings using the project's Specta generator and validate nested patch serialization and main/legacy/editor hide/reopen behavior; this remains explicitly unverified by runtime tests.

## Lock and shutdown race fix (2026-11-13)

- **Window creation deadlock:** `WindowManager::create` previously held the non-reentrant `WindowTable` mutex across `start_fallback`, which takes that same mutex for watchdog registration. This could deadlock the initial window build. The `built(label)` transition is now stored in a local variable, dropping the guard before starting the watchdog, as in `ref/backend/tauri/src/window/manager.rs` at the pinned commit.
- **Startup/exit race:** the previous Service → port → core startup task was spawned without `AppShutdown.tasks` ownership, allowing the exit boundary to finish draining while the task was still preparing port files or starting the core. It is now tracked by `AppShutdown.tasks` and observes the corresponding cancellation token before blocking preparation, between Service/port steps, and after joining the blocking worker. If cancellation occurs during preparation, the join completes before shutdown cleanup and subsequent core reconciliation is skipped.
- **Already-running lifecycle commands:** after `init_core` is issued to the lifecycle actor, the tracked startup task awaits its terminal result even if shutdown is requested. The existing exit path waits for tracked tasks *before* calling `shutdown_core`, preserving ordering. This deliberately favors correctness over immediate exit when Service/core startup is unusually slow; an actor command that never resolves could still delay shutdown.
- **Targeted verification:** `cargo test --manifest-path backend/Cargo.toml -p chimera --lib startup_shutdown_tests -- --nocapture`: 2 passed. `cargo test --manifest-path backend/Cargo.toml -p chimera --lib window:: -- --nocapture`: 20 passed. The tests exercise the cancellation/drain barrier and table transitions; no real GUI cold start or process-exit E2E was run in this repair.
- **Scope:** the two implementation files for this repair are `backend/tauri/src/window/manager.rs` (still an untracked file from the previous migration) and `backend/tauri/src/utils/resolve.rs`. Other dirty CI, script, frontend, and `ref` files are not part of this repair. No commit/push or branch movement authorized.

## Remaining acceptance / rollback

Use an isolated desktop runner/VM to exercise fresh cold start, initial window focus, main/legacy switch, close/reopen, editor URL with special characters, frontend failure and the ten-second fallback, Deep Link delivery, and graceful exit. Capture failure-time screenshots/logs. Run across supported platforms where possible, without modifying the user's live network configuration. Revert only this task's window/IPC/frontend/E2E files as a scoped rollback; leave existing CI/Node changes and the `ref` submodule untouched.
