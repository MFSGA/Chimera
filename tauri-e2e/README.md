# Chimera Tauri E2E

E2E testing is Chimera's main extension to the reference implementation. The project preserves both the main UI and the legacy UI, and adds agent capability on the shared business layer. Follow [AGENTS.md](../AGENTS.md) and the [alignment guide](../docs/ref-alignment-guide.md) when changing that layer.

This package uses WebdriverIO with an embedded WebDriver in the real Tauri application. It contains desktop, runtime, profile, settings, agent, network, and upgrade scenarios, plus supporting unit tests. Test suites prove the behavior they assert; they do not by themselves prove source or directory alignment with ref.

## Commands

Run from the repository root after installing dependencies and preparing required sidecars/resources:

```powershell
pnpm --filter @chimera/tauri-e2e test:unit
pnpm --filter @chimera/tauri-e2e typecheck
pnpm e2e:tauri:build
pnpm --filter @chimera/tauri-e2e test:smoke
```

`pnpm e2e:tauri` builds the application and runs the default smoke suite. `pnpm e2e:tauri:test` runs smoke against an existing binary. Neither command automatically runs unit tests or all desktop suites. Rebuild after relevant source changes.

| Command in `@chimera/tauri-e2e`                               | Current selection                                                                                                                                                     |
| ------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `test`, `test:desktop`, `test:smoke`                          | Smoke scenarios, including legacy proxy localization                                                                                                                  |
| `test:critical`                                               | Smoke + runtime + profiles                                                                                                                                            |
| `test:runtime`, `test:profiles`, `test:settings`, `test:main` | The named group in `spec-suites.ts`                                                                                                                                   |
| `test:agent`                                                  | Agent UI/orchestration with the default stale-proxy fixture                                                                                                           |
| `test:network`, `test:lan`                                    | Allow LAN; requires the configured external test client                                                                                                               |
| `test:hermetic`                                               | Smoke + runtime + profiles + settings + main + agent; excludes network, upgrade, and system                                                                           |
| `test:all`                                                    | All non-system base groups; includes network prerequisites and the conditional upgrade spec                                                                           |
| `test:system`                                                 | **Destructive Windows Service/TUN lifecycle**; dedicated disposable VM, elevated shell, clean service ownership and explicit opt-in required. Never automatic in CI   |
| `test:upgrade:v0.22.3`                                        | Dedicated two-phase upgrade runner; requires its old/new binary setup                                                                                                 |
| `test:unit`                                                   | Explicit list: `agent-issue-guidance`, `clash-runtime`, `profile-definition`, `process-cleanup`, `privacy-safe-context`, `runtime-path`, and `spec-suites` unit tests |

Inspect [spec-suites.ts](spec-suites.ts) for the authoritative membership and [upgrade-v0223-v0230.ts](upgrade-v0223-v0230.ts) for upgrade prerequisites. The upgrade spec skips when `CHIMERA_E2E_UPGRADE_PHASE` is absent: a normal `test:all` result is not evidence that both upgrade phases ran. The unit command is an explicit list, not discovery of every `.test.ts` file.

## Current CI coverage

[The desktop workflow](../.github/workflows/e2e.yaml) currently runs on Windows: PRs select `critical`, pushes select `smoke`, and scheduled runs select `hermetic`. It also runs the controller-port fallback regression and the registered harness unit tests. Agent/settings/main coverage is not part of the PR `critical` group. Network, the dedicated upgrade runner, and the destructive Windows `system` suite are not automatically exercised by those selections.

Check the workflow and suite membership when adding tests. Register new desktop specs and provide executable unit-test entries; report which assertions actually ran. Build, typecheck, skipped tests, and unselected suites are not test passes.

## Windows Service/TUN system lifecycle (manual only)

This is an **optional host-mutating integration test**, not a desktop UI interaction test and **not** a default CI or `test:all` target. It installs and restarts a system Service, changes the test app's TUN/Service settings, and checks Windows routes. Use only an isolated, disposable Windows VM you control, with no existing Chimera Service or TUN usage. Elevated permissions and the explicit opt-ins below are necessary but **do not prove VM isolation**; the operator is responsible for that prerequisite. Never run it on a daily-use host or shared hosted CI runner.

