# Profile transform-chain editor navigation contract

- Contract ID: `PROFILE-CHAIN-NAVIGATION`.
- Risk: L1 for the navigation action itself. It scrolls the existing main Profile detail page and does not mutate configuration.
- Boundary: Windows desktop UI E2E in the registered `profiles` suite, included by `hermetic`.
- Initial state: The suite creates and activates a test-owned local Profile and opens its detail route. The test requests a 1240×360 window, resets that Profile's main scroll viewport to `scrollTop = 0` and asserts it, then reads the actual nested scroll viewport and editor bounds to confirm the editor center starts below it; the desktop minimum size may clamp the requested height.
- Operation: Click the visible Profile transform-chain action once.
- Expected result: The matching Profile's inline editor scrolls in its nearest scroll-area viewport. At the small requested window, a taller-than-viewport editor aligns its top with the visible interior's top, while an editor that fits moves its center inside the viewport. The test then chooses a window size where the editor fits and has enough scroll range to center it, clicks the action again, and verifies the editor center aligns within 2 CSS pixels of the viewport center. It restores a 1240×720 window before editing the chain; `afterEach` restores the window and resets the Profile detail viewport to `scrollTop = 0`, even if an assertion fails. The test then verifies persisted order and runtime diagnostics in the companion transform rejection contract.
- Independent result source: The browser reads the editor card's and its nearest scroll viewport's bounding rectangles, client dimensions, and scroll range before and after each click. It resets and verifies the main viewport at `scrollTop = 0`, proves the initial editor center starts below the viewport, then checks both fit-aware positioning branches. The action selects the editor and trigger by Profile UID so an exiting route's editor cannot receive the scroll.
- Regression sensitivity: A mismatched DOM selector, missing editor, or click handler that does not scroll fails before the later chain mutations can pass.
- Interaction limits: The action uses a visible WebDriver click. No synthetic DOM event, direct state write, or IPC call substitutes for the click. This case does not assert native pointer input semantics.
- Omitted coverage: Dragging with a physical pointer remains unverified due to the pinned desktop driver's synthetic mouse-event path; keyboard reorder and accessible move buttons are covered separately.
- Cleanup: `afterEach` restores the window size and resets the Profile detail viewport to `scrollTop = 0`; the existing suite restores the prior active Profile and global transform list and removes only its uniquely named fixture Profiles.
- Relevant rules: T01, T02, T03, T04, T05, T07, T10, and T13.

## CI failure follow-up

- PR #484 Windows run `38962020671` on `b66eb2ffc8b08d3c6de8c249ea609fac9e4e0b5d` failed this case at the post-click viewport assertion. The observed editor was 771 CSS pixels tall in a 263-pixel viewport, with its top at 335 and the visible viewport top at 88; after the click, `scrollTop` was only 49 (maximum 820), so the editor remained below view. Three later failures in the same spec followed the first failure.
- The Chimera-only action targets the editor for the clicked Profile UID and its nearest Radix scroll-area viewport. The follow-up changes the bounded scroll operation from smooth animation to an immediate scroll position; the Windows E2E must verify that the new position reaches the editor before this is considered resolved. The failed run does not prove the follow-up works.
