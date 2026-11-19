# RuntimeReconciler: S2 owner-step continuation through the existing mailbox

## Scope and reference

- Base Chimera master: `e5275825cbf8c2c71a2f0282c3bf304c21d2e68a`. At integration, fetch and preserve all subsequent remote commits, including independent UI changes.
- Pinned `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (unchanged).
- Ref source: `ref/backend/nyanpasu-core/src/client/reconciler/{plan.rs,workflow.rs,apply.rs,startup.rs}`; S2 `Step::Owner` is a prerequisite to S3/S4, not an error that permanently consumes a committed source.
- Chimera mapping: `client/reconciler/attempt.rs::OwnershipTransitionPending`, `client/core_lifecycle/{workflow.rs,mod.rs}`, existing `ApplicationWorkflow::follow_service`. The CoreLifecycle actor remains the **only Core mutation executor**; no additional runtime actor or Service writer.

## Objective, constraints and risk

L2 runtime ownership migration. Make a committed source blocked by a concrete S2 Owner step automatically resume S3/S4 **after the existing serialized Service handoff succeeds**, without replaying the prehandoff Candidate or demanding a second SourceCommitted notification.

Non-goals: replace StartupWorkflow's leased owner transition, change run intent, alter Service/Local OS permissions, expand public IPC, modify persisted configuration, ref or runtime gitlinks, or introduce a new worker or polling loop. Failure of a Service transition must not be interpreted as permission to start Local, and an unreadable probe must not acquire a transition lease.

## Decision and execution sequence

1. Ref `plan(Facts)` is already called with the latest source, selected host, active receipt, ServiceActor evidence and recoverability. Its concrete `Step::Owner(OwnerPlan)` becomes `OwnershipTransitionPending::owner(plan)`; generic S2 `Waiting`, stale Core status, receipt replacement and foreign-owner uncertainty become `OwnershipTransitionPending::waiting(reason)`. Both remain retryable dependency waits, but **only the concrete Owner variant** authorizes recording a continuation.
2. When the CoreLifecycle actor settles a source-driven S2 owner result, it latches one `source_after_handoff` intent. `follow_service()` already performs a second Service readiness/ownership check, obtains the nonblocking Service transition lease, and inserts the existing `FollowService` at the front of the serialized mailbox. Its pre-existing safety gates (stopped intent, uncertain operations, unproven startup ownership, transition lease and queue limits) remain operative.
3. After **successful** `FollowService`, if runtime intent is Running and there is no uncertain/unproven/shutdown state, the actor consumes that single continuation flag, coalesces a source-driven reconcile, and re-reads the **latest committed** Application/Clash/Profiles and managed files. The new round reruns S2 then S3/S4. If the handoff itself applied that target, the ref planner skips redundant apply using the authoritative receipt; otherwise it applies once under the established host.
4. Failure leaves the continuation pending for a later readiness/recovery event or explicit user action; it does not spin a retry timer. Explicit StopCore and shutdown clear it. Generic Waiting does **not** authorize the follow-up continuation or turn an unknown owner into an actionable step.

## Acceptance and regression evidence

- The `attempt.rs` unit test distinguishes actionable `OwnerPlan` from ordinary S2 waiting, while preserving both as dependency waits.
- An Actor integration fixture starts with no daemon, establishes a real StartupReport, applies a source, then exposes an additional owned Service Core during a newer committed Profiles source notification. Without sending another source notification, it must produce one safe owner retirement, one updated Core submission, and a second `SourceCommitted` round that proves Converged. Diagnostics include the operation ledger and round history if the sequence stalls.
- Full `chimera-core` and Tauri Rust library test suites, Rustfmt, Clippy, `git diff --check`, and signed commits are the gates. The fixture's authoritative stopped-baseline port is explicitly implemented so it does not obtain false startup clearance from an incomplete status mock.
- Existing tests cover unreadable Service evidence, instance/receipt replacement during asynchronous fact sampling, release-owed cleanup, stopped intent, Profile source fencing and Service fallback. These are **injected port tests**, not macOS/Windows/Linux installed-service, TUN or desktop user-flow E2E.

## Remaining differences and rollback

The ref has a standalone RuntimeReconciler actor with its own lifecycle and full round planner. Chimera still uses a compatibility ApplicationWorkflow for S2 execution, while the CoreLifecycle actor schedules S3/S4. This change closes the one-shot continuation gap, but it does not migrate the complete ref actor or promise automatic recovery when no startup owner has ever been established. Retain the main and legacy UI and agent's single shared API. Revert this scoped code commit, then the design note if rollback is needed; do not reset remote UI work.
