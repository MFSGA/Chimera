# Runtime safety: retain committed-source intent under a full lifecycle queue

## Baseline and reference

- Chimera master at the start: `ab6ee6263f6b13ee2523746c38e10355f73e429c`.
- Fixed upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; fixed runtime gitlink: `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Ref behavior: `ref/backend/nyanpasu-core/src/client/reconciler/{workflow.rs,plan.rs,tests/commit_first.rs}` retains a durable source and treats lower queue saturation as a recoverable dependency, not a successful runtime application.
- Scope: Chimera's compatibility `backend/tauri/src/client/core_lifecycle/mod.rs` Actor, `DirtyTick` admission only. No change to Core/Service ownership, Runtime artifact state, public IPC, TUN, persisted configuration, or submodule versions.

## Reproduced defect

The Actor coalesces source and runtime updates into `dirty`/`source_dirty`. On `Message::DirtyTick`, the original implementation cleared both flags **before** invoking `push_background(Command::ReconcileSource)`. With `MAX_PENDING=32` queued lifecycle commands, `push_background` discarded the command, but the dirty intent had already been consumed. The latest durable source remained stored without an automatic runtime apply.

A new Actor integration test uses an injected Core that blocks its first reconcile, then fills the mailbox with 32 accepted but pending lifecycle requests and commits a source version. The test waits until the dirty coalescing window expires, releases the Core, drains the queue, and verifies a second Core reconcile and one `SourceCommitted` round.

With the final test's actual behavioral assertions and the old implementation, this fails: Core reconcile count remains 1, the pending queue becomes empty, and no `SourceCommitted` round is recorded. The exact failure was captured at `/tmp/chimera-queue-dirty-baseline-red.log`. With the fix the same test passes.

## Fix and invariant

At `DirtyTick` dispatch, test queue capacity **before** clearing `dirty` and `source_dirty`. If the queue is full, retain both flags and skip enqueue. Existing completion handling already calls `rearm_deferred_dirty_after_owner`, which, once an active operation completes, schedules a single coalesced dirty tick when the actor is running, not shutting down, not uncertain, and no S2 owner transition is pending.

This requires no new timer loop: it reuses the existing dirty coalescing event, serial Core executor and source-version fence. A stopped or uncertain runtime remains blocked by the existing gates. Later updates continue to collapse into the freshest committed snapshot, not replay an old candidate. Source-only notifications are not changed.

## Acceptance, risk and rollback

- Risk: L2 runtime correctness, deliberately narrow.
- Required: the targeted full-queue regression, existing dirty burst and queued owner lease tests, complete `chimera-core` and Tauri Rust library suites, rustfmt, Clippy, `git diff --check`, signed commits and ordinary fast-forward push.
- This injected-port test proves command retention and re-execution, **not** production Core receipt verification or a healthy actual proxy connection. Existing receipt/target and runtime product tests still gate convergence. macOS/Windows/Linux installed Core/Service/TUN/DNS E2E remains separate.
- Rollback is one scoped code commit and this record. Do not alter pinned gitlinks or unrelated remote UI work.
