# Chimera Tauri E2E

This package contains the initial WebdriverIO smoke test for the Tauri desktop application.

## What it covers

- Builds the existing React frontend.
- Builds a debug Tauri binary with the Cargo `e2e` feature.
- Registers `tauri-plugin-wdio-webdriver` only in that E2E build.
- Launches the binary through the embedded WebDriver provider.
- Verifies that the desktop document loads and React renders into `#root`.

The richer `@wdio/tauri-plugin` command execution, mocking, and log-forwarding APIs are intentionally not enabled yet. This keeps the bootstrap limited to basic user-visible WebDriver operations.

## Run

From the repository root:

```bash
pnpm install
pnpm e2e:tauri
```

To run only the test against an already-built binary:

```bash
pnpm e2e:tauri:test
```

The E2E build uses the isolated `backend/target/e2e` Cargo target directory, so it does not overwrite a running development binary. The default binary path is `backend/target/e2e/debug/chimera.exe` on Windows and `backend/target/e2e/debug/chimera` elsewhere. Override it with `CHIMERA_E2E_BINARY` when needed.

Each test run also recreates `tests/tauri-e2e/.tmp/runtime` and passes its `config` and `data` subdirectories through E2E-only environment variables. This prevents the test process from opening the developer instance's configuration or `storage.db` while leaving normal builds unchanged.

On Windows, the build enables AWS-LC's prebuilt NASM objects so a separate NASM installation is not required.

The application may require an elevated shell because Chimera development can initialize system-level networking components.

The current service diagnostics may report missing `tauri-driver` or Unix executable permissions even when the embedded provider is active on Windows. Treat the final WebDriver session and spec result as authoritative for this bootstrap.
