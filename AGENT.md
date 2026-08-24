# Chimera Agent

Chimera Agent is a feature-gated, local-only diagnostics and guided-repair subsystem. It exposes privacy-safe status tools through the desktop UI and an authenticated loopback HTTP bridge. It does not provide arbitrary command execution, arbitrary file access, or unattended network changes.

## Cargo feature boundary

The Tauri crate enables the `agent` feature by default:

```toml
[features]
default = ["default-meta", "agent"]
agent = ["dep:subtle"]
```

All Agent Rust modules, Tauri commands, managed state, Specta exports, and invoke-handler registrations must remain guarded by `#[cfg(feature = "agent")]` or live below the guarded `features::agent` module. Both configurations are required to compile:

```bash
cargo check --manifest-path backend/Cargo.toml -p chimera
cargo check --manifest-path backend/Cargo.toml -p chimera --no-default-features --features default-meta
```

The dedicated `.github/workflows/agent-ci.yaml` workflow enforces this boundary together with Agent tests, generated TypeScript bindings, frontend builds, and updater-manifest safety tests for the published Chimera Client artifact mapping. It also exports bindings under the no-Agent configuration and fails if any Agent command or `Agent*` type remains in that IPC surface. A separate Windows, Ubuntu, and macOS matrix compiles both feature configurations and runs the complete History and Bridge test modules on each platform. Ubuntu and macOS therefore execute the Unix owner-only permission, symbolic-link, and directory-sync paths, while Windows executes the reparse-point and write-through replacement paths.

## Architecture

```text
Tauri commands
      |
      v
NyanpasuClient facade
      |
      v
AgentClient
      |
      +-- AgentProposalActor --> planning --> runtime / confirmation ports
      +-- AgentHistoryActor  --> history store --> persistence port
      +-- AgentBridgeActor   --> authenticated loopback Bridge
                                           |
                                           v
                                      Tool Registry
                                           |
                 +-------------------------+----------------------+
                 |                         |                      |
          Diagnostics snapshot      Network probe          privacy projection
```

Tauri commands remain transport adapters: they obtain the managed `NyanpasuClient`, project the calling window label when ownership is required, and delegate. `AgentClient` owns typed clients for three anonymous ractor actors; it never resolves actors through the global name registry. Each typed client shares an `Arc` lifecycle handle, and dropping the final clone sends the actor stop signal. Actors own proposal, history, and Bridge runtime state without external mutexes. Proposal confirmation and execution are moved out of the proposal actor mailbox into actor-owned tasks, so one native confirmation cannot block unrelated propose or cancel messages. Those tasks share a single execution semaphore, are aborted when the actor stops, and project unexpected task panics to the closed `ActionFailed` result. Proposed and completed outcomes are sent to the dedicated history actor from those background tasks and awaited before the corresponding client response, preserving audit durability without holding the proposal mailbox during slow storage. `AgentRuntimePort` is an orchestration boundary assembled from explicit configuration, core-lifecycle, routing-probe, mutation, service-control, system-proxy, and telemetry ports. Confirmation, tool execution, and history persistence are separate injected ports. Legacy configuration globals, the core singleton, module-level service state, Tauri managed state, and `sysproxy` I/O are confined to named implementations in `adapters/`; runtime orchestration, snapshot assembly, diagnostics, planning, and public models depend only on Agent-owned projections and port traits.

The Tool Registry is the single source of truth for tool names, versions, descriptions, risk classification, read-only status, input shape, timeout, argument validation, and dispatch. One closed static definition table drives the UI/Bridge manifest, request validation, timeout lookup, and execution-kind dispatch, so those surfaces cannot drift independently. Manifest, tool, input-schema, snapshot-output, and probe-output versions are centralized constants; registry tests require every public schema version to be nonzero and equal to its declared contract. Public tool names are exported as the closed `AgentToolName` union rather than arbitrary strings; descriptions are copied only from the same compile-time static table. Every serialized tool result also passes through one recursive privacy-shape validator that rejects sensitive key fragments such as token, secret, URL, host, address, target, log, and bypass—including compound snake_case and camelCase names—before the value can reach the Bridge response. The five explicit `contains_*` privacy metadata fields remain allowed only when their value is the boolean `false`; a positive or malformed privacy assertion fails closed. Public core selection is projected from the application configuration into the closed `AgentSelectedCore` enum before it enters snapshots, tool output, action preconditions, or generated TypeScript bindings, so an arbitrary configuration string cannot propagate through the Agent surface. Operating-system identity is likewise projected from the compile target into the closed `AgentOsFamily` enum, with unrecognized targets represented as `unknown` instead of propagating an open platform string. Telemetry speeds and cumulative transfer totals remain numeric byte counters throughout the Rust and generated TypeScript contracts; upstream values are never converted into open strings. Agent CI also scans production modules to reject process or Shell APIs, confines direct filesystem APIs to the bounded history store, and verifies that the Bridge reaches diagnostic implementations only through the Registry.

