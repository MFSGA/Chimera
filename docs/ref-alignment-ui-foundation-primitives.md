# Ref alignment: foundation primitives

## Baseline and objective

- Repository base: `master@7baaf1cdec5db7bed53e5ab010c5274b33392c23`.
- Fixed read-only reference: `ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`.
- Objective: add the pinned reference's small independent primitives without
  changing Chimera's supported legacy UI or service/runtime contracts.
- Risk L1–L2: presentation-level primitives only; `LazyImage` has a legacy
  root export that is deliberately preserved.
- Non-goals: backend, IPC, persistent settings, agents, window routing, and the
  reference submodule gitlink.

## Reference mapping and compatibility

| Pinned reference | Chimera target | Outcome |
| --- | --- | --- |
| `frontend/ui/src/kbd.tsx::Kbd` | Same relative source path | Exact functional source after brand import mapping and formatting |
| `frontend/ui/src/action-swap-text.tsx::{ActionSwap,ActionSwapText}` | Same relative source path | Exact functional source; respects reduced-motion setting |
| `frontend/ui/src/material-shape.tsx::MaterialShape` | Same relative source path | Exact functional source; preserves 96-vertex polygon variants and reduced motion |
| `frontend/ui/src/lazy-image.tsx::{LazyImage,LazyImageProps}` | Same relative source path | Port ref source. Expose via `@chimera/ui/lazy-image` deep import only, since `@chimera/ui` already exports the supported legacy Material You LazyImage used by CacheImage and TrayImage |

The existing `frontend/ui/src/index.ts` gets exports for ActionSwap,
ActionSwapText, Kbd and MaterialShape. No new root export for LazyImage is
introduced: two same-name root exports would be ambiguous, and the
existing application relies on the legacy root. This is a narrow
Chimera compatibility boundary.

No third-party packages or lockfile modifications are necessary.

## Actually executed tests

The six Node contracts in
`frontend/ui/tests/ref-foundation-primitives.test.cjs` were run against
the exact TypeScript/TSX source blobs in an isolated Node 22 environment,
with TypeScript transpilation and mocked React/Motion/JSX boundaries:

1. Kbd DOM properties and className composition.
2. ActionSwap keyed presence, enter/exit props, and initial animation behavior.
3. ActionSwapText value mapping and OS reduced-motion suppression.
4. MaterialShape polygon variants with 96 vertices and depth override.
5. MaterialShape zero-duration transition under reduced motion.
6. LazyImage placeholder visibility, forwarded attributes, state transition
   after onLoad, and user event callback.

They passed **6/6** in the isolated harness, and their Git Blob hashes were
checked against the sources committed with this branch. The six tests are
registered under `pnpm --filter @chimera/ui test:unit` for CI.

**Limitations:** The isolated harness has no real React rendering, browser
geometry, styles, image load lifecycle, keyboard input, accessibility tree,
or desktop WebDriver. AppleDev workspace connectivity was unavailable, so
full pnpm install, typecheck, Prettier, production build and desktop E2E
have **not** been run. Keep the PR Draft until these checks, independent
review and a maintainer-verified signed commit can be obtained.

## Inventory and remaining migration

At this branch's baseline, the pinned shared UI package had 52 TSX paths,
24 absent from Chimera `master`. This slice adds four relative paths,
leaving 20 missing relative paths before accounting for unmerged Draft PRs.
File presence alone is not behavioral or application-level alignment.
Additional migration is required for the missing complex components and
for existing sources that differ from ref. Audit main and legacy page callers
and full window-level E2E separately; do not mark the overall frontend as done.

## Rollback

Revert this one independent feature commit, including its test script and
new source files. The legacy LazyImage remains unchanged and no persisted
data or user flow is migrated.
