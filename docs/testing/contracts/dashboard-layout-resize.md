# Dashboard responsive layout resize contract

- Contract ID: `dashboard-layout-resize`.
- Boundary: the main Dashboard's real window-size and `ResizeObserver` path.
- Suite: `dashboard-main-layout.e2e.ts`, registered in the `main` suite and included by `hermetic`.
- Setup: keep the Dashboard on its normal wide viewport and snapshot the exact `dashboard-widgets` storage value and current window dimensions.
- Operation: resize the real desktop window to a narrower width, then restore its original dimensions. Wait for the viewport and visible widget set to return to the initial state.
- Assertions: the selected wide layout converges back to the original visible widget IDs and shortcuts, and responsive projections do not overwrite the saved layout data. Always restore the original window dimensions in `finally`.
- Coverage limit: this verifies the settled round-trip through the desktop resize observer. It does not assert every intermediate animation frame or a precise 200 ms timing boundary.
- Evidence: the test proves UI convergence and storage preservation. It does not prove layout behavior for every preset or operating system; the registered Windows hermetic run is required for PR evidence.