## Local Agent Bridge

The Bridge lifecycle is controlled explicitly from the Agent page.

- It binds only to an operating-system-assigned loopback address. The runtime retains that listener as a validated `SocketAddr`, rejects non-loopback or zero-port endpoints, and derives the public HTTP base URL and health-check URL from that closed endpoint type instead of retaining an arbitrary URL string.
- Creating a new Bridge runtime returns a bearer token once to the initiating UI call.
- Repeating the start command while the same runtime is healthy returns its address but never discloses the token again.
- The token is held by the HTTP server in memory only and is not retained by the lifecycle state or written to configuration, history, logs, React Query cache, or diagnostics.
- The UI hides an unclaimed token after 60 seconds. After copying, it immediately removes the token from component state and clears the clipboard after 30 seconds only when the clipboard still contains that exact token, so newer clipboard content is never overwritten. Component lifecycle tests cover expiry, stop cleanup, stale-timer isolation, unchanged-value clipboard clearing, and preservation of newer clipboard content.
- Requests must use an exact `Authorization: Bearer <token>` header. Token bytes are compared with a constant-time equality primitive rather than ordinary string equality.
- A health endpoint is used to detect stale or unreachable runtimes.
- If the task exits or the endpoint becomes unreachable, status checks clean up the stale runtime. A later start creates a new port and token.
- Stopping the Bridge allows up to two seconds for graceful shutdown, then aborts and joins an unresponsive task so the lifecycle command remains bounded.
- A completed shutdown releases the listener port.
- Bridge lifecycle tracing uses fixed messages only: it does not attach the bearer token, listener address, base URL, or raw server/join errors. Token-bearing lifecycle result types intentionally do not implement `Debug`, preventing accidental debug-log disclosure.

Public Bridge responses use request IDs and structured errors. Tool calls are subject to bounded timeouts, an in-flight concurrency limit, and per-window rate limiting. Authentication, tool lookup, body-size enforcement, and request-schema validation happen before an execution permit is acquired, so rejected traffic cannot consume a tool slot or access application state. Each accepted tool runs in a registered task that owns its semaphore permit. If the HTTP timeout expires, the response returns `tool_timeout` but the real task keeps the permit until it finishes, preventing timed-out work from being hidden behind newly available capacity. If the client disconnects first, the request-owned task guard aborts execution and releases the permit. Bridge stop, stale-runtime reconciliation, and unexpected server exit close the task registry, reject new registrations, and abort all unfinished executions. Executor panics and task-join failures are projected to the static `execution_failed` response without reflecting panic text. Invalid authentication, malformed payloads, unknown tools, rejected arguments, timeouts, overload, and internal failures are returned as stable error codes rather than raw internal errors. Error codes and messages are compile-time static strings; HTTP canary tests verify that rejected credentials, unknown tool names, target URLs, query parameters, request bodies, and panic canaries are never reflected in an error response. Additional integration tests verify status codes and request-ID correlation, prove that excess concurrent requests never enter the executor, confirm that unauthenticated traffic does not consume the authenticated request budget, and cover timeout capacity retention, registry shutdown cancellation, rejected post-shutdown registrations, disconnect cancellation, panic projection, and permit reuse.

The Agent page includes a localized “still not resolved?” path to the official GitHub bug form. Opening it is an explicit user action. Chimera pre-fills the existing environment field and a narrow Agent summary containing only schema version, health, finding codes, and probe-failure codes. The URL builder never receives raw errors, logs, tokens, controller secrets, subscription URLs, or connection targets, and frontend canary tests keep those values out of the generated link.

## Tool manifest

