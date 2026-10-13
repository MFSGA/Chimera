# Agent privacy-safe history contract

- **Contract ID:** `agent-history-ui`
- **Behavior:** In the isolated stale-proxy Agent fixture, refreshing the action history displays the successful proxy repair as a verified action. Saving a diagnosis from the visible control stores one fresh, privacy-safe snapshot and displays its summary.
- **Regression:** If the audit view omits verified actions or exposes internal identifiers, if diagnosis capture happens without an explicit click, or if its persisted result is not returned to the UI, the assertions fail.
- **Boundary:** Desktop UI E2E through the history controls and Tauri commands. Read-only IPC observations verify the persisted projection after the visible action.
- **Suite:** `agent`, included in `hermetic`; registered in `tauri-e2e/spec-suites.ts` and run by the Windows PR workflow.
- **Environment:** E2E-feature Tauri binary, isolated runtime directory, `stale-proxy` Agent fixture, `zh-cn` language, Agent route `/main/assistant`.
- **Real dependencies:** Agent history cards, refresh/capture controls, audit and diagnostic history commands, and the bounded local history file.
- **Fixture boundary:** The fixture simulates proxy state and uses SHA-256 revisions matching the production snapshot contract; it makes no host-network changes. The proxy repair outcome is fixture-backed, so this does not prove native authorization or real system proxy mutation.
- **Initial state:** Earlier cases in this spec create and verify one stale-proxy repair. The isolated runtime starts with no diagnostic history; the test reads its count before capturing.
- **Operation:** Click the audit refresh control once, then click the diagnostic Save control once. Polling only reads history through IPC and visible text.
- **Expected UI:** The audit card shows a verified `disable stale system proxy` event. The diagnostic card shows one additional entry with the fixture's healthy/stopped summary.
- **Authoritative result:** `agent_get_audit_history` reports a verified proxy action, and `agent_get_diagnostic_history` reports exactly one more persisted diagnostic entry after the click. Rust history tests cover bounded projections, redaction, and corrupt-file preservation.
- **Persistence and restart:** The explicit capture is written to the isolated app-data history document and read back during the same app process. App restart persistence is not covered.
- **Failure and cleanup:** The harness owns the unique runtime directory and removes it after the run. The fixture only changes in-process state; no system proxy, Service, or TUN settings are touched.
- **Waits and diagnostics:** Each visible-state/history wait has a 15-second bound. Failures retain WDIO output and harness artifacts under the run's isolated `.tmp` directory.
- **Rules and limits:** Covers `T01` through `T05`, `T07`, and `T10`. The embedded driver click path is synthetic and does not establish native pointer input.

## Local execution record

- `cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::history::tests`: 9 passed.
- The first desktop run exposed a fixture contract mismatch: fixture revisions were descriptive strings, so audit recording rejected them; that run also timed out waiting for the final proxy proposal. The fixture now uses distinct SHA-256 revisions for each simulated state, and the full suite passed on the next run.
- `pnpm --filter @chimera/tauri-e2e exec wdio run ./wdio.conf.ts --suite agent --logLevel error`: 8 passed on macOS WebKit 605.1.15. E2E-feature binary: `/Volumes/ChimeraBuild/chimera-pr472-agent-history-e2e/debug/chimera`, SHA-256 `f8a6f4c20387cce0d7c929341c4a7fb6ddf7a4c4d1cf1c10d00cda8c16b58a00`.
- `pnpm typecheck`, the E2E harness unit suite (19 passed), `pnpm test:agent:intent` (5 passed), frontend boundary lint, changed-file Oxlint, Prettier, and `pnpm --filter chimera-ui build` passed.
- The test proves visible controls, IPC projection, and same-process disk persistence only; cold-start persistence and real host networking remain unverified.
