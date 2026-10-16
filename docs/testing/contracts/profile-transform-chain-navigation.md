# Profile transform-chain editor navigation contract

- Contract ID: `PROFILE-CHAIN-NAVIGATION`.
- Risk: L1 for the navigation action itself. It scrolls the existing main Profile detail page and does not mutate configuration.
- Boundary: Windows desktop UI E2E in the registered `profiles` suite, included by `hermetic`.
- Initial state: The suite creates and activates a test-owned local Profile and opens its detail route at the top of the page. The test requests a 1240×360 window, then reads the actual `window.innerHeight` and editor bounds to confirm the editor center starts outside the effective viewport; the desktop minimum size may clamp the requested height.
- Operation: Click the visible Profile transform-chain action once.
- Expected result: The inline editor's center moves into the visible window. The test restores a 1240×720 window before editing the chain and the `afterEach` hook restores it even if an assertion fails. The test then verifies persisted order and runtime diagnostics in the companion transform rejection contract.
- Independent result source: The browser reads the actual editor card's bounding rectangle and `window.innerHeight` immediately before and after the click. It first proves the editor center starts outside the viewport, then waits for its center to enter the viewport.
- Regression sensitivity: A mismatched DOM selector, missing editor, or click handler that does not scroll fails before the later chain mutations can pass.
- Interaction limits: The action uses a visible WebDriver click. No synthetic DOM event, direct state write, or IPC call substitutes for the click. This case does not assert native pointer input semantics.
- Omitted coverage: Dragging with a physical pointer remains unverified due to the pinned desktop driver's synthetic mouse-event path; keyboard reorder and accessible move buttons are covered separately.
- Cleanup: The existing transform-chain suite restores the prior active Profile and global transform list and removes only its uniquely named fixture Profiles.
- Relevant rules: T01, T02, T03, T04, T05, T07, T10, and T13.