The read-only registry currently contains:

| Tool               | Purpose                                                                                              |
| ------------------ | ---------------------------------------------------------------------------------------------------- |
| `system.snapshot`  | Return the complete privacy-safe Agent snapshot.                                                     |
| `network.diagnose` | Return findings derived from the current snapshot.                                                   |
| `network.probe`    | Probe an explicitly supplied public HTTP(S) endpoint with validated timeout and status expectations. |
| `core.status`      | Return core state, run type, selected implementation, routing mode, and consistency state.           |
| `proxy.status`     | Return desired and observed system proxy state without raw host or bypass data.                      |
| `tun.status`       | Return desired, generated, and controller-observed TUN state and consistency.                        |
| `profile.summary`  | Return counts and active-reference validity without profile names or URLs.                           |
| `service.status`   | Return desired, observed, IPC, and runtime-compatibility service state.                              |

`network.probe` rejects credentials, local hostnames, private IP ranges, loopback, link-local, multicast, documentation, benchmarking, and other non-public address ranges. URLs are limited to 2048 bytes. DNS resolution receives at most three seconds of the request's total budget, every returned address is checked before use, duplicate addresses are removed, and no more than 16 public addresses are pinned into the HTTP client. The caller-selected timeout is a total DNS-plus-request budget between one and ten seconds; the Bridge registry allows a separate 12-second protective tool budget so the inner operation can return its stable timeout error instead of being preempted by the outer guard. Redirects remain disabled.

## Privacy boundary

Agent output must never include:

- Bridge bearer tokens;
- controller secrets;
- subscription or profile URLs;
- profile names unless a future explicit privacy review permits them;
- connection destinations or target domains;
- raw core logs;
- full runtime configuration;
- raw system proxy host or bypass lists.

Diagnostics use summaries, enums, counts, booleans, expected ports, state timestamps, and stable finding codes. Core-status, service-status, controller configuration, and operating-system proxy probes have explicit infrastructure timeouts. The loopback controller probe reads both the observed routing mode and `tun.enable` in one request, disables proxies and redirects, accepts at most 64 KiB of JSON, checks both declared length and streamed chunks, and never uses an unbounded response `.json()` helper. If a running core omits its TUN state, the snapshot records `tun_status_unavailable` and TUN write actions fail closed. The system-proxy adapter owns the only `Sysproxy` values and a shared single-flight gate for probes, reads, writes, and desired-state mutations; blocking tasks retain the owned permit until completion, and an uncertain write timeout is reported as partially applied rather than as a clean failure. Core start, restart, service control, and telemetry reconnection likewise map both timeout and post-dispatch internal failure to `PartialApply` when no rollback can prove a clean state. Snapshot assembly and diagnostics receive only the closed `SystemProxyConfiguration` projection, and raw host/bypass data is never serialized. A core-status timeout is projected as `core.state = unknown`, `run_type = unknown`, and `core_status_timeout`; it is never misrepresented as a stopped core. Unknown core identity makes every write plan unavailable and prevents recommendations from advertising executable actions. The UI's context copy action copies only this projected snapshot.

## Guided repair model

Write actions are not Bridge tools. They are available only through the desktop proposal flow:

1. Collect a fresh snapshot.
2. Confirm that the action is available in the observed state.
3. Build a proposal containing risk, impacts, before/after changes, snapshot revision, expiry, and digest.
4. Require explicit user confirmation in a native dialog.
5. Re-read state and validate action-specific preconditions.
6. Execute the narrow operation.
7. Poll asynchronous service or IPC transitions where necessary.
8. Collect a final snapshot and verify the expected state.
9. Record the outcome in the audit history.

Proposals are owner-scoped, expire after a short interval, are rate limited, have global and per-owner capacity limits, and are consumed on execution. Owner and constant-time digest checks happen before removal, so invalid references cannot consume a valid pending proposal. A digest mismatch, stale state, expiry, declined confirmation, or execution panic prevents a verified result. Long confirmation and execution work runs outside the proposal mailbox but under one actor-owned execution permit; waiting proposals re-check expiry and preconditions before any mutation, and actor shutdown aborts all outstanding execution and response tasks.

