# Debug Path Utils grid differs from ref

## Summary

The Main window Debug settings page renders the Path Utils action grid with three columns at the desktop reference viewport, while the local `ref/` implementation uses four columns. This changes the card rhythm and makes the Chimera page visibly diverge from the reference layout.

## Environment

- Platform: Windows
- WebView2 / Edge runtime observed by WebdriverIO: 151.0.4129.72
- Viewport: 1240 × 638 (WebView content 1224 × 629)
- Test boundary: WebdriverIO Tauri E2E using the embedded WebDriver provider
- Runtime/config data: isolated under the E2E runtime directory configured by `tauri-e2e/wdio.conf.ts`

## Scope and severity

- Scope: `frontend/chimera/src/pages/(main)/main/settings/debug/_modules/path-utils-card.tsx`
- Severity: low. This is a reference-layout mismatch; functionality of the buttons remains available.

## Current buggy behavior

At the desktop test viewport, the Path Utils grid computes three columns:

```text
gridTemplateColumns: 306.656px 306.672px 306.656px
column count: 3
```

The focused regression assertion fails with:

```text
3 !== 4
```

![Before fix](./before.png)

## Expected correct behavior

The Path Utils grid should follow the local reference implementation:

- two columns below the `md` breakpoint;
- four columns at and above the `md` breakpoint;
- existing 8 px grid gap and button geometry remain unchanged.

At the fixed desktop viewport, `gridTemplateColumns` must contain four columns.

## Automated reproduction

Regression test:

```text
tauri-e2e/specs/debug-main-layout.e2e.ts
```

Exact reproduction command:

```bash
CHIMERA_E2E_EVIDENCE_PATH='../docs/bugfixes/2026-08-08-debug-path-grid/before.png' pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/debug-main-layout.e2e.ts
```

The application and embedded WebDriver session start normally. The test reaches `/main/settings/debug`, enables advanced tools, validates the surrounding reference contract, and fails specifically because the Path Utils grid has three columns instead of four.

## Base SHA

Pre-reproduction base SHA:

```text
a8b779e1dcafe8c17743caa38f5289d53d71d559
```

## Reference implementation and differences

The analogous implementation was inspected at:

```text
ref/frontend/nyanpasu/src/pages/(main)/main/settings/debug/_modules/path-utils-card.tsx
```

Reference grid classes:

```text
grid grid-cols-2 gap-2 md:grid-cols-4
```

Chimera currently uses:

```text
grid grid-cols-2 gap-2 md:grid-cols-3
```

The surrounding Debug settings structure and the other measured layout values already match the intended contract, so the production fix should be limited to the responsive column count.

## Test isolation and limitations

- The E2E harness uses isolated config/data directories and restores captured Windows proxy settings on completion.
- The test does not enable TUN mode, alter the host proxy, restart services, or use real user data.
- WebdriverIO prints a Windows-inapplicable executable-permission diagnostic (`Binary Permissions: 666`); the Windows application still starts and reaches the intended UI assertion, so it is not the failure cause.

## Completion

Red evidence commit: pending.

Root cause, implemented fix, after screenshot, and final verification results will be added in the Green completion update.
