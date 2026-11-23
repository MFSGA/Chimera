# RuntimeReconciler: unified S2→S3/S4 committed-source Facts sampling

## Baselines and reference

- Chimera base `master`: `5bbeec756104693a6d0abb6c7efda7cf3663643a`.
- Fixed upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (no submodule update).
- Reference semantics: `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs::plan(Facts)` and `workflow.rs` (ownership and runtime facts sampled before one planned round).
- Chimera implementation: `backend/tauri/src/client/reconciler/decision.rs` (`FactFrame`), `client/core_lifecycle/workflow.rs::planned_committed_round`, `client/application_workflow/startup.rs::s2_step`, and the existing single CoreLifecycle mailbox.

## What is now shared

The S2 startup/readiness stage and the source-committed S2→S3/S4 stage use the same **ref-shaped Facts assembly boundary** rather than separately constructing two `Facts` objects with incompatible ownership assumptions.

Startup S2 calls `plan_owner_boundary` with `Ownership::Unproven`, fresh Service evidence and an intentionally unreadable Core state. This can select only a concrete ownership transition or a dependency wait; it cannot authorize Core Apply/Stop. The existing StartupWorkflow still owns and executes the chosen handoff under the Service transition lease, retaining its accurate waiting/recovery diagnostics.

A `SourceCommitted` or explicit `Retry` now captures latest committed inputs and samples the selected Core owner, active accepted receipt, previous target/host attempt, fresh Service ownership evidence and Service endpoint recovery status into one `plan_committed_target` round. Ownership is initially unproven, so **S2 is evaluated before S3/S4** even if the desired host equals the current host:

- `Step::Owner(...)` becomes typed `OwnershipTransitionPending`, classified as `Waiting`. No direct Core submit/stop occurs. The existing CoreLifecycle completion path requests its ordinary serialized Service follow-up, which alone is allowed to acquire a lease and hand off ownership.
- `Step::Idle(Waiting)` from unreadable or quarantined ownership, unresolved host mismatch, or a recovering Service endpoint blocks the new Core effect and preserves the durable source. A foreign Service Core is left untouched; if Local is already the established desired host, it does not by itself prohibit Local operation.
- With established S2 ownership, `Step::Idle(Converged)` skips reapplication of an exactly proven target; `Step::Stop { intent:false }` stops the outdated accepted Core before the usual source application; and a refused Service target uses the existing single-owner safe Local fallback.
- An unverified Running Core without an authoritative accepted receipt or a status with timestamp zero never becomes a ref S3/S4 stop proof. The existing compatibility/recovery path remains responsible for such cases.

**Sampling fence:** after the asynchronous Service probe, CoreLifecycle reads Core status again and demands the same instance, run type, state and change timestamp. It rechecks accepted receipt pointer identity before acting on the sample. A change yields `OwnershipTransitionPending` rather than an outdated Stop/Apply.

## Regression and test-fixture correctness

- Pure planner tests prove S2 precedes S3, selecting AdoptService/RetireServiceThenLocal before runtime mutation; Service recovery and unreadable ownership wait; established matching targets are no-ops and outdated targets require Stop; previous Service refusal retains fallback precedence.
- Production Actor test: a changed committed Profiles target with a second owned Service Core cannot stop/reapply Local; when the other Service Core stops and a fresh notification arrives, the source advances safely.
- Production Actor test: unreadable Service probe blocks source-driven Core Stop/Apply.
- Production Actor test: changing Core instance during S2 evidence sampling blocks a stale Stop/Apply.
- Full compatibility suites continue to exercise Service handoff leases, stop intent, target refusal, Source CAS, RuntimeProduct publication and release-owed maintenance.
- Legacy **direct-only** Core client tests do not have a ServiceActor or an installed daemon. Their injected Service fixture is now explicitly `NotInstalled` (not a potentially unrelated OS-global legacy status). No production Service evidence rule was weakened.

## Limitations and next steps

This is a scoped **shared facts/preflight migration** in the existing CoreLifecycle executor, not ref's independent RuntimeReconciler actor. The code still uses existing ApplicationWorkflow/StartupWorkflow ownership execution, the compatibility path for running Core without a receipt, and distinct actor command admission. Explicit Start and startup recovery do not yet share this exact committed-source fact frame. Production OS Core/Service/TUN E2E, especially host transitions and no-leak behavior, is still unverified by injected tests.

No persisted config schema, UI wire, public IPC, gitlink, PR, or worktree change is introduced. Require Rustfmt, complete chimera-core and Tauri library tests, Clippy, diff-check and signed commits before a normal fast-forward push.