Supported repairs include explicit TUN enablement or disablement, explicit system-proxy enablement or disablement, service-mode enablement or disablement, routing-mode changes, idempotent core start, core restart, telemetry reconnection, service start/stop/restart, stale system-proxy disablement, and restoration of a mismatched system-proxy endpoint. Core start checks and starts under the same lifecycle lock, so concurrent callers cannot turn an idempotent start into an unintended restart. The desktop UI includes a deterministic, 160-character-bounded intent resolver for those reviewed actions and explicit diagnostics. Ambiguous proxy wording returns fixed clarification choices, and unsupported text never becomes a command. Resolved write intents still enter the normal proposal, native confirmation, precondition, execution, verification, rollback, and audit flow. TUN changes are accepted only from a consistent running source state with a valid active profile and a controller-observed TUN value matching the desired and generated source state. The proposal records possible core restart, connection interruption, platform DNS impact, and administrator-permission requirements. Execution awaits narrow application capabilities such as `set_tun_enabled` and `set_system_proxy_enabled`, then polls diagnostics under a five-second hard deadline before restoring the previous target when execution or verification fails. TUN verification requires the desired setting, generated runtime configuration, and controller-reported `tun.enable` to all equal the target; system-proxy disablement requires the operating-system proxy probe to explicitly report disabled. A stalled observation is bounded by the remaining deadline, and missing observations never count as successful closure. Telemetry reconnection is available only while the core is running and the connector is explicitly disconnected; it restarts the existing managed connector and verifies that the same core instance reaches the connected state. Operations attempt rollback when a multi-step change fails. If rollback or final state cannot be proven, the result is reported as partially applied rather than as a clean failure.

## Diagnostics and audit history

Chimera stores a bounded, privacy-safe history document under the application data directory:

- up to 100 diagnostic snapshots;
- up to 200 proposal/audit outcomes;
- consecutive diagnostic entries with the same revision are deduplicated.

The persisted document contains only timestamps, fixed-format revisions, health and state enums, finding/probe codes, irreversible proposal references, action kinds, and a closed audit-outcome enum. A history file larger than 1 MiB is rejected before JSON parsing; reads are capped at `limit + 1` bytes so a replaced or growing file cannot trigger unbounded allocation. All blocking history reads, writes, permission repairs, durable syncs, and Windows replacement operations share a single permit with bounded acquisition and execution timeouts. If an outer timeout expires, the detached blocking task retains that permit until it actually exits, preventing later requests from accumulating additional blocking threads. Its document and entry types reject all unknown JSON fields, so injected or obsolete fields cannot be silently accepted as valid history and are handled by the existing quarantine-and-recovery path. Runtime proposal IDs are 32-character lowercase hexadecimal values; history stores only a SHA-256-derived 32-character reference, migrates legacy identifiers to that form, and discards entries whose 64-character snapshot revision is not valid lowercase hexadecimal. Arbitrary outcome text cannot enter persistence, tracing, or the generated frontend contract; action audit tracing records only the stable action kind and outcome code, not proposal IDs or snapshot revisions. The public history response derives a summary from that retained data: latest health, first-to-latest health trend, unhealthy sample ratio, verified action ratio, and frequency counts for finding and probe codes. These aggregates do not add new persisted or sensitive fields. History mutations are serialized through one store transaction, so concurrent diagnostic and audit writes cannot overwrite each other. The in-memory cache is advanced only after persistence succeeds. Writes exclusively create the fixed recovery temporary path, flush and `sync_all` its contents before replacement, and sync directory metadata after Unix rename or removal operations. A pre-existing file or symbolic link is never opened with truncation. Existing history entries must be regular files. Unix reads verify that the opened descriptor still identifies the inspected inode before repairing owner-only `0600` permissions. Windows opens the final component with `FILE_FLAG_OPEN_REPARSE_POINT`, rejects `FILE_ATTRIBUTE_REPARSE_POINT`, keeps the current user's inherited application-data ACL, and commits writes with `MoveFileExW` using `MOVEFILE_WRITE_THROUGH`; normal writes atomically replace the primary entry, while startup recovery and quarantine moves cannot overwrite an existing destination. On startup, a valid temporary document is promoted only when the primary file is missing; stale, invalid, or non-regular temporary entries are removed, so interrupted writes recover without overriding a committed history. Replacement failures discard the temporary copy while the committed primary still exists, but retain the already-private temporary only when the primary has been removed and startup recovery is required. A malformed history document is moved aside as `agent-history.corrupt-<timestamp>-<128-bit nonce>.json` and the store recovers with an empty privacy-safe document, preventing repeated parse failures and same-millisecond destination collisions without logging file contents. Retention accepts only the legacy nonnegative timestamp form or the new exact 32-character lowercase hexadecimal nonce form; malformed lookalike names are never sorted, deleted, or permission-repaired as history artifacts. History persistence warnings are fixed messages without raw I/O errors, application-data paths, temporary-file paths, or quarantine filenames. A retention pass scans at most 4096 directory entries with constant memory and keeps at most the three newest valid quarantined files observed within that budget; reaching the budget stops cleanly with a stable warning rather than extending one history RPC indefinitely. Clearing is enforced behind the backend confirmation port rather than trusting the frontend: the Tauri command projects the calling window label, a native warning dialog is shown for that window, rejection returns the stable `agent_confirmation_declined` cancellation without sending a clear message, and a 60-second confirmation timeout fails closed before the history actor receives a clear message.

