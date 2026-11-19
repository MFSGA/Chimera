# RuntimeReconciler: recover S2 source continuation after a different verified lifecycle round

## Baseline and upstream rule

- Chimera base: `7cae712cd934c2edc8c6cf7775216e511d19d36f` on `master`.
- Pinned ref: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; no gitlink changes.
- Upstream: `ref/backend/nyanpasu-core/src/client/reconciler/{workflow.rs,startup.rs,plan.rs}` — the one reconciler retains committed-source intent across a dependency wait, and begins a fresh facts round after ownership becomes proven.
- Chimera: `backend/tauri/src/client/core_lifecycle/mod.rs` preserves the existing single CoreLifecycle actor, the S2 `FollowService` leased command, and `source_after_handoff` continuation flag.

## Gap fixed

The preceding migration retained a source continuation only until a _successful_ `FollowService`. If S2 had selected a concrete owner move but the lease was busy or handoff later failed, the source remained durable while its one-shot continuation was latched. A subsequent explicit `StartCore` or startup recovery could re-establish the owner and a verified running Core, yet the flag was not consumed because the successful round's trigger was `Start` or `Startup`, not `ServiceReadiness`. Without another source notification, the compatibility actor might never schedule its fresh committed-source planning pass.

After any successful _reconciling lifecycle operation_, the actor now consumes a retained source continuation **only if** all conditions hold:

- The Core's accepted runtime receipt proves it is `Running` for the current runtime identity (`verified_running`).
- The startup owner is proven and both local and shared owner-uncertain flags are clear.
- The lower Core outcome is not uncertain and the lifecycle is not shutting down.
- RunIntent remains `Running`; an explicit `StopCore` or shutdown clears the continuation before this point.

It then sets `source_dirty` and schedules a single coalesced `ReconcileSource` through the existing mailbox. That later S2/S3/S4 round captures the **latest committed** Application/Clash/Profiles and managed bytes; it never replays the pre-handoff candidate or creates another Core writer. A failed or unverified recovery does not consume the continuation or authorize a speculative Local start.

## Acceptance

- Existing actor integration: a concrete `Step::Owner` with a successful leased Service retirement resumes a fresh source round without a second notification.
- New actor integration: the first transition lease is explicitly refused. The original source round settles as `Waiting`; no extra Core stop/submit or foreign owner retirement happens. The other Service owner later exits; explicit `StartCore` re-establishes a verified Core and automatically causes the retained source to be re-read by a new `SourceCommitted` round. The proof includes the operation ledger and round history if the sequence stalls.
- The rest of the Tauri and `chimera-core` Rust unit suites, Rustfmt, Clippy and `git diff --check` must pass before push.

## Remaining work

This is a narrow recovery/continuation improvement. The full ref `RuntimeReconciler` actor and source/owner command loop are not completely migrated. Automatic retry on a busy lease is deliberately **not** invented; a later explicit command, new readiness event, or existing startup recovery can advance the dependency. Real installed desktop/Service/TUN E2E has not been validated in this round. No public API, config schema, UI files, gitlink, branch or worktree change is needed.