Build the matching E2E app and matching `chimera-service.exe` for the VM. Set `CHIMERA_E2E_SERVICE_BINARY` to the Service executable if it is not alongside the E2E app, and ensure the system-test runtime/config is isolated. In an elevated PowerShell session on that VM:

```powershell
$env:CHIMERA_E2E_SYSTEM_LIFECYCLE = '1'
$env:CHIMERA_E2E_DEDICATED_VM = '1'
pnpm --filter @chimera/tauri-e2e test:system
```

The WDIO configuration rejects the suite _before launching the test application_ unless opt-in, dedicated-VM acknowledgment, Windows elevation, a readable current Service executable and a not-installed Service are all confirmed. The spec also refuses to overwrite pre-enabled TUN/Service settings. Any failed or interrupted host-level cleanup must be resolved in that VM before reusing it. The ordinary `test:unit` command tests the fail-closed preflight logic without installing Service or changing network settings. Real Service/TUN behavior remains **unverified** until this dedicated VM suite is actually executed and its result recorded.

## Runtime and isolation

- The E2E build uses `backend/target/e2e` and the Cargo `e2e` feature. The embedded WebDriver is feature-gated; the normal release build should not include that test interface.
- The default binary is `backend/target/e2e/debug/chimera.exe` on Windows, or `chimera` elsewhere. `CHIMERA_E2E_BINARY` overrides it. The default embedded port is `4446`, configurable with `CHIMERA_E2E_WEBDRIVER_PORT`.
- The harness initially connects to the `legacy` window. Main-window specs open and switch to the main window. Both UIs remain supported; shared changes need checks for affected flows in both.
- Each top-level spec starts and ends with the harness closing temporary non-app windows, focusing `legacy`, and restoring the legacy entry URL. Main specs use the shared main-window helper to reuse a rendered singleton, recover a stale blank main window when necessary, and enter their target route through SPA navigation. Persistent application/config state must still be restored explicitly by the owning test.
- Each normal run creates a unique `tauri-e2e/.tmp/runtime` directory and passes isolated config/data paths to the application. An explicit `CHIMERA_E2E_RUNTIME_DIR` is retained by the harness for callers such as the upgrade runner.
- The application still runs production service/core/system-proxy initialization paths. Isolated files do not isolate host networking. Use a dedicated runner or VM for host-affecting tests and remain within the authorized scope.
- On Windows, the harness captures host proxy settings and attempts to restore them in `onComplete`, unless explicitly disabled. A forced termination can prevent cleanup. Process cleanup currently matches the E2E binary directory, not a per-run PID set; do not run independent sessions sharing that directory concurrently.
- Non-Windows host proxy restoration and process cleanup are not implemented by `process-cleanup.ts`. Do not infer cross-platform isolation or verification from the available binary-path branches.

## Agent evidence boundary

Selecting `agent`, `hermetic`, or `all` defaults to the `stale-proxy` agent fixture. In that fixture, the backend returns deterministic snapshots, bypasses the native confirmation dialog, and marks the simulated proxy as repaired rather than changing the real proxy.

These tests exercise the real desktop UI and parts of proposal orchestration. They do not prove native authorization, real host repair, rollback, or recovery. Add and report real-state tests separately when changing those behaviors; keep deterministic UI coverage.

## Artifacts and further details

`CHIMERA_E2E_ARTIFACT_DIR` enables harness failure screenshots, page source, and error files. Some specs also write local evidence under `.tmp`. Artifacts can contain application content and are not automatically redacted; use synthetic data and review evidence before sharing. Keep private configurations, credentials, subscription URLs, and raw logs out of commits.

See the [E2E architecture notes](../docs/tauri-e2e-minimal-architecture.md) for the implementation map and remaining coverage boundaries.
