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
3. **Geometry (updated):** main and legacy window dismissal now queues captured geometry through the typed SessionState Actor rather than blocking the Tauri event thread. The actor serializes and persists the save; graceful exit issues a read barrier and writes the legacy Verge mirror. Other legacy IPC and active-window shutdown persistence still use the existing async compatibility path. Native E2E close/reopen and exit timing remain to be verified.
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

## SessionState window geometry follow-up (2026-11-13)

- Migrated main/legacy `AppWindow::on_dismissed` to `queue_window_state_save` and added actor `SaveMainWindow` messages, matching the pinned ref's queued write semantics while preserving Chimera's dual UI.
- Removed the synchronous `block_on(persist_window_state(...))` from `lib.rs` `RunEvent::WindowEvent::CloseRequested`. The event handler now only logs the close; window manager hooks capture geometry before hide/destroy.
- Added `flush_main_window_geometry_saves()` to graceful cleanup. It sends a `Get` read-barrier behind pending geometry messages and then persists the legacy `verge.yaml` mirror before `shutdown_core()`. The shutdown barrier does not use the normal read timeout, so a slow queued persistence cannot be mistaken for a completed flush.
- Added a session-state actor regression test for ordered queued updates, preservation of unrelated window state, and typed file persistence. Rust compilation and the targeted queued-save and window unit tests pass. Real Windows graphical E2E remains unverified.
- The existing legacy IPC `save_window_size_state` path is retained for compatibility, but its acknowledged save now uses the same `SaveMainWindow` Actor message as queued dismissals. The old whole-map snapshot-and-patch could overwrite a previously queued geometry update; this interleaving is removed.
- Main and legacy window creation now restores geometry from the typed SessionState snapshot when the client is available (the legacy Verge fallback applies only before the client is registered). Exit cleanup only captures a currently visible window, so hidden/unready windows do not overwrite geometry already queued on dismissal; no monitor capture no longer deletes the saved geometry.
- Added regression cases for queued-then-acknowledged saves preserving other window entries, the shutdown barrier waiting for queued geometry to become durable, and conversion between typed and legacy geometry shapes. `cargo fmt --manifest-path backend/Cargo.toml -p chimera -- --check` and `git diff --check` passed. The full `cargo test --locked --quiet --manifest-path backend/Cargo.toml -p chimera --lib` suite passes (845 passed, 1 ignored). Real Windows graphical E2E remains unverified.
- The shutdown read barrier confirms earlier actor messages have been processed in order. A fire-and-forget geometry write failure is logged by the actor and is not returned by the following `Get`; this retains the pinned ref's queued-save error handling rather than claiming the barrier independently proves every write succeeded.
- In the same uncommitted tree, `resolve_setup` has been updated to use `TaskTracker::spawn_on` outside Tokio's thread context. It must receive `tauri::async_runtime::handle().inner()` (Tokio Handle), not the outer Tauri RuntimeHandle. Both occurrences were corrected after the initial E0308 diagnostics; the corrected startup scheduling test passed locally after the shared build lock cleared.

## Push CI failure triage and repair (2026-11-13)

- Remote `master` commit `41ed1dd855b560f825385869a4431e6bccfe1e4c` is verified signed. Its push-run CI failed on all three platforms: [CI run 43432488898](https://github.com/MFSGA/Chimera/actions/runs/43432488898).
- The CI failures are the same `chimera-config` partial patch deserialization bug on Ubuntu, Windows, and macOS: `application::tests::patch_honours_legacy_aliases` and `update_channel_patch_preserves_clear_and_set_semantics` failed with `missing field window_close`. Matched the pinned ref's application struct `#[patch(attribute(serde(default)))]` and added it to `ChimeraAppConfig`. Added a regression for omitting `window_close` and nested editor-only changes. Targeted `chimera-config` application test group: 5/5 passed locally; full `chimera-config --lib` test suite also passed 233/233.
- [Tauri E2E push run 43432488787](https://github.com/MFSGA/Chimera/actions/runs/43432488787) failed during Windows smoke startup. The actual diagnostic log records a Tokio reactor panic at `tauri/src/utils/resolve.rs:213` when native Tauri setup called `TaskTracker::spawn` outside an entered Tokio runtime. The diagnostic ZIP contains no failure-time screenshot. The fix uses `TaskTracker::spawn_on(..., tauri::async_runtime::handle().inner())`, matching the pre-existing storage listener pattern, and adds a synchronous-thread scheduling regression test. The new startup scheduling test passes locally, as does the full `startup_shutdown_tests` group (3/3). The full `chimera --lib` Rust test suite passed 844/844 (1 ignored), including Specta TypeScript bindings freshness. The actual Windows E2E run has not been repeated.
- A local `cargo check` for the geometry migration passed; the queued geometry persistence and window unit tests also passed. The startup/config fixes were later committed and pushed as signed `4601f561189abecc32407d012542c0d168e603b8`. Its CI run [43505230603](https://github.com/MFSGA/Chimera/actions/runs/43505230603) succeeded. Its Tauri E2E run [43505230592](https://github.com/MFSGA/Chimera/actions/runs/43505230592) failed in smoke (1/5 specs passed): logs include a main-frontend `No locale found` error and unavailable sidebar controls. The diagnostics artifact download stalled, so failure-time screenshots were not inspected and the cause remains unconfirmed. This did not exercise geometry persistence; real Windows close/reopen E2E remains unverified.
- D: became full during Rust recompilation (`os error 112`); a narrowly scoped `cargo clean -p chimera` recovered 27.3 GiB of generated target artifacts. A subsequent cargo test hit missing build fingerprint paths while another cargo process apparently rebuilt or modified shared `backend/target`. No source files, `pnpm-lock.yaml`, or dirty `ref` content were deleted.

## Remaining acceptance / rollback

Use an isolated desktop runner/VM to exercise fresh cold start, initial window focus, main/legacy switch, close/reopen, editor URL with special characters, frontend failure and the ten-second fallback, Deep Link delivery, and graceful exit. Capture failure-time screenshots/logs. Run across supported platforms where possible, without modifying the user's live network configuration. Revert only this task's window/IPC/frontend/E2E files as a scoped rollback; leave existing CI/Node changes and the `ref` submodule untouched.
