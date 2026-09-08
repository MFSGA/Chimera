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

| Command in `@chimera/tauri-e2e`                               | Current selection                                                                  |
| ------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `test`, `test:desktop`, `test:smoke`                          | Smoke scenarios, including legacy proxy localization                               |
| `test:critical`                                               | Smoke + runtime + profiles                                                         |
| `test:runtime`, `test:profiles`, `test:settings`, `test:main` | The named group in `spec-suites.ts`                                                |
| `test:agent`                                                  | Agent UI/orchestration with the default stale-proxy fixture                        |
| `test:network`, `test:lan`                                    | Allow LAN; requires the configured external test client                            |
| `test:hermetic`                                               | Smoke + runtime + profiles + settings + main + agent; excludes network and upgrade |
| `test:all`                                                    | All base groups; includes network prerequisites and the conditional upgrade spec   |
| `test:upgrade:v0.22.3`                                        | Dedicated two-phase upgrade runner; requires its old/new binary setup              |
| `test:unit`                                                   | Only `process-cleanup.test.ts`, `runtime-path.test.ts`, and `spec-suites.test.ts`  |

Inspect [spec-suites.ts](spec-suites.ts) for the authoritative membership and [upgrade-v0223-v0230.ts](upgrade-v0223-v0230.ts) for upgrade prerequisites. The upgrade spec skips when `CHIMERA_E2E_UPGRADE_PHASE` is absent: a normal `test:all` result is not evidence that both upgrade phases ran. The unit command is an explicit list, not discovery of every `.test.ts` file.

## Current CI coverage

[The desktop workflow](../.github/workflows/e2e.yaml) currently runs on Windows: PRs select `critical`, pushes select `smoke`, and scheduled runs select `hermetic`. It also runs the controller-port fallback regression and the registered harness unit tests. Agent/settings/main coverage is not part of the PR `critical` group. Network and the dedicated upgrade runner are not automatically exercised by those selections.

Check the workflow and suite membership when adding tests. Register new desktop specs and provide executable unit-test entries; report which assertions actually ran. Build, typecheck, skipped tests, and unselected suites are not test passes.

## Runtime and isolation

- The E2E build uses `backend/target/e2e` and the Cargo `e2e` feature. The embedded WebDriver is feature-gated; the normal release build should not include that test interface.
- The default binary is `backend/target/e2e/debug/chimera.exe` on Windows, or `chimera` elsewhere. `CHIMERA_E2E_BINARY` overrides it. The default embedded port is `4446`, configurable with `CHIMERA_E2E_WEBDRIVER_PORT`.
- The harness initially connects to the `legacy` window. Main-window specs open and switch to the main window. Both UIs remain supported; shared changes need checks for affected flows in both.
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
