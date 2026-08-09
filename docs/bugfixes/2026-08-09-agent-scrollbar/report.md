# Network assistant scrollbar

## Summary

The network assistant page renders content below the desktop viewport, but the shared Radix scrollbar renders an empty scrollbar container without a thumb. The user can see the lower content cut off without a discoverable scrollbar.

## Environment

- Windows desktop Tauri UI
- Viewport: 1240x638
- Locale: `zh-cn`
- Base SHA before reproduction: `85944e22edc0560d68adf3205edb56aa5fb7cebf`

## Scope and severity

- Scope: main UI network assistant page (`/main/assistant`)
- Severity: medium; content remains inaccessible or undiscoverable at smaller window sizes

## Current buggy behavior

1. Open the main UI.
2. Open the network assistant from the top navigation/help menu.
3. Wait for diagnostics content to render.
4. At a 1240x638 window, the page content extends below the viewport.
5. The shared scroll area contains a visible scrollbar element, but its thumb is not mounted, so there is no visible or draggable thumb.

The supplied before screenshot is embedded below.

![Before](/docs/bugfixes/2026-08-09-agent-scrollbar/before.png)

## Expected behavior

The network assistant must use the shared scroll-area behavior: overflowing content remains scrollable, the scrollbar thumb is mounted and visible while idle, and the thumb has the shared shadow treatment.

## Red reproduction

Regression test: `tauri-e2e/specs/agent-scrollbar.e2e.ts`

Command:

```text
pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/agent-scrollbar.e2e.ts
```

The test is intentionally expected to fail before the production fix because the shared scrollbar container has no mounted thumb.

Observed Red result:

```text
hasScrollbar: true
hasThumb: false
scrollbarHtml: <div ... data-slot="scroll-area-scrollbar" data-state="visible" ...></div>
```

The Red evidence was committed as `d46f236`.

## Root cause and fix

`AppContentScrollArea` renders the Radix scrollbar with `type="always"`, but the shared `ScrollAreaThumb` was not force-mounted. Radix therefore left an empty scrollbar container in the DOM while idle, making the page technically scrollable but visually undiscoverable.

The shared thumb now uses `forceMount`, preserving the existing reusable scrollbar behavior and shadow styling for all pages that use `AppContentScrollArea`.

## Green verification

Commands:

```text
pnpm e2e:tauri:build
pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/agent-scrollbar.e2e.ts --logLevel warn
pnpm --filter @chimera/tauri-e2e typecheck
pnpm exec prettier --check frontend/chimera/src/components/ui/scroll-area.tsx tauri-e2e/specs/agent-scrollbar.e2e.ts
git diff --check
```

The focused E2E test passed with the following observed values:

```text
scrollHeight: 1514
clientHeight: 541
scrollTop: 240
scrollbarState: visible
scrollbarOpacity: 1
thumbHeight: 529
```

The final tested code SHA is `340131e`.

The supplied after screenshot is embedded below.

![After](/docs/bugfixes/2026-08-09-agent-scrollbar/after.png)

The before state had an empty scrollbar container and no visible thumb; the after state has a visible thumb on the right edge and remains programmatically and interactively scrollable.

## Reference comparison

`ref/` does not contain the network assistant feature. The applicable shared behavior is the current Chimera `AppContentScrollArea` contract, which uses an always-visible Radix scrollbar with a shadowed thumb.

## Test isolation

The E2E suite uses its isolated runtime configuration and data directories. It does not enable or modify the host system proxy, TUN mode, startup settings, credentials, or real user data.
