# Embedded WebDriver pointer lifecycle / UI menu regression

## Scope and observed failures (2026-11-09)

- Candidate base: Chimera master@03d6e641, branch codex/scrollarea-wheel-and-tooltip-e2e-20261109.
- Ref source for UI: read-only Nyanpasu ref@911644c8; preserve upstream component semantics.
- This is an **embedded WebDriver test-driver patch**, not a change to physical OS mouse/trackpad handling or Core/Service.
- [PR #636 hermetic run 42661562377](https://github.com/MFSGA/Chimera/actions/runs/42661562377): Tooltip received zero pointermove events after a W3C pointer action (centerHitsTrigger=true); the driver dispatch_pointer_event generated only mousemove on moves and only pointerdown on presses. That cannot exercise React/Radix PointerEvent handlers.
- Settings failures in the same run had a separate root cause: the named slots language-selector-menu and theme-mode-selector-menu were overwritten by the shared MenuSurface's historical main-dropdown-menu-motion-content slot under Radix asChild. The language menu was genuinely open with checkbox items in the failure-time DOM, but its named slot was absent. UI caller slots now take precedence; the default historical locator remains when no named slot is supplied.

## Driver behavior

- On an embedded W3C pointer move, dispatch a JavaScript-created PointerEvent of type pointermove with pointerType=mouse and pointerId=1, as well as the existing compatibility mousemove. When the hit target changes, synthesize matching pointerout/mouseout for the previous connected target and pointerover/mouseover for the new target with relatedTarget.
- On down and up, emit the corresponding pointerdown and pointerup before the compatibility mousedown/mouseup; preserve existing click/contextmenu synthesis after release.
- Track the previous hovered element per WebView JS context; do not send events to a detached element when a route replaces the old target.
- This is **not native input**. Every injected JS event has isTrusted=false; do not assert trusted pointer delivery or browser-native hover physics. The desktop Tooltip regression must assert synthetic pointer events **actually reach the real component**, its portal opens, and pointer exit removes it.
- A real system mouse/trackpad acceptance test remains necessary to make claims about OS input.

## Checks and acceptance gate

- Unit tests execute JavaScript source extracted from the vendored Rust executor, covering initial movement, pointer target transitions, repeated movement, detached route elements, and down/up. The ScrollArea wheel tests separately cover nested scrolling, cancellation, and containment.
- The UI contract test mounts the real DropdownMenu component and verifies both the default historical slot and an explicitly named theme-mode-selector-menu slot.
- Local candidate: full workspace TypeScript, UI component tests (43/43), E2E harness (44/44), Prettier, Rust source rustfmt, vendor plugin cargo check and targeted Clippy pass.
- **Not yet accepted:** new-head CI and Windows WebView2 settings/main suites must be inspected. Historic runs before the pointer and data-slot patches are failure evidence only, not acceptance. On a failing run, inspect diagnostics before changing the test to avoid false green results.
