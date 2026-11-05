# Ref UI: SegmentedButton Glass and reduced-motion alignment

## Fixed scope and dependency

- Stacked parent: `codex/ref-ui-glass-control-group-20261105@dee33c43fc8f1a5389a06bdbbff70010b2a90b50`, Draft PR #598. This is a continuation of #596 and its `glass-surface.tsx` foundation. Do not merge this branch before those dependencies.
- Repository base before the stack: `master@94bdd8e4cb5100bbbdbfa6ff6333bf13909bded9`.
- Pinned reference gitlink: `ref@518bf49843e1f735380e11da1ebe2a85eb8ce8c6`. No submodule updates.
- Risk L2: `SegmentedButton` is shared across main and legacy UI; pointer/keyboard selection, focus, animations and layout measurements require browser E2E.
- Non-goals: backend, IPC, persisted UI preferences, route structure, agent, or legacy Material You replacements.

## Reference path and symbol mapping

Reference `ref/frontend/ui/src/segmented-button.tsx` maps to the existing Chimera `frontend/ui/src/segmented-button.tsx`.

- `segmentedButtonVariants`, `segmentedButtonItemVariants`: add reference `glass` variants. Keep existing `filled` and `tabs` unchanged.
- `SegmentedButton`: use `useReducedMotion()` to disable pill animation while respecting OS motion preferences. Allow the selection pill, `useLayoutEffect` measurement and `ResizeObserver` to run for both `tabs` and `glass`. Deduplicate identical rectangles in `setPillRect` to avoid a re-render loop, and memoize provider context so remeasurement alone does not re-render children.
- `SegmentedButtonItem`: render the translucent state-layer for both `tabs` and `glass`, but leave the automatic check indicator specific to `filled`. Preserve Chimera's `hideIndicator` and existing click/Ripple behavior rather than blindly overwriting local APIs.
- Add `frontend/ui/tests/ref-segmented-glass.test.cjs` to the existing `test:unit` command, keeping both earlier SearchField and GlassControlGroup test suites.

## Tests, limitations and acceptance

Node tests are supplied for variant styling, selected pill and controlled value callbacks, reduced-motion transitions, measurement deduplication/observer cleanup, and item click/disabled/state-layer behavior. Their mocked React hooks, Radix components, CVA classes and Motion boundaries do not prove real browser layout, drag/keyboard input, accessibility focus or Windows Tauri E2E.

The AppleDev/DevSpace workspace was unavailable during this iteration. The source was compared against the pinned ref, and the new test file passed a JavaScript syntax check, but **complete UI unit tests, pnpm typecheck, Prettier, frozen install, Vite build and browser/Tauri E2E were not run**. Remote CI on these stacked branches does not trigger automatically because `ci.yaml` watches PRs targeting master/main/dev/release branches only. Before merge, validate locally, fix all parent Prettier/CI failures, and replace unsigned GitHub API commits with locally verified signed commits.

## Rollback

Revert this one commit to restore the previous two-variant SegmentedButton. Group/Glass/Current Input base components remain owned by prerequisite PRs. No backend or persisted user data rollback is necessary.
