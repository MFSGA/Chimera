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

## Reference comparison

`ref/` does not contain the network assistant feature. The applicable shared behavior is the current Chimera `AppContentScrollArea` contract, which uses an always-visible Radix scrollbar with a shadowed thumb.

## Test isolation

The E2E suite uses its isolated runtime configuration and data directories. It does not enable or modify the host system proxy, TUN mode, startup settings, credentials, or real user data.
