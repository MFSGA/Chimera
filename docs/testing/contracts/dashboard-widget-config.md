# Dashboard widget configuration contract

## Behavior under test

- **Initial state:** a dedicated desktop E2E runtime opens the main Dashboard in Simplified Chinese. The Traffic Down widget may have a prior saved unit, so the test first restores its default through the widget's visible configuration menu and confirms `B/s` in the rendered rate.
- **Action:** enter Dashboard edit mode with the dashboard context menu, open the Traffic Down widget settings, and click the visible two-option speed-unit toggle to choose `bit/s`.
- **Expected result:** without refreshing, the widget rate changes to the bit unit and the storage write finishes. After a page refresh, the same widget still displays the bit unit. The visible reset action then restores `B/s`.
- **Independent observation:** before refresh, the test reads the stored JSON through the read-only `get_storage_item` command and checks the Traffic Down instance's unit. It separately checks the rendered rate, then refreshes and checks the rendered rate again. The setting change itself always goes through the visible context menu and two-option toggle.
- **Failure path:** first, the test wraps Tauri invoke and rejects one `get_storage_item` for `dashboard-widget-configs`. It verifies the read error, confirms the edit-mode Save action remains enabled, exits and re-enters edit mode, and uses Retry to restore the read. After confirming the default is stored, it injects one `set_storage_item` failure, changes the unit through the visible toggle, waits for the save error, and confirms the backend still contains `bytes`. A `finally` restores the original invoke function; the visible Retry button then saves `bits`, which is checked in the backend and rendered UI before refresh. Only the two failures are mocked; the retry and successful write use the real commands.
- **Failure exposed:** a broken setting control, incorrect unit formatting, blocked edit-mode exit after a read error, a save error reported as success, an incorrect retry, or a setting not read from the backend after refresh fails the corresponding assertion.

This is a desktop UI E2E for widget-level KV storage, failure recovery, and webview-refresh persistence. The read-only backend observer reads the KV store, not an in-memory React value. It does not prove durability across abrupt process termination or persistence across a full application restart.

## Resources and cleanup

The spec is part of the existing `main` suite and runs in its dedicated E2E runtime directory. It snapshots the original raw widget-config KV value, uses the visible Reset control during the test, then restores the exact original KV value and refreshes the page during cleanup on both success and failure. A failed assertion captures a screenshot, page source, and error before cleanup in the configured E2E artifact directory (or the system temporary directory). Cleanup failures are reported alongside the test failure. No user profile, subscription, or host network setting is read or changed.

## Reference and limits

Reference baseline: `ref` commit `685042aaedaea26a591900be0b494dfe8efd79b0`; paths `frontend/nyanpasu/src/components/widgets/widget-config.ts`, `provider.tsx`, `widget-config-menu.tsx`, `widget-config-row.tsx`, `widget-sparkline.tsx`, and `widget-shortcut.tsx`. The E2E extension stays in `tauri-e2e/`. The implementation currently exposes configuration for Chimera's six existing widget types; reference widgets without Chimera data/render contracts remain deferred.

The spec is registered in `tauri-e2e/spec-suites.ts` through `baseE2eSuites.main`, which is included in the PR `hermetic` suite. The hook's dynamic-key switch during an in-flight old-key write is covered by `frontend/interface/tests/kv-storage-write-echoes.test.ts`, which exercises the production echo registry through the A→B→A and inactive-key event sequences. There is no local hook-browser test harness, and this desktop spec exercises the fixed Dashboard config key. Local desktop execution is not available in the current disk budget; report the actual CI result for this exact revision before claiming desktop E2E passed.
