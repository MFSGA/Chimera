# Shared Input and SearchField reference alignment

## Baseline and intent

- Base: `master@94bdd8e4cb5100bbbdbfa6ff6333bf13909bded9`.
- Pinned `ref` gitlink: `518bf49843e1f735380e11da1ebe2a85eb8ce8c6`.
- Scope: `frontend/ui/src/input.tsx`, `search-field.tsx`,
  `glass-surface.tsx`, and dashboard widget-sheet SearchField usage.
- Risk: L1 UI styling/API compatibility; no backend, IPC, user data, agent,
  routes, legacy presentation, or reference submodule changes.

## Reference mapping and decisions

**Input:** `ref/frontend/ui/src/input.tsx::{ Input, NumericInput }` maps to
the same Chimera source path. No production change is needed. After
normalizing imports and formatting, the only behavioral difference is
`useMemo([label, children])` in Chimera versus `useMemo([])` in upstream.
Keep Chimera's reactive dependencies to avoid stale floating-label
detection when a translated label or child changes. This is a documented
compatibility/bug-fix exception, not a reason to regress for cosmetic parity.

**SearchField:** `ref/frontend/ui/src/search-field.tsx` maps to
`frontend/ui/src/search-field.tsx`. Restore the complete reference
`searchFieldVariants` with the original `basic` default and new `glass`
variant. Switch icons to the established `~icons/material-symbols` imports
and map `@nyanpasu/utils` to `@chimera/utils`. The component body matches
upstream after formatting and this package-name mapping.

**Glass base:** `ref/frontend/ui/src/glass-surface.tsx` maps to the same
new path in Chimera. Copy its complete `glassSurfaceVariants` and
`GlassControlGroupContext` definitions with formatting. Other Glass
controls can use this shared foundation in separate migration work.

**Dashboard consumer:**
`frontend/chimera/src/pages/(main)/main/dashboard/_modules/widget-sheet.tsx`
stays unchanged. It uses the original default `basic` style and supplies
translated placeholder and clear-button labels.

## Executed checks and limitations

- Executed **4/4** Node contract tests on the exact SearchField and
  GlassSurface Git blobs to be committed: default style/value forwarding,
  `glass` variant, Escape/clear/focus, and read-only/disabled/ARIA behavior.
- Verified the production `input.tsx` blob separately from GitHub: the
  dynamic-label `useMemo` dependency list is `[label, children]`, unlike
  upstream's empty list. The fifth AST contract test is included for CI, but
  was **not executed locally against the complete Input source** because
  DevSpace was unavailable.
- The Node tests transpile actual TSX and replace React and CVA/Icon modules
  at the boundary. They do **not** prove actual browser focus, translations,
  visual CSS or keyboard/pointer navigation.
- Local AppleDev/DevSpace, full TypeScript typecheck, frozen install,
  repository Prettier, production build and desktop E2E were unavailable.
  The GitHub API commit is unsigned. Keep the PR Draft until validation.
- Required follow-up: `pnpm install --frozen-lockfile`,
  `pnpm --filter @chimera/ui test:unit`, `pnpm typecheck`,
  `pnpm lint:frontend-boundaries`, `pnpm lint:prettier`,
  `pnpm --filter chimera-ui build`, real focus/clear/keyboard browser E2E,
  and a signed local commit with independent review.

## Rollback

Revert this feature commit to restore the previous SearchField with a single
appearance. Input, its consumers, persistence and the legacy UI remain
unchanged; no migration or special data rollback is necessary.
