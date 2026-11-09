# Ref UI: dashboard Sparkline compositor migration and DnD Grid contracts

## Baseline and scope

- Chimera base: `master@8d903008bbd1034bc69d61513f5fbe14aaa489b5`.
- Read-only reference: `ref@911644c8b` (`libnyanpasu/clash-nyanpasu` main).
- Risk: L2. The shared Sparkline changes its DOM root from `svg` to a `div`, and moves streaming animation from a JavaScript animation loop to Web Animations API (WAAPI).
- Scope: `frontend/ui/src/sparkline.tsx`, its unit/DOM/SSR contracts, and pure DnD Grid geometry regression tests. No dashboard persistence, backend, IPC, or DnD implementation changes.

## Sparkline source and compatibility

- `frontend/ui/src/sparkline.tsx` follows the latest ref implementation. The only behavioral dependency adaptation is using the already direct `lodash-es/isEqual` instead of `es-toolkit/isEqual`; for this numeric-array content comparison the semantics are equivalent. The ref's `@nyanpasu/utils` import is mapped to `@chimera/utils`.
- The existing `d3` dependency is retained at `7.9.0` rather than changing it to ref's `7.9.2`; the component uses stable D3 APIs. No dependency or lockfile change is required.
- The dashboard widget consumer passes only `data` and `className`, so no widget JSX change is needed. `frontend/chimera/src/components/dashboard/dataline.tsx` imports the root-exported Material You Sparkline and is intentionally unaffected. The legacy Material You implementation remains untouched.

## Behavior now covered

- Equal-content arrays do not reset a running animation when websocket updates rebuild arrays.
- Append-only sliding windows use WAAPI: horizontal translation is linear, vertical rescaling is independent, and an update arriving mid-animation continues from the current fractional position.
- `ResizeObserver` owns size updates; ordinary React re-renders do not read layout. Resize redraws the path without waiting for a new data sample.
- Empty/short histories settle safely, the first nonzero peak avoids a zero-scale stretch artifact, and unmount cancels animations and disconnects the observer.
- DnD Grid source is unchanged. The added pure geometry contract covers overlap boundaries and resize constraints only; it is not a pointer, keyboard, or desktop drag test.

## Verification and limitations

- `@chimera/ui` unit suite: 43 passed, 0 failed. This includes the Sparkline SSR contract, three Happy DOM/WAAPI lifecycle tests, and three DnD geometry tests.
- Full workspace `pnpm typecheck`: passed, including UI, main app and Tauri E2E types.
- Prettier, Oxlint on changed Sparkline files, and `git diff --check`: passed.
- Happy DOM stubs verify DOM lifecycle and animation scheduling, not actual browser compositor timing or CSS layout. A new Dashboard Tauri E2E now waits for the four visible Sparkline roots to receive ResizeObserver measurements, then asserts the `div` root, SVG, line/area paths and hidden overflow in the real WebView2. Its GitHub run is still pending; it does not yet drive a new sample to prove compositor timing. The upstream Vitest browser suite's mid-animation/resize timing checks remain a follow-up.
- No persisted layout or runtime data is changed. Rollback is an isolated revert of the Sparkline component and its added tests.
