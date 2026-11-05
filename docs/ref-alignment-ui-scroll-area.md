# Ref UI behavior: ScrollArea and sticky page titles

## Baseline and scope

- Chimera baseline: clean `master@d438e217880e542bd75d86aa4e31bf79e1e2ca8e` before this slice.
- Work branch: `codex/ref-ui-scroll-area-20261104`; separate from the pending Draft PR #589, which aligns `text-marquee` and `dropdown-menu`. This is an independently reviewable slice.
- Fixed read-only reference: `ref@5ed3c019e13015fc5b06fc8d7069b330607d9036`; nested gitlinks stay unchanged.
- Objective: align shared scroll state, viewport ref attachment, scroll direction, header threshold and scrollbar structure to the pinned ref, with title consumers migrated to the ref's `isPastHeader` contract.
- Non-goals: change backend, agent, IPC, routes, saved preferences, user data, window entry points, or unrelated legacy presentation code.
- Risk: **L2** because scrollbar visibility and the sticky title transition are visible behavior across many pages; no intentional host or persisted data side effects.

## Ref mapping, ownership and exceptions

| Reference source                                                                     | Chimera source                                                                  | Decision                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ref/frontend/ui/src/scroll-area.tsx`                                                | `frontend/ui/src/scroll-area.tsx`                                               | Copy complete ref source with **only** `@nyanpasu/utils` → `@chimera/utils` and local Prettier. Keep stable viewport ref and viewport-element contexts; replace per-scroll `offset` object with `isPastHeader`, whose boundary is strictly `scrollTop > 40`. Use ref `type="scroll"`, `ScrollBar` classes, scroll state memo and `Viewport` shape |
| `ref/frontend/nyanpasu/src/pages/(main)/main/providers/_modules/providers-title.tsx` | `frontend/chimera/src/pages/(main)/main/providers/_modules/providers-title.tsx` | Change `useScrollArea().offset.top > 40` to `useScrollArea().isPastHeader`. Rest of existing body and routing remain unchanged                                                                                                                                                                                                                    |
| `ref/frontend/nyanpasu/src/components/page-title.tsx` (relevant hook usage)          | `frontend/chimera/src/pages/(main)/main/settings/_modules/settings-title.tsx`   | Change only the scroll-state selection to `isPastHeader`. Preserve Chimera settings-specific title path, responsive back button, accessibility and layout animation. Correct outdated inline implementation note                                                                                                                                  |
| `ref/frontend/ui/tests/scroll-area.browser.test.tsx`                                 | `frontend/ui/tests/ref-scroll-area.test.ts`                                     | Adapt actual-state/viewport ref assertions to Node + Happy DOM, using real React lifecycle and a controlled scroll-height and scroll-event boundary. Test both `ScrollArea` and `AppContentScrollArea`                                                                                                                                            |
| Test dependency                                                                      | `frontend/ui/package.json`, `pnpm-lock.yaml`                                    | Add pinned **dev-only** `happy-dom@20.8.3` and register the four new tests under `test:unit`                                                                                                                                                                                                                                                      |

**Compatibility audit:** `rg` across Chimera main + shared UI found only two app consumers of `useScrollArea().offset`: `SettingsTitle` and `ProvidersTitle`. Both only needed `offset.top > 40`. There were no other offset field consumers. Removing the per-pixel `offset` context state therefore removes a local extension rather than breaking a known consumer. The upstream `useScrollAreaViewport` and `useScrollAreaViewportElement` contracts remain unchanged, as do the route-specific `data-slot` attributes passed to ScrollArea roots.

**Visual/interaction change to review:** The previous Chimera shared ScrollArea defaulted to always-visible scrollbars, with a custom shadow on the thumb. The pinned ref uses Radix `type="scroll"` and an unshadowed thumb. This is intentional ref alignment, but must be reviewed in real Windows/macOS desktop UI, including nested modal scrolling, pointer scrolling, the settings sidebar, table virtualizers and sticky titles. Explicit caller-provided `type` remains forwarded through `...props`.

## Behavior evidence and verification scope

Four Node/Happy DOM component tests exercise the **actual** JSX module and React effects:

1. ScrollArea: exactly 40px does not activate sticky title; 41px does; scroll-top changes within a constant flag state do not re-render context consumers.
2. AppContentScrollArea: same flags and render-count contract.
3. ScrollArea: attached viewport element causes the element-context consumer to see `null` then the real DOM node; stable ref and flag-context consumers are not redundantly re-rendered.
4. AppContentScrollArea: same viewport-attachment contract.

Both scrolling tests also verify direction, top/bottom state and the idle timer.

**The fake DOM does not perform real layout, browser scroll geometry, or scrollbar visibility testing.** It is not WebDriver, Vitest Browser or desktop Tauri E2E. The original pinned reference browser tests are not run here. Real browser and desktop tests remain required for visual acceptance. No new E2E skip, retry or timeout was added.

Final local checks will include frozen offline pnpm install, full `typecheck`, all UI tests, UI test TypeScript strict check, frontend boundary verification, Oxlint, Prettier, `git diff --check` and main/legacy production Vite build. Record actual results in the handoff; do not infer remote CI success from local results.

## Rollback and remaining work

Revert this isolated branch's commit to restore the former `offset` shape, scroll-area implementation and two title consumers together; the dev-only testing package is removable without production effects. No database, backend config, submodule or host settings migration is required. No tag, release or remote `master` rewrite is authorized.

After this slice, review `tooltip.tsx`'s hover-delay/provider behavior and the `dnd-grid`/slider/sparkline implementation differences. PR #589's pending UI behavior changes should be reviewed and merged independently from this scroll-area PR.
