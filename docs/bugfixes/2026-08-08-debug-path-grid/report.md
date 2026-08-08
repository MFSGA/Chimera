# Debug Path Utils grid differs from ref

## Summary

The Main window Debug settings page rendered the Path Utils action grid with three columns at the desktop reference viewport, while the local `ref/` implementation uses four columns. This changed the card rhythm and made the Chimera page visibly diverge from the reference layout.

The production fix is intentionally limited to the responsive grid column class.

## Environment

- Platform: Windows
- WebView2 / Edge runtime observed by WebdriverIO: 151.0.4129.72
- Viewport: 1240 × 638 (WebView content 1224 × 629)
- Test boundary: WebdriverIO Tauri E2E using the embedded WebDriver provider
- Runtime/config data: isolated under the E2E runtime directory configured by `tauri-e2e/wdio.conf.ts`

## Scope and severity

- Scope: `frontend/chimera/src/pages/(main)/main/settings/debug/_modules/path-utils-card.tsx`
- Severity: low. This was a reference-layout mismatch; functionality of the buttons remained available.

## Red behavior

At the desktop test viewport, the Path Utils grid computed three columns:

```text
gridTemplateColumns: 306.656px 306.672px 306.656px
column count: 3
```

The focused regression assertion failed with:

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

## Root cause

The local reference uses:

```text
grid grid-cols-2 gap-2 md:grid-cols-4
```

Chimera had drifted to:

```text
grid grid-cols-2 gap-2 md:grid-cols-3
```

No other measured Debug layout contract needed a production change.

## Implemented fix

`frontend/chimera/src/pages/(main)/main/settings/debug/_modules/path-utils-card.tsx` now uses:

```text
grid grid-cols-2 gap-2 md:grid-cols-4
```

![After fix](./after.png)

## Automated reproduction and verification

Regression test:

```text
tauri-e2e/specs/debug-main-layout.e2e.ts
```

Red reproduction command:

```bash
CHIMERA_E2E_EVIDENCE_PATH='../docs/bugfixes/2026-08-08-debug-path-grid/before.png' pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/debug-main-layout.e2e.ts
```

Result before fix: 1 spec failed with `3 !== 4`.

Green verification command:

```bash
CHIMERA_E2E_EVIDENCE_PATH='../docs/bugfixes/2026-08-08-debug-path-grid/after.png' pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --spec ./specs/debug-main-layout.e2e.ts
```

Result after fix: 1 spec passed, 1 test passed.

Broader Main UI regression run after the fix:

```text
15 spec files passed, 0 failed.
```

Additional validation:

```text
pnpm --filter @chimera/tauri-e2e typecheck  -> passed
pnpm e2e:tauri:build                       -> passed
pnpm lint                                  -> passed
```

`pnpm lint` still reports existing Rust compiler/clippy warnings, but exits successfully with no lint failure.

## Evidence commits

Pre-reproduction base SHA:

```text
a8b779e1dcafe8c17743caa38f5289d53d71d559
```

Red evidence commit:

```text
dbf7277d1a334b6bd0c68f580a90d722706c84a3
```

The exact Green commit SHA is recorded in the final checkpoint metadata after the commit is created, because a Git commit cannot embed its own final hash.

## Reference implementation

The analogous implementation was inspected at:

```text
ref/frontend/nyanpasu/src/pages/(main)/main/settings/debug/_modules/path-utils-card.tsx
```

The surrounding Debug settings structure and the other measured layout values already matched the intended contract, so the production change remained scoped to the responsive column count.

## Test isolation and limitations

- The E2E harness uses isolated config/data directories and restores captured Windows proxy settings on completion.
- The test does not enable TUN mode, alter the host proxy, restart services, or use real user data.
- WebdriverIO prints a Windows-inapplicable executable-permission diagnostic (`Binary Permissions: 666`); the Windows application starts successfully and the diagnostic is unrelated to this layout assertion.
