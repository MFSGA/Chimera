# Reference Snackbar migration

- Base: `master@c4bfe36b9d67237a9ecbc743f48f432d32bf6877`; pinned read-only ref:
  `ref@5ed3c019e13015fc5b06fc8d7069b330607d9036`.
- Source map: `ref/frontend/ui/src/snackbar.tsx` →
  `frontend/ui/src/snackbar.tsx`. Export `SnackbarProvider`,
  `SnackbarProviderProps`, `Snackbar` and `SnackbarProps` from the
  shared root barrel, matching the reference.
- Only brand mapping is `@nyanpasu/utils` → `@chimera/utils`;
  React, Radix Toast, Motion and controllable-state semantics remain unchanged.
  No new dependency or lockfile change.
- This is a foundation-only slice: do not replace
  `frontend/chimera/src/components/base/base-notice.tsx` or existing
  application notification providers yet. Main and legacy UI remain unchanged.
  Existing notices may use different lifetime and queue semantics; consumer
  migration needs separate interaction and accessibility review.
- Regression contracts: `pnpm --filter @chimera/ui test:snackbar`
  transpiles the real TSX source and mocks module boundaries. Tests cover
  provider viewport, default/controlled open, callback, forwarding, motion
  entry and exit. These tests do not establish real browser focus,
  announcement, timer expiry, swipe dismissal or accessibility.
- Acceptance remains blocked on full CI and desktop E2E, real
  accessibility/interaction review and maintainer signature. The API
  commit is unsigned and the PR must remain Draft. Do not infer a test pass
  from adding test code.
- Rollback: revert the isolated Snackbar commit; no persistence,
  backend, IPC, or existing notification consumer was changed.
