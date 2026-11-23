# Runtime safety: do not acquire a Service lease from a stale startup retry after Stop

## Baseline and reference

- Chimera `master` baseline: `fd50782387b5d01a42778bfe519a801edd4edf0a`.
- Pinned `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` and `backend/chimera-runtime`: `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Scope: one guarded `Message::StartupRetryDue` admission in `backend/tauri/src/client/core_lifecycle/mod.rs`, and the existing injected-ports Actor regression.
- Runtime policy: a user-explicit `StopCore` latches `RunIntent::Stopped`. Only an explicit `StartCore` may resume Core execution. See the ref-shaped `plan(Facts)` intent model and `docs/design/2026-11-19-runtime-stop-intent-late-startup.md`.

## Reproduced defect

The command execution gate already rejected delayed `RetryStartup`, `RecoverStartup` and `FollowService` after explicit Stop, but its earlier `Message::StartupRetryDue` handler **still initiated** `service.begin_transition()` while stopped. This unnecessarily acquired or attempted the Service transition lease, potentially delaying other Service operations, despite the resulting Core command later being discarded.

The existing `startup_retry_lease_failure_is_drained_after_actor_operation_returns` test was extended to wait through a rescheduled retry deadline **after StopCore had completed**, measuring the injected Service port's `transition_attempts`. Before the fix the assertion failed: 3 attempts occurred instead of 2. The exact repro log was captured locally as `/tmp/chimera-stopped-retry-lease-red.log`.

## Fix, accepted behavior and safety boundaries

Gate `StartupRetryDue` on `RunIntent::Running` in addition to the already-existing uncertainty and shutdown checks. A timer which fires after explicit Stop does not begin a transition lease or submit another Core command. This is a narrow mailbox-admission change, not an alteration of the Service transition lease implementation, retry budget, Core start/stop mechanics or source persistence. A later explicit Start continues through the existing regular lifecycle path.

After the change, the same red test passes without a third lease attempt. The existing `late_startup_recovery_after_explicit_stop_cannot_restart_core` test also remains passing. The complete `chimera-core` and Tauri Rust suites, Rustfmt, Clippy and `git diff --check` are required before delivery.

## macOS live-instance limitation

During inspection, the host already ran `/Applications/Chimera.app` and `chimera-client-alpha`, with a live listener on port 7890 and system HTTP/HTTPS/SOCKS proxies enabled. These observations were strictly read-only. This task **must not** stop/restart the user's live Core, modify system proxy, DNS/TUN, apply Service changes or reuse the user's application profile for tests. The regression relies on isolated injected ports; Windows GitHub Tauri E2E does not constitute macOS installed-runtime end-to-end verification.

Risk level: L2 runtime admission safety. Rollback: revert the scoped code commit and this design record while preserving all unrelated remote work; no submodule or persisted-data changes.
