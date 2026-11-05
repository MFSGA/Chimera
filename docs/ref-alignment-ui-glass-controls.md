# Ref alignment: GlassControlGroup and Button

## Provenance and scope

- Stacked parent: `codex/ref-ui-input-search-20261105@ddcc69c88f7646fa4c4d589154de7b801f63edf7`, Draft PR #596. This parent introduces the shared `glass-surface.tsx` foundation used here; **this branch must not merge before its parent**.
- Repository master at branch creation: `94bdd8e4cb5100bbbdbfa6ff6333bf13909bded9`.
- Pinned reference: `ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`, including `frontend/ui/src/glass-control-group.tsx` and `button.tsx`.
- Risk: L2 because `Button` is broadly used across main, legacy and agent UI, including its `asChild`, ripple, disabled and loading contracts. No backend, IPC, settings or submodule changes.

## Ref path and symbol mapping

- `ref/frontend/ui/src/glass-control-group.tsx::GlassControlGroup,glassControlGroupVariants` → `frontend/ui/src/glass-control-group.tsx`: use the exact reference JSX/variants with product import mapped to `@chimera/utils`; provider sets the existing `GlassControlGroupContext` value to true.
- `ref/frontend/ui/src/button.tsx::buttonVariants,Button` → `frontend/ui/src/button.tsx`: port upstream `glass` variant for standalone buttons, its width and icon sizing, `glassGroupButtonVariants`, and group context handling. Grouped glass buttons use the shared group surface rather than painting individual shadows. Preserve existing basic/raised/stroked/flat/fab states, ripple/disabled/Click API, and `asChild` child attributes; upstream's content wrapper keeps loading text invisible while progress stays visible.
- `ref/frontend/ui/src/index.ts` → `frontend/ui/src/index.ts`: add the named group to the barrel export; leave legacy Material You exports and all other API paths unchanged.
- Test file `frontend/ui/tests/ref-glass-control-group.test.cjs` is added to the existing UI `test:unit` script **alongside**, not instead of, the four previous Input/SearchField checks and the Input AST test from PR #596.

## Tests and verification boundaries

Four Node source contracts were **executed and passed** in an isolated Linux Node + TypeScript transpilation environment. Their source files had the same Git Blob SHA as the proposed `glass-surface`, `glass-control-group` and `button` changes; the test itself also matches the locally exercised bytes. Contracts: container provider/CVA styling and props; standalone vs grouped glass buttons, including icon sizing; click/ripple/disabled logic; and `asChild` plus loading content. This is not real browser rendering, Radix Slot composition, actual CSS layout, focus navigation, or Tauri E2E.

The macOS DevSpace connection was unavailable, so complete pnpm typechecks, frozen install, repository Prettier, web build and desktop E2E remain required. The GitHub API commit is unsigned. Keep a Draft PR until CI and independent review, then replace with a signed, locally verified commit. In particular, run the pinned ref's `frontend/ui/tests/button.browser.test.tsx` scenarios in a browser to verify loading does not hide the progress indicator, remove content, resize the button or break `asChild`.

## Rollback

Revert this one stacked feature commit to restore the old Button and remove the new group. The foundational `glass-surface` remains owned by parent PR #596. No storage or backend migration is necessary.
