# RuntimeReconciler attempted-target policy (reference alignment)

## Baseline and source mapping

- Chimera starting point: `4d835a30b04f9b7a6c54c0099177cdd9a0c36890`.
- Fixed upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (do not advance this gitlink).
- The upstream authoritative rules are `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs::plan` and `ref/backend/nyanpasu-core/src/client/reconciler/apply.rs::not_submitted_outcome`.
- The target/host/outcome/trigger types were already present in Chimera's pure `backend/chimera-core/src/client/reconciler/plan.rs`; their visibility is extended to the single mutating client owner. The compatibility integration is `backend/tauri/src/client/reconciler/attempt.rs` and `backend/tauri/src/client/core_lifecycle/workflow.rs::reconcile_with_trigger`.

## Behavior

1. One CoreLifecycle workflow holds an attempted-target cache. It keys refusals to the full `RuntimeInputs::target_key()` (application, Clash, Profiles, frozen managed file content), and to the execution host (Local or Service).
2. `SourceCommitted`, service events, external Core changes and startup observations are **not** forced retries. A previously refused identical target/host is reported as `NotInEffect`, not repeatedly submitted.
3. Explicit `Start` and `Retry` may reattempt a refused target. A different runtime target, a different execution host, or a successful explicit retry removes the old refusal's effect. Stop intent continues to take precedence at the CoreLifecycle mailbox.
4. Four not-submitted dependency errors match upstream's `Waiting` classification regardless of the `CoreError.retryable` hint: `RevisionConflict`, `OperationConflict`, `QueueFull`, `BackendUnavailable`. Such results are not cached as permanent refusals. An uncertain result is also never treated as an ordinary refusal; the existing uncertain-operation gate still blocks unsafe work.
5. A single `RunType` is selected for both the host-scoped attempt key and the submission so a mid-capture IPC-state change cannot falsely attribute one host's refusal to the other.
6. The source commit remains durable if Core application fails. A blocked attempt changes neither the document nor the managed file; it is a runtime conclusion.

## Validation contract

- Unit tests in `client/reconciler/attempt.rs` cover source-notification blocking, explicitly forced triggers, host and target scoping, transient dependencies, uncertain outcomes, and successful explicit retries.
- One CoreLifecycle Actor regression verifies the sequence: committed target rejected → repeated notification is blocked without another Core call → explicit retry successfully applies the same committed target.
- Another verifies that a genuinely different source target applies automatically after an earlier target was refused.
- A ReconcilerStatus unit test verifies that all four dependency errors report `Waiting`, not `NotInEffect`.
- The existing suite also covers service readiness, explicit stop intent, concurrent source advances, stale CAS and source file compensation. Use the full `chimera-core` and Tauri library tests, rustfmt, Clippy and `git diff --check` before delivery.

## Remaining architecture gaps and release boundary

This is a scoped migration of **upstream policy into the existing single owner**, not yet a separate upstream RuntimeReconcilerActor. The currently retained `ApplicationWorkflow` and `CoreLifecycle` still coordinate actual Core/Service handoff. The remainder of upstream `plan(Facts)`, including verified ownership, Service failure fallback to Local, per-host applied revision proof, release maintenance and a complete independent runtime Actor, requires a dedicated subsequent integration. Avoid spawning a second writer simply to match the module name.

The new tests use injected Core/Service ports. Production Core binary behavior, daemon handoff and real desktop E2E remain unverified by these tests; do not report them as passed. No persistence schema, IPC interface, UI behavior, gitlink or new worktree is changed. Rollback is a normal revert of the migration commits in reverse order.
