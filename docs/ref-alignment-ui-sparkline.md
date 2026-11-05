# Ref alignment: shared Sparkline model and animation

## Baseline and scope

- Chimera base: `master@94bdd8e4cb5100bbbdbfa6ff6333bf13909bded9`.
- Fixed reference gitlink: `ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`.
- Ref source: `ref/frontend/ui/src/sparkline.tsx` (blob `4ea239c9178e437cbd8a9a913cabc61bf3bde4f0`).
- Risk level: L2, dashboard visualization behavior, animation and resizing. No persisted state, IPC or backend changes.

## Path / symbol mapping

- `ref/frontend/ui/src/sparkline.tsx::{Sparkline,computeScale,findShift,scrollIn,drawSettled}` -> `frontend/ui/src/sparkline.tsx`. Copy the upstream source and map only product package import `@nyanpasu/utils` to `@chimera/utils`. The component now renders a `div` chart root wrapping SVG, as the reference does.
- `ref/frontend/ui/tests/sparkline.browser.test.tsx` -> `frontend/ui/tests/ref-sparkline-model.test.cjs` for executable Node model contract coverage. The reference browser tests remain the authoritative visual/interaction target; the Node tests use stubbed DOM/Animation.
- Direct dependency `es-toolkit@1.48.1` is added to `frontend/ui/package.json` and to the `frontend/ui` lockfile importer, reusing the version already present in the pinned lock. The upstream `isEqual` and `d3` import symbols are preserved.
- UI consumer `frontend/chimera/src/components/widgets/widget-sparkline.tsx` passes `data` and `className` only and requires no JSX changes. The separate legacy Material You Sparkline remains untouched; the application root export still points to Material You.

## Observable behavior and tests

- Equal-content arrays avoid resetting animation when websocket state creates fresh arrays.
- Append-only windows animate the new samples using native Web Animations with linear X translation and independent easing for Y rescale; subsequent updates continue from the in-flight fractional position without snapping.
- ResizeObserver performs size updates without layout reads during every React render; cancellations remove animations and unmount disconnects observation.
- Empty/one-point series clears paths; first nonzero peaks avoid a zero-height scaling artifact.
- Four Node contract tests exercise the actual `computeScale`, `findShift`, `scrollIn` and `drawSettled` helper logic in a mocked animation environment. These do not prove compositor timing or actual browser SVG layout.

## Acceptance, limitations and rollback

Because local DevSpace was unavailable, the browser tests, strict local typecheck, frozen install, Prettier, build, and Node tests have NOT been executed here. The GitHub API commit is unsigned and the PR must remain Draft. Run `pnpm install --frozen-lockfile`, `pnpm --filter @chimera/ui test:unit`, `pnpm typecheck`, `pnpm lint:frontend-boundaries`, `pnpm exec prettier --check`, `pnpm --filter chimera-ui build`, and actual browser E2E before merge. Review current CI results and re-sign the commit locally when DevSpace returns. No interactive desktop evidence is claimed.

Rollback by reverting the Sparkline source, package metadata, lockfile importer and model tests together. Avoid replacing `frontend/ui/src/materialYou/components/sparkline/`, which is still used by legacy UI.