## Error codes

Desktop action and lifecycle commands use stable string codes. Specta exports them as the `AgentCommandError` string-literal union, and the frontend maps that union exhaustively to localized and actionable text. Adding a backend code without a frontend mapping therefore fails the TypeScript build instead of silently falling back in production.

| Code                               | Meaning                                                     |
| ---------------------------------- | ----------------------------------------------------------- |
| `agent_action_not_available`       | Current state does not safely permit the requested action.  |
| `agent_proposal_not_found`         | Proposal is missing, consumed, or owned by another window.  |
| `agent_proposal_expired`           | Confirmation or execution happened after expiry.            |
| `agent_proposal_digest_mismatch`   | Proposal contents did not match the supplied digest.        |
| `agent_network_state_changed`      | Preconditions changed after proposal creation.              |
| `agent_proposal_rate_limited`      | Proposals were created too quickly.                         |
| `agent_proposal_limit_reached`     | Pending proposal capacity was reached.                      |
| `agent_confirmation_declined`      | The user declined the native confirmation.                  |
| `agent_action_failed`              | The action failed without a verified partial state.         |
| `agent_action_partially_applied`   | A side effect may remain or rollback could not be verified. |
| `agent_action_verification_failed` | Execution returned but final state did not match.           |
| `agent_bridge_start_failed`        | The local listener could not be started.                    |
| `agent_history_clear_failed`       | Persistent history could not be cleared safely.             |

Bridge protocol errors additionally include the request ID so clients can correlate responses without exposing logs or secrets.

## Development verification

Run the focused checks after Agent changes:

```bash
cargo fmt --manifest-path backend/Cargo.toml --all -- --check
cargo test --manifest-path backend/Cargo.toml -p chimera features::agent --lib
cargo test --manifest-path backend/Cargo.toml -p chimera specta_export::tests::typescript_bindings_are_fresh -- --exact
cargo check --manifest-path backend/Cargo.toml -p chimera
cargo check --manifest-path backend/Cargo.toml -p chimera --no-default-features --features default-meta
pnpm --filter @chimera/interface build
pnpm exec tsx --test frontend/chimera/src/features/agent/components/bridge-token-lifecycle.test.ts frontend/chimera/src/features/agent/components/issue-guidance.test.ts frontend/chimera/src/features/agent/model/privacy-safe-context.test.ts
pnpm --filter chimera-ui build
```

The generated `frontend/interface/src/ipc/bindings.ts` file must stay synchronized with Rust Specta exports. Do not hand-edit it unless reproducing the exact generated output and immediately validating it with the binding freshness test.

## Security review checklist

Before adding a tool or repair action, verify all of the following:

- The capability cannot execute arbitrary shell commands.
- It cannot read or modify arbitrary paths.
- Read-only output is explicitly projected and serialized without sensitive fields.
- Network inputs reject local and non-public destinations where SSRF is possible.
- Time, payload size, concurrency, and request-rate bounds are explicit.
- Write behavior is narrow, proposed first, confirmed by the user, and verified afterward.
- Failure and rollback semantics distinguish clean failure from partial application.
- Tokens and secrets are absent from tracing, persistence, clipboard previews, and frontend caches.
- Both the default build and `--no-default-features --features default-meta` build compile.
