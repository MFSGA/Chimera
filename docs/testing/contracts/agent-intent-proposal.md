# Agent intent proposal contract

- **Contract ID:** `agent-intent-proposal`
- **Behavior:** In the isolated stale-proxy Agent fixture, entering `关闭残留代理` and choosing Continue opens the proposal dialog. The simulated system proxy remains enabled until the user confirms; this test cancels the proposal.
- **Regression:** If the UI executes the resolved network change while interpreting the text or opening the proposal, the confirmation dialog or authoritative snapshot assertion fails.
- **Boundary:** Desktop UI E2E through the Tauri app, Agent intent IPC, and proposal/cancel IPC. The test reads the Agent snapshot through IPC as a read-only observation.
- **Suite:** `agent`, included in `hermetic`; the existing suite registration in `tauri-e2e/spec-suites.ts` and Windows PR workflow execute this spec.
- **Environment:** Tauri E2E binary with the `e2e` feature, the suite's default `stale-proxy` fixture, `zh-cn` language, Agent route `/main/assistant`.
- **Real dependencies:** Agent UI, `agent_resolve_intent`, proposal creation, cancellation, and read-only `agent_get_network_snapshot`.
- **Fixture boundary:** The fixture supplies a deterministic snapshot and does not mutate the host proxy. The test never clicks Confirm; it does not prove native authorization, host proxy changes, or recovery.
- **Initial state:** Read the fixture snapshot and require `system_proxy.observed_enabled === true`. If the intent card is not rendered yet, start diagnosis through its visible UI control and wait for the card.
- **Operation:** Type the supported phrase into the intent input, submit once, then select Continue once.
- **Expected UI:** The supported intent message appears, followed by the proposal dialog describing the system-proxy change. The enabled confirmation action remains available to the user but is never activated by this contract.
- **Authoritative result:** Read `agent_get_network_snapshot` before cancellation and after cancellation; both observations must keep `system_proxy.observed_enabled === true`.
- **Persistence and restart:** Not covered. The fixture is in-memory and the test makes no host change.
- **Failure and cleanup:** If proposal creation is attempted, use the visible Cancel control even after an assertion failure. Preserve the original test error and aggregate a cleanup error. The harness owns the isolated runtime directory and removes it at teardown.
- **Waits and diagnostics:** Use bounded 15-second waits for rendered UI state. The fixture and fixed language make text assertions deterministic; harness failure artifacts stay in the local `.tmp` directory.
- **Rules and limits:** Covers `T01`, `T02`, `T03`, `T04`, `T05`, `T07`, and `T10`. The embedded driver click path is synthetic and does not prove native pointer input.

## Local execution record

- The focused WebKit run passed this case: `1 passing`.
- The complete Agent suite later passed `7 passing` on macOS WebKit 605.1.15 with the same E2E app binary at `/Volumes/ChimeraBuild/chimera-pr472-agent-e2e/debug/chimera` (SHA-256 `346296b58a289a863748f07c6ea6d7abefe0df0f4ee3422b9145f6fd09cf8a39`).
- Neither run changes host proxy or TUN settings; real native authorization and host networking remain unverified.
