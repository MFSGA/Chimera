# UI behavior alignment: marquee and dropdown menu

## Baseline, objective and safety

- Chimera base: local `master` at `d438e217880e542bd75d86aa4e31bf79e1e2ca8e` with a clean worktree before this slice. Work branch: `codex/ref-ui-behavior-20261104`.
- Fixed read-only reference: `ref/` at `5ed3c019e13015fc5b06fc8d7069b330607d9036`, including its nested modules. Do not update the reference pin or rewrite its source.
- Objective: converge the shared scrolling-text implementation and menu surface composition on the reference source, then prevent regressions with directly executed component behavior tests.
- Non-goals: change application IPC, persisted user data, core/runtime, agent privileges, legacy UI behavior, the menu system's business actions or global theme.
- Risk: **L1 UI behavior**, with L2 verification for adding a pinned test-only dependency. The changed shared controls are used across main/legacy pages; regression can affect overflow labels, menu focus and appearance.

## Ref paths and decisions

| Ref path / symbols                                                                                 | Chimera path                                                | Decision                                                                                                                                                                                                                                                                                                   |
| -------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/ui/src/text-marquee.tsx::TextMarquee`                                                    | same                                                        | Copy the entire pinned ref implementation, mapping only `@nyanpasu/utils` to `@chimera/utils` and applying local Prettier. Replace the JS-driven Motion loop with Web Animations API, width measurements on actual text change, out-of-flow secondary copy and visibility pause on the **container**       |
| `frontend/ui/src/dropdown-menu.tsx::DropdownMenu*`                                                 | same                                                        | Copy the complete pinned ref implementation with the package-name mapping. Replace local `MotionContent` with upstream `MenuSurface` for normal and nested menus. Preserve `data-slot="main-dropdown-menu-motion-content"` on both `MenuSurface` instances as a documented **E2E compatibility exception** |
| No upstream equivalent to old extra duplicate                                                      | `frontend/chimera/src/components/main-ui/dropdown-menu.tsx` | Replace identical-but-unused duplicate implementation with a stable `export * from '@chimera/ui/dropdown-menu'` bridge. Current main consumers already import shared UI; the alias keeps legacy deep-import compatibility if needed                                                                        |
| Upstream Vitest browser tests `text-marquee.browser.test.tsx` and `dropdown-menu.browser.test.tsx` | `frontend/ui/tests/ref-ui-behavior.test.ts`                 | Add executable Node/Happy DOM characterization tests; not a replacement for actual Chromium/WebKit or WDIO tests                                                                                                                                                                                           |
| Test dependency                                                                                    | `frontend/ui/package.json`, `pnpm-lock.yaml`                | Add pinned, dev-only `happy-dom@20.8.3`, and register the additional test in `test:unit`; no new production dependency                                                                                                                                                                                     |

The only necessary ref source divergence in `dropdown-menu.tsx` is the pre-existing E2E locator attribute. It is referenced by `tauri-e2e/specs/main-dropdown-menu.e2e.ts` to verify visible 48px items, popup geometry, and the current stable animation state. Dropping the attribute would silently break existing desktop coverage. Main and legacy still use the same shared UI module and IPC.

## Tested contracts

The Node DOM test uses the **real JSX component source** (loaded through Vite) and React's real mounting/effect pipeline, while replacing browser observers, element measurements, Web Animations, and icon modules at the test boundary. It verifies:

1. Overflowing marquee creates one native WAAPI animation with compositor-compatible transform keyframes and an out-of-flow duplicate exactly `textWidth + gap` pixels from the first.
2. `IntersectionObserver` observes the outer, stationary container; offscreen notifications pause animation and onscreen notifications resume it; unmount cancels animation and disconnects observers.
3. Parent re-render with unchanged text does not repeatedly measure width; resizing the containing box so the text fits cancels animation, then shrinking it restarts scrolling.
4. Short, fitting text renders once, stays truncated and creates no animation.
5. A real `DropdownMenu` default-open tree renders its item and the shared menu surface, and retains the existing desktop E2E locator.

**Limitations:** Happy DOM does not measure real text, perform compositor animation, exercise true browser focus/dismissal, prove pixel geometry, or drive real mouse/keyboard events through WebDriver. These behaviors remain subject to the existing desktop menu E2E and upstream browser-test parity. Do not describe this test as visual or desktop E2E.

## Remaining UI behavior differences

- `frontend/ui/src/scroll-area.tsx`: ref exposes `isPastHeader` and defaults to `type="scroll"`; Chimera additionally exposes `offset` for the current settings/providers title components, uses `type="always"`, and styles the thumb shadow. A direct copy would break consumers or change layout. Migrate those title hooks along with scroll-state and viewport browser tests before replacing it.
- `frontend/ui/src/tooltip.tsx`: local zero-delay provider differs from the upstream app-level delayed provider. Align together with page-level TooltipProvider adoption and focus/hover E2E.
- `frontend/ui/src/dnd-grid`, `slider.tsx`, `sparkline.tsx`, and other already-present paths still need semantic, API and visual review. Path coverage alone is not behavior parity.

## Verification and recovery

- Verify `corepack pnpm --filter @chimera/ui test:unit` (new behavior tests + existing 13 tests), `corepack pnpm typecheck`, `corepack pnpm lint:frontend-boundaries`, Oxlint/Prettier, `corepack pnpm install --frozen-lockfile --offline --ignore-scripts`, `git diff --check` and `corepack pnpm --filter chimera-ui build`. Record actual results in the final handoff.
- Real browser and Windows Tauri desktop E2E **not run in this local iteration**. Previously observed Windows E2E failures on the parent branch do not count as evidence about this change.
- Rollback by reverting the work-branch commit(s). This slice changes no user data or host settings and adds no production dependencies. Before merging, review the dropdown menu locator, nested menu interaction, OS reduced-motion behavior and large-list scrolling performance.
