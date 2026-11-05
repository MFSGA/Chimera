# Frontend UI primitives alignment: progress, switch and missing ref modules

## Scope and source

- Reference: `ref/` commit `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`; clean worktree.
- Target: `frontend/ui/` on Chimera `master`; the pre-existing working tree already contained the glass-controls slice documented in [ref-alignment-glass-controls.md](ref-alignment-glass-controls.md).
- Objective: compare exact file paths and source dependencies, port upstream visual primitives and align the existing progress/switch implementation while preserving supported legacy UI exports.
- Explicit non-goals: changing the backend/IPC, persisting or migrating user data, rewriting theme packages, changing app navigation/adoption of newly ported controls, replacing legacy Material You widgets, or touching the pinned reference checkout.
- Risk: L1, shared UI component logic and rendering. The shared `LinearProgress` appearance changes to ref's h-3 `bg-secondary-container` model. No system, network, authorization or data side effects.

## Reviewed path/symbol mapping and decisions

| Reference file/symbol                                               | Chimera file/symbol                                   | Action / status                                                                                                                                             |
| ------------------------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/ui/src/progress.tsx::{CircularProgress,LinearProgress}`   | same                                                  | Align ref implementation, separate determinate and indeterminate states, preserve circular semantics                                                        |
| no ref equivalent                                                   | `frontend/ui/src/linear-progress.tsx::LinearProgress` | Preserve compatibility for direct imports; do not delete the existing subscription UI bar                                                                   |
| `frontend/ui/src/switch.tsx::{Switch,SwitchItem}`                   | same                                                  | Align ref structure and loading `data-slot` attributes; leave legacy root `SwitchItem` binding                                                              |
| `frontend/ui/src/action-swap-text.tsx::{ActionSwap,ActionSwapText}` | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/kbd.tsx::Kbd`                                      | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/lazy-image.tsx::LazyImage`                         | same                                                  | Copy complete pinned implementation; keep legacy root export by using the ref component's existing `@chimera/ui/lazy-image` subpath                         |
| `frontend/ui/src/material-shape.tsx::MaterialShape`                 | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/launcher-menu.tsx::LauncherMenu*`                  | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/accordion.tsx::Accordion*`                         | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/chip.tsx::FilterChip`                              | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/drawer.tsx::Drawer*`                               | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/popover.tsx::Popover*`                             | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/snackbar.tsx::Snackbar*`                           | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/shape-toggle.tsx::ShapeToggle`                     | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/expressive-choice.tsx::ExpressiveChoice`           | same                                                  | Copy implementation, add braces to satisfy Chimera Oxlint; no behavior change                                                                               |
| `frontend/ui/src/number-stepper.tsx::NumberStepper`                 | same                                                  | Copy complete pinned implementation                                                                                                                         |
| `frontend/ui/src/index.ts`                                          | same                                                  | Export reference primitives through root wherever there is no legacy name collision; retain legacy switches, lazy image, sparkline and Material You exports |
| `frontend/ui/package.json`                                          | same                                                  | Register actual executable SSR fixture tests via `test:unit`, preserve already-added `typecheck` and old build/exports                                      |
| `frontend/ui/tests/`                                                | tests                                                 | Add `progress-switch.test.ts` and `ref-primitives.test.ts` as registered SSR fixtures; not substitutes for ref browser tests                                |

**Exact mechanical copy mapping:** only `@nyanpasu/utils` → `@chimera/utils` was applied to the 13 missing ref sources. Their original filenames, symbol names, logic and contracts were retained. Prettier normalization follows Chimera style; lint-only braces were added in `expressive-choice.tsx`.

## Remaining differences and compatibility

- Direct imports of `@chimera/ui/linear-progress` keep old `data-slot="linear-progress-bar"` markup; `@chimera/ui/progress` and package root use ref's `data-slot="linear-progress-indicator"` plus indeterminate animations. The old import is a narrow compatibility boundary until affected screens can migrate with screenshot and interaction evidence.
- `@chimera/ui` root preserves the legacy `LazyImage`, `SwitchItem`, and `Sparkline` bindings. Ref's corresponding same-name primitives remain addressable at their individual subpaths. Do not replace root exports blindly.
- The existing `TextMarquee`, `Button`, and style/theme implementations still differ in other slices; copied `NavigationList` was deferred until the `TextMarquee` dependency is inspected.
- Browser-only semantics (accordion exit animation; chip Space/click/disabled behavior; drawer repeated reopen; popover focus/escape; snackbar placement; stepper keyboard and clamping; selection focus restoration) remain unverified. Existing ref browser tests are evidence of the reference behavior only, not proof that this target has run the same tests.
- No new imports in the main/legacy application were added to activate these primitives; the changes prepare the shared UI API without silently replacing presentation.

## Remaining path inventory (not a parity claim)

Of the 59 TypeScript files under `ref/frontend/ui/src`, 50 now have a same-relative-path counterpart. The nine missing modules are:

`date-time-field.tsx`, `fab-menu.tsx`, `hct-color-picker.tsx`, `menu-surface.tsx`, `menubar.tsx`, `navigation-list.tsx`, `select.tsx`, `shared-tooltip.tsx`, `sidebar.tsx`.

The 50 matching paths **are not all logic-aligned**; the existing `animated-tabs`, `dnd-grid`, `dropdown-menu`, `input`, `modal`, `scroll-area`, `slider`, `sparkline`, `text-marquee`, `tooltip` and other modules still require path-by-path analysis.

## Verification and rollback

- `corepack pnpm --filter @chimera/ui test:unit`: 11 passed, 0 failed (SSR fixture only; markup/state contracts, not user interaction).
- `corepack pnpm typecheck`: passed for interface, utils, UI, main UI (including Paraglide compile), and E2E TypeScript. This required a transient ignored `.tmp/bin/pnpm` Corepack shim because the host has no standalone `pnpm` executable; the first aggregate attempt failed on the missing executable, not TypeScript.
- `corepack pnpm --filter @chimera/ui typecheck`: passed.
- `corepack pnpm --filter chimera-ui exec tsc --noEmit -p tsconfig.json`: passed.
- `corepack pnpm lint:frontend-boundaries`: passed, confirming main and legacy UI still use generated shared application IPC boundaries.
- `corepack pnpm exec oxlint` on changed/new primitive source files: passed after the `expressive-choice` lint-only brace correction.
- `corepack pnpm exec prettier --check` on edited source/manifest/test files: passed after formatting.
- `git diff --check`: passed.
- `corepack pnpm --filter chimera-ui build`: passed **after all 13 added primitives**. Vite warned about the native config loader's `__dirname` and large bundle chunks; neither caused a build failure.
- Not run: reference browser interaction specs, actual desktop/legacy window E2E, visual regression, independent review and full frontend test matrix.

Rollback: revert only this slice's `progress.tsx`, `switch.tsx`, `index.ts`, UI manifest test script, 13 new primitive files and two test files, leaving the earlier glass-controls slice and unrelated user modifications intact. Neither the ref baseline nor persisted user data was changed.
