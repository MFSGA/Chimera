# Startup owner recovery completion is outcome-scoped

## Baseline and reference

- Chimera starting `master`: `7b61f1504c314015cfea05dbd2dabc7aa9e32e6c`.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`.
- Upstream behavior: `ref/backend/nyanpasu-core/src/client/reconciler/{workflow.rs,startup.rs,plan.rs}` uses one ref-shaped planned round, and treats incomplete ownership recovery as a dependency wait rather than permission to apply a committed target.
- Chimera implementation: `backend/tauri/src/client/core_lifecycle/mod.rs`, retaining the single CoreLifecycle mutation actor and existing Service transition lease.

## Defect and decision

The previous source-intent fix emitted an untyped `StartupRecoveryFinished` event once an asynchronous Startup recovery attempt finished. The actor handled it by calling `replan_retained_source_on_service_change` even if the Service lease failed, recovery timed out or the workflow returned an error. If the shared ownership flag changed between the original source and this completion event, an unsuccessful recovery might initiate a source-driven Core round solely because the worker had exited.

The event now carries `succeeded: bool` from the actual worker result. The lifecycle actor **only** allows that _completion notification_ to wake a retained source when the attempt returned `Ok`. The existing replan gate independently checks the current RunIntent, startup-owner evidence, uncertain outcome, shutdown state, active Core operation and pending `FollowService`. A failed or timed-out attempt retains the source continuation without launching a speculative Core stop or apply. A later independently verified Service readiness update or successful Startup recovery may still request a fresh S2/S3/S4 facts round.

The worker clears `startup_recovery_pending` before sending the result-tagged notification, preserving the previous protection against a source arriving before or after the recovery command's completion. No new timer, second Core writer, public API, persisted state or gitlink change is introduced.

## Regression and acceptance

- Targeted Actor integration exercises two committed Profiles generations held behind a Startup recovery gate, then sends an unsuccessful completion. It requires **zero** extra SourceCommitted rounds and no new Core submissions. A subsequent successful completion must apply the latest generation exactly once and converge.
- The same scenario with explicit `StopCore` confirms the continuation stays canceled even if a later success event arrives.
- The actual `CoreLifecycleClient::source_committed` integration continues to cover the lease-blocked entry path with latest-source coalescing.
- Full `chimera-core` and Tauri Rust tests, rustfmt, Clippy and Git diff checks are required before signed fast-forward delivery.

## Remaining work

The compatibility StartupWorkflow still owns S2 lease execution; this is a bounded completion/result bridge, **not** the full upstream independent RuntimeReconciler actor migration. The tests use injected ports rather than real installed desktop Core, Service, TUN or DNS environments. The next stage should unify the admission and source-intent state across Startup, ServiceReadiness and SourceCommitted without weakening ownership proof.
