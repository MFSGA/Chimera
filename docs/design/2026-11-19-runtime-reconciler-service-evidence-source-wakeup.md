# RuntimeReconciler: resume retained sources on Service evidence changes

## Baseline and upstream mapping

- Chimera `master` baseline: `c6b8f0a1e22bff2c54966ce6be669cc97419b7ca`.
- Fixed upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (gitlink unchanged).
- Upstream model: `ref/backend/nyanpasu-core/src/client/reconciler/{startup.rs,workflow.rs,plan.rs}` — a committed source remains durable while S2 ownership is unresolved; new Service evidence restarts the same full planned round.
- Chimera implementation: `backend/tauri/src/client/core_lifecycle/mod.rs` within the existing single CoreLifecycle actor.

## Recovery hole

The previous migration preserved `source_after_handoff` when the S2 owner move could not acquire a transition lease. It resumed after successful `FollowService` or another _verified reconciling command_ (for example explicit `StartCore`), but a different owner might cease running **externally**, leaving no successful CoreLifecycle operation to consume the continuation. A ServiceActor readiness update could find no further handoff due and return without re-reading the committed source.

Now a positive `ObserveService` or `ServiceReadiness` notification first calls the existing leased `follow_service` path, then — **only when no FollowService is queued and none is active** — coalesces a fresh `ReconcileSource` through the existing mailbox if there is a retained S2 continuation.

This is a request for **new facts**, never authority to mutate Core. The fresh S2 planner must still prove ownership and will return `Waiting` without stopping or applying Core when Service is owned by another instance, unreadable or otherwise unsafe. The continuation remains latched on failure. A generic Service probe failure does not trigger a fresh source attempt. The helper also honors stopped run intent, shutdown, uncertain lower outcome and both startup-owner-unproven flags.

After a successful verified `SourceCommitted` pass, the continuation is consumed **without scheduling an identical third pass**, and an obsolete `Service ownership transition is waiting for its lease` maintenance diagnostic is cleared. Other successful operations (Start/FollowService/startup recovery) still schedule one fresh committed-source pass rather than treating their own result as sufficient to consume the latest source. No additional Core writer, retry timer, persistent schema or public IPC is introduced.

## Verification and acceptance

- An injected Actor test establishes a real Startup ownership report, then commits a new Profiles target while a second owned Service Core exists.
- A deliberately rejected first transition lease produces `Waiting` without another Core submission, stop or retirement.
- The external Service owner then exits. Without explicit `Start`, a second source notification or a background polling loop, exactly one Service readiness change wakes the retained source.
- The subsequent fail-closed S2/S3/S4 round stops the outdated selected Core once, applies the latest committed target once and reaches `Converged`. The obsolete handoff-maintenance warning is cleared and no redundant third SourceCommitted round appears after the coalescing window.
- Existing Actor tests cover successful handoff continuation, explicit Start recovery, unreadable Service evidence, stop intent, instance-change races, release-owed and pending RuntimeProduct repair.
- Full `chimera-core` and Tauri Rust library tests, Rustfmt, Clippy and `git diff --check` are gates before a signed fast-forward push.

## Remaining limitations

S2 execution still delegates to the compatibility StartupWorkflow and Service transition lease rather than a standalone upstream RuntimeReconciler actor. Actual installed daemon, desktop UI and TUN E2E were not run locally by this batch. Event-driven recovery depends on a subsequent real Service evidence notification; it intentionally does **not** poll automatically after a failed lease. Keep ref and runtime gitlinks fixed and preserve unrelated remote UI commits.
