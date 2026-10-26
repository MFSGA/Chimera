# Dashboard responsive layout resize contract

- Contract ID: `dashboard-layout-resize`.
- Boundary: the main Dashboard's real window-size and `ResizeObserver` path.
- Suite: `dashboard-main-layout.e2e.ts`, registered in the `main` suite and included by `hermetic`.
- Setup: snapshot the exact `dashboard-widgets` storage value and current window dimensions; write a deterministic fixture with one item in `4x5` and six widgets in `8x6`, then refresh the Dashboard so the fixture is loaded by the real storage hook.
- Operation: resize the real desktop window to a narrower width and wait until the single-widget `4x5` layout is visible; restore the original wide dimensions and wait for all six `8x6` widgets and both shortcut cards.
- Assertions: the viewport selects the expected layout at each settled size, and responsive projections do not overwrite the fixture. In `finally`, restore the original KV value and window dimensions, refresh the route and verify the exact stored value.
- Interaction guard: `DndGrid.onInteractionChange` reports grid drag/resize activity. A resize-selection timer that expires during an interaction remains pending and is applied after the interaction ends. The desktop spec does not synthesize a simultaneous drag-and-window-resize sequence.
- Coverage limit: this verifies settled selection through the desktop resize observer. It does not assert every intermediate animation frame or a precise 200 ms timing boundary.
- Evidence: the test proves UI convergence and storage preservation. It does not prove layout behavior for every preset or operating system; the registered Windows hermetic run is required for PR evidence.
