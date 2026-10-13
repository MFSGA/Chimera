# Agent TUN readiness and proposal contract

- **Contract ID:** `agent-tun-readiness`
- **Behavior:** On platforms where TUN is available, the Agent action panel permits a TUN enable proposal; confirming it updates the fixture-backed UI state. A second, separately confirmed proposal disables TUN. Windows keeps enable unavailable unless its Service prerequisites are met.
- **Regression:** The UI exposes an unavailable action, proposes the wrong target, or reports success without updating the fixture snapshot and current controls.
- **Boundary:** Desktop UI E2E through the Tauri app, proposal IPC, and execute IPC. It verifies the current UI after each simulated action.
- **Suite:** `agent`, included in `hermetic`; the existing spec remains registered in `tauri-e2e/spec-suites.ts`.
- **Environment:** E2E-feature Tauri binary, `stale-proxy` fixture, Agent route, Chinese locale, one desktop window.
- **Real dependencies:** Agent action panel, proposal lifecycle, current Agent snapshot query, and platform readiness checks.
- **Fixture boundary:** The E2E fixture keeps TUN state in process memory and bypasses native confirmation and host configuration changes. This does not prove native authorization, real TUN setup, host routes, or recovery.
- **Initial state:** The fixture starts with TUN disabled and a stopped core. Enable is expected on non-Windows; Windows requires a running Service-backed core. Disable is unavailable while TUN starts disabled.
- **Operation:** Read action availability, propose and confirm TUN enable once, wait for the proposal dialog to close and current UI controls to reflect enabled state, then propose and confirm disable once and verify the controls return to disabled state. Operations are outside polling.
- **Authoritative result:** The proposal confirmation and returned fixture snapshot update the current controls. The test does not claim persistence or host state.
- **Cooldown:** The backend limits proposal creation to one per window every 500 ms. After the enable result is verified, a bounded read-only clock wait (600 ms from the request, with a 1.5-second limit) ensures the second UI action begins after this explicit eligibility window; it does not retry or repeat the action.
- **Failure and cleanup:** Fixture mutations exist only in the app process and disappear when the harness terminates it. No system proxy or host networking settings are changed.
- **Waits and diagnostics:** Modal closure and UI convergence are bounded at 15 seconds; cooldown observation is bounded at 1.5 seconds. Failures retain WDIO output and harness artifacts in the run's isolated directory.
- **Rules and limits:** Covers T01, T02, T03, T05, T06, T07, and T10. The provider click path is synthetic and does not establish native pointer input.

## Local execution record

- The first full macOS WebKit run exposed the second TUN proposal hitting the 500 ms backend cooldown. The test now has a bounded read-only wait for that eligibility window; it does not repeat the action.
- The final full Agent suite passed `7 passing` with E2E binary `/Volumes/ChimeraBuild/chimera-pr472-agent-e2e/debug/chimera` (SHA-256 `346296b58a289a863748f07c6ea6d7abefe0df0f4ee3422b9145f6fd09cf8a39`).
- This fixture only changes process-local state. No host networking, TUN authorization, or real Service behavior was tested.
