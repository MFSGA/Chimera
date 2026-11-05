# Frontend UI: menu, navigation, selection and color primitives

## Baseline, scope and risk

- Chimera checkout: `master` at `ddad1aac71c36b01aac10f650abbf3cc8749b39e`, with uncommitted UI alignment changes present **before** this slice. None of those earlier changes were overwritten intentionally.
- Local read-only `ref/`: `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` (clean).
- Newer `master` ref gitlink: `5ed3c019e13015fc5b06fc8d7069b330607d9036`. GitHub's directory listing showed identical Git Blob SHAs for all **six migrated source files** and for the existing upstream `button.tsx`, `tooltip.tsx`, and `text-marquee.tsx` dependencies at the two reference revisions. Thus this slice can use the local checkout without changing the submodule pin.
- Objective: add the complete source files and root exports for six upstream UI components currently absent from Chimera.
- Non-goals: replace legacy widgets or theme, adopt these primitives in app pages, change persisted state, backend IPC, agent, runtime, release packaging, or reference submodules.
- Risk: L1 shared rendering and dependency compatibility; no intentional data/host side effects. The source compiles against Chimera's existing React 19, Radix 1.6, and Motion 14 versions even though the reference package has later versions. Browser interaction parity is not yet proven.

## File and symbol decisions

| Reference path                                  | Chimera path       | Symbols and decision                                                                                                                                                                                                                                 |
| ----------------------------------------------- | ------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `frontend/ui/src/menu-surface.tsx`              | same relative path | Copy `MenuSurface` and its motion/Slot composition; shared dependency of Menubar                                                                                                                                                                     |
| `frontend/ui/src/menubar.tsx`                   | same               | Copy `Menubar*` and `menubarTriggerVariants`; explicitly annotate `MenubarMenu`, `MenubarSub`, and `MenubarPortal` with their public Radix component types to avoid TS2883's inference through an internal `Scope` type under Chimera's pinned Radix |
| `frontend/ui/src/navigation-list.tsx`           | same               | Copy navigation list, group, active item, icon/text/trailing slots; use existing `Button`, `MaterialShape`, and `TextMarquee`. Note that Chimera's TextMarquee is not fully aligned                                                                  |
| `frontend/ui/src/select.tsx`                    | same               | Copy `Select` compound control, CVA variant exports and controlled state; uses previously aligned `launcher-menu.tsx`                                                                                                                                |
| `frontend/ui/src/fab-menu.tsx`                  | same               | Copy FAB dropdown menu, trigger, content and item; uses existing ref-aligned `Button`                                                                                                                                                                |
| `frontend/ui/src/hct-color-picker.tsx`          | same               | Copy HCT color picker and `parseHexColor` without introducing a second color model                                                                                                                                                                   |
| `frontend/ui/src/index.ts`                      | same               | Add the six root exports; retain Chimera-only Material You, legacy UI and agent-compatible entries                                                                                                                                                   |
| `frontend/ui/tests/ref-menu-primitives.test.ts` | Chimera test-only  | Vite SSR fixture loads the actual source with virtual icons replaced at the unused presentation boundary; assert Radix roles/slots and hex normalization. No real browser events                                                                     |
| `frontend/ui/package.json`                      | same               | Register the test in the existing `test:unit` command, no package update                                                                                                                                                                             |

Copy rules: only the exact token `@nyanpasu/utils` was mapped to `@chimera/utils`; imported third-party module names, component behavior, class strings, and data slots were not renamed. Formatting uses Chimera Prettier. Explicit `typeof MenubarPrimitive.*` declarations are a documented compatibility difference.

## Remaining three missing reference UI paths

Across `frontend/ui/src` and its subdirectories, **49 of 52** upstream TSX files now have same-relative-path counterparts in Chimera (including **45 of 48** top-level TSX files). The remaining three were inspected but intentionally not copied yet:

1. `date-time-field.tsx` depends on `react-aria-components` and `@internationalized/date`, absent from Chimera's current direct UI dependencies and lockfile. Migrate as an explicit dependency/lockfile change with browser locale, parsing, time-zone and keyboard coverage.
2. `shared-tooltip.tsx` imports `radix-ui/internal::{DismissableLayer,Popper}`. Chimera pins `radix-ui@1.6.7`, which does not export `./internal`. Verify a supported upgrade or an alternative shared-tooltip implementation **without silently copying a module that will not compile**.
3. `sidebar.tsx` imports `useIsMobile` from `@nyanpasu/hooks`, a package without a matching Chimera shared-package boundary. Chimera has an application-only hook; importing that into `@chimera/ui` would reverse the correct dependency direction. Align hooks/package ownership before porting.

**Path coverage is not functional parity.** Existing paths still differ in theme, motion, exported semantics and legacy compatibility. In particular, `TextMarquee`, `Tooltip`, `dnd-grid`, `dropdown-menu`, `slider`, `sparkline`, and `scroll-area` warrant separate semantic review.

## Verification and follow-up

- `corepack pnpm --filter @chimera/ui typecheck`: passed after a minimal Menubar type annotation.
- `corepack pnpm --filter chimera-ui exec tsc --noEmit -p tsconfig.json`: passed.
- `corepack pnpm exec oxlint` on all six added files and root index: passed.
- `corepack pnpm exec tsx --test tests/ref-menu-primitives.test.ts`: passed; one test checks markup and seven HCT inputs. Its icon stub is fixture-only, not used by production.
- Full `corepack pnpm typecheck` passed for interface, utils, UI, main UI/Paraglide and Tauri E2E types. The host has no global `pnpm`; it was invoked through an ignored `.tmp/bin/pnpm` Corepack wrapper without changing system configuration.
- `corepack pnpm --filter @chimera/ui test:unit`: **12 passed, 0 failed** (11 pre-existing SSR fixture tests plus the new menu/navigation/color fixture).
- `corepack pnpm lint:frontend-boundaries`: passed; generated shared main/legacy IPC boundary remained intact.
- `corepack pnpm exec oxlint frontend/ui/src/*.tsx frontend/ui/tests/*.ts`: 0 warnings and 0 errors over 49 source/test files.
- `corepack pnpm --filter chimera-ui build`: passed, with Vite's existing large-chunk warning.
- Prettier check for the touched UI files, tests and three alignment reports: passed; `git diff --check` passed.
- Not run: ref Vitest browser tests (Menubar arrow/Escape focus, navigation refs/layout, Select popover interactions, FAB dismissal, HCT slider updates); actual main/legacy desktop UI E2E and screenshots. Do not claim cross-platform or visual parity.
- Rollback only this slice: remove the six new sources and one test, remove their six root exports and `test:unit` test file entry. Preserve all earlier glass/progress/legacy changes and uncommitted files. No commit or push was performed.
