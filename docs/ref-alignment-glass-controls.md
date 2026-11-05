# Frontend utils + UI glass-controls alignment slice

- Ref commit: `518bf49843e1f735380e11da1ebe2a85eb8ce8c6` (local `ref/` clean).
- Chimera branch: `master`; initial working tree clean.
- Objective: assess concrete files against pinned ref, align the complete glass-control primitive dependency chain without removing supported legacy UI.
- Non-goals: rewrite `frontend/ui` wholesale, migrate the reference theme package, change runtime or persisted data, change the default look of existing controls, modify ref, or introduce new backend APIs.
- Risk: L1 shared UI rendering and styling. No persistence, IPC, host-network, authorization, or privilege changes.

## Path and symbol decisions

| Ref path                                  | Chimera path                                          | Decision                                                                                        |
| ----------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `frontend/utils/src/chains.ts::chains`    | same relative path                                    | Behavior identical; semicolons/formatting only, keep                                            |
| `frontend/utils/src/classnames.ts::cn`    | same relative path                                    | Behavior identical; brand import mapping only, keep                                             |
| `frontend/utils/src/index.ts`             | same relative path                                    | Retain extra color exports used by the Chimera theme                                            |
| no corresponding file                     | `frontend/utils/src/color.ts::{alpha,lighten,darken}` | Chimera theme compatibility; retain, assess location when theme is aligned                      |
| `frontend/utils/package.json`             | same relative path                                    | Add `private` and `typecheck`; keep Chimera package identity, version, and existing build entry |
| `frontend/utils/tsconfig.json`            | same relative path                                    | Retain self-contained config: Chimera has no root `tsconfig.base.json`; ref extends one         |
| `frontend/ui/src/animated-item.tsx`       | same relative path                                    | Logic already aligned; no source change                                                         |
| `frontend/ui/src/empty-state.tsx`         | same relative path                                    | Logic already aligned; no source change                                                         |
| `frontend/ui/src/glass-surface.tsx`       | same relative path                                    | Copy pinned ref behavior and symbols                                                            |
| `frontend/ui/src/glass-control-group.tsx` | same relative path                                    | Copy pinned ref behavior and symbols, use `@chimera/utils` brand mapping                        |
| `frontend/ui/src/button.tsx`              | same relative path                                    | Align Glass variant, group context and button loading content semantics                         |
| `frontend/ui/src/search-field.tsx`        | same relative path                                    | Align `searchFieldVariants` and Glass variant; keep currently installed MUI icons               |
| `frontend/ui/src/segmented-button.tsx`    | same relative path                                    | Align Glass variant, pill measurement, reduced motion, stable context and `chains` handler      |
| `frontend/ui/src/index.ts`                | same relative path                                    | Export `GlassControlGroup` without removing legacy Material You, Switch or Sparkline exports    |
| `frontend/ui/package.json`                | same relative path                                    | Add `private` and `typecheck`; retain legacy dependency set, export paths and build entry       |

## Compatibility boundaries and residual differences

- `frontend/ui/src/index.ts` still exposes legacy-only Material You hooks/components and preserves compatibility for root-level `SwitchItem` and `Sparkline`; do not overwrite it with ref's index file.
- The reference version's SearchField imports `~icons/material-symbols` while Chimera's current component uses MUI icon components. This was not changed in this slice; visual parity remains to be verified when the UI/theme dependency graph converges.
- The reference package has more UI primitives and browser test infrastructure than Chimera's current package. This slice is **partial directory alignment**, not proof of `frontend/ui` parity.
- Existing default variants and current caller contracts remain valid. The newly supported Glass variants are available to callers but UI adoption is a separate feature-scope decision.
- Existing main/legacy UI and agent integrations were not deleted or reimplemented. This is a shared rendering-layer change, not an IPC/business API mutation.
- Preserve the current brand-package versions and standalone TypeScript configurations until a cohesive package/tooling migration is verified.

## Acceptance and evidence

- [x] Ref SHA and clean worktree verified before change.
- [x] Every touched file mapped to reference source and required Chimera-specific boundaries reviewed.
- [x] `corepack pnpm --filter @chimera/utils typecheck` — passed.
- [x] `corepack pnpm --filter @chimera/ui typecheck` — passed.
- [x] `corepack pnpm --filter chimera-ui exec tsc --noEmit -p tsconfig.json` — passed.
- [x] `corepack pnpm lint:frontend-boundaries` — passed; reported shared main/legacy generated IPC boundary verified.
- [x] `corepack pnpm exec oxlint` on six changed UI source files — passed.
- [x] `corepack pnpm exec prettier --check` on changed packages/sources — passed.
- [x] `git diff --check` — passed.
- [x] One-off React SSR smoke with `tsx` and its classic JSX global-React test adapter: standalone Glass button and grouped Glass button emitted expected classes/data slots. This is a fixture-level smoke only.
- [ ] Reference `frontend/ui/tests/button.browser.test.tsx` and `glass-controls.browser.test.tsx` not executed; Chimera currently has no corresponding registered browser-test entry for these UI primitives.
- [ ] Visual/interaction regression for loading/asChild button, hover/focus, dark theme and glass-group geometry, plus actual main/legacy desktop E2E, not executed. Do not claim full UI parity.

## Rollback and follow-up

Rollback scope is the two manifest edits, four touched pre-existing UI source files, and two new glass source files; preserve unrelated working-tree work. `ref/` itself remains unchanged. When proceeding to the next `frontend/ui` slice, examine missing reference primitives and their source + imports + browser tests together rather than copying isolated exports. Run targeted interactive browser tests before promoting this from partial to verified UI alignment. Independent review and desktop/visual coverage remain pending.
