# Ref UI: DnD Grid audit and Sparkline compositor migration

## Baseline and objective

- Chimera base: `master@d438e217880e542bd75d86aa4e31bf79e1e2ca8e`.
- Read-only pinned reference: `ref@5ed3c019e13015fc5b06fc8d7069b330607d9036`.
- This slice aligns the dashboard's shared Sparkline to the pinned reference while retaining the existing DndGrid implementation and Chimera-specific dashboard persistence protections.
- Non-goals: change persisted dashboard layouts, backend/agent/IPC, legacy Material You Sparkline, drag/drop interaction semantics, or submodule gitlinks.
- Risk L2: Sparkline DOM changes from an SVG root to a div containing transformed wrappers, and live animations move from main-thread requestAnimationFrame to Web Animations API.

## Source and API mapping

| Reference | Chimera | Decision |
| --- | --- | --- |
| `frontend/ui/src/sparkline.tsx` | Same relative path | Copy complete pinned logic and markup. Map `@nyanpasu/utils` to `@chimera/utils`. Reuse existing `lodash-es::isEqual` rather than introducing `es-toolkit`; for the numeric-array equality required here, both provide deep content equality. This is an explicit package-dependency compatibility exception |
| `frontend/ui/tests/sparkline.browser.test.tsx` | `frontend/ui/tests/ref-sparkline-contract.test.ts` | Add a narrower SSR component markup contract; upstream browser tests of compositor animations, reactive resizing and streaming samples remain unported and must be run in a real browser |
| `frontend/ui/src/dnd-grid/*` (10 files) | Same relative paths | Audited: all geometry, root contexts, sensors, layout, cross-grid source/drop and resize logic match after import order, formatting and brand mapping. No wholesale rewrite is needed |
| `frontend/ui/src/dnd-grid/dnd-grid.tsx::DndGridProps.onInteractionChange` | Same | Preserve **Chimera-required extension**; `frontend/chimera/src/pages/(main)/main/dashboard/index.tsx` uses it to defer responsive layout selection and persistent KV updates during dragging/resizing. Reference does not supply an equivalent callback |
| `frontend/ui/src/dnd-grid/dnd-grid-item.tsx` | Same | Explicit React PointerEvent annotations are retained for Chimera's Motion typing; handler logic matches ref |
| `frontend/ui/src/dnd-grid/use-grid-layout.ts` | Same | Reference imports `isEqual` from es-toolkit, Chimera uses lodash-es on numeric records; no algorithmic differences found |
| `frontend/ui/tests/dnd-grid.browser.test.tsx` | `frontend/ui/tests/ref-dnd-grid-contract.test.ts` | Add pure overlap/resizing constraint tests; browser dragging, source unmount, overlay and exit animations remain subject to reference browser suite / Tauri E2E |

The main dashboard imports `@chimera/ui/sparkline`, while legacy dashboard imports `Sparkline` from the package root, whose Material You version is intentionally untouched. Existing dashboard widget data and data-slot selectors remain unchanged. The new ref Sparkline avoids unnecessary layout reads on identical series, handles streaming updates mid-animation and adjusts on ResizeObserver notifications.

## Verification limits and rollback

Local DevSpace was unavailable during this iteration. **No local build, typecheck, Prettier, browser E2E or WebDriver results can be claimed.** The Draft PR's remote CI should be checked and any format/type issues fixed before review. The Node unit tests cover mathematical grid invariants and Sparkline SSR markup only, not native dragging, pointer capture, touch input, resizing, keyboard focus or compositor animation.

The pinned reference DndGrid itself registers pointer and touch sensors, not an upstream KeyboardSensor; do not claim keyboard drag parity. Preserve Chimera's existing behavior until accessible keyboard movement has a separately specified implementation and tests.

Rollback: revert the isolated feature branch commit; no data migration, backend config update, production dependency addition or host side effects are involved.
