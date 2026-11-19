# RuntimeReconciler: first production S3/S4 `plan(Facts)` bridge

## Fixed baselines and ownership

- Starting Chimera `master`: `deaca0683d6e54467cb8a5b34d204dd19ab905b9`.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`, not advanced.
- Upstream rules copied in `backend/chimera-core/src/client/reconciler/plan.rs` already matched `ref/backend/nyanpasu-core/src/client/reconciler/plan.rs::plan(Facts)`, but were not called by the production committed-source path.
- New adapter `backend/tauri/src/client/reconciler/decision.rs`; single Core executor stays in `backend/tauri/src/client/core_lifecycle/workflow.rs`. Neither module creates a new actor or writes source state.

## Production behavior migrated

In a `SourceCommitted` round or explicit `Retry`, after the exact committed Application/Clash/Profiles contents are captured and before Core submission, the workflow samples:

1. selected execution host and stable Core status (`state_changed_at != 0`);
2. the actual active Core-accepted runtime receipt;
3. the existing target/host-scoped previous attempt; and
4. independent verification that this receipt represents the exact committed target and **the same** live Core instance.

Only when the **desired host equals the selected stable host** does Chimera construct the ref `Facts` with established ownership. Missing live receipt for a Running Core, unreadable/unstable Core state, or a host handoff are deliberately left to the existing lifecycle owner; the adapter never invents an ownership proof.

The ref planner now drives these actual S3/S4 actions:

- `Idle(Converged)`: an ordinary notification for an already verified current target is a no-op. The source revision is checked again before returning, preventing a stale round from claiming the latest source.
- `Stop { intent: false }`: a Running Core with an authoritative receipt that does **not** prove the latest committed target is stopped before the existing submit path is allowed to apply the new target. Stop failure aborts the new apply.
- `FallBackToLocal`: the planner can select the existing instance-bound Service fallback after a refusal and an observed Stopped Service Core. The compatibility wrapper must not execute the fallback twice.
- `Apply` and `NotInEffect`: existing submission and refusal-policy adapters remain in place. Explicit Retry is forced by ref policy; an ordinary source notification is not.

`StartCore`, shutdown, startup reestablishment, Service-owner transition lease, stopped intent and shutdown decisions remain in the existing single actor until the rest of `plan(Facts)` is migrated coherently. These are **not** claimed as a complete independent RuntimeReconciler.

## Tests and safety

- Four planner adapter tests cover no-op for proven matching target, forced Retry, mismatched active receipt requiring Stop, owner/unreadable proof refusal and Service-target fallback precedence.
- An Actor test sends real `SourceCommitted` notifications through the one lifecycle mailbox. It proves one submit for a duplicate durable revision; after a different committed Profiles target, exactly one Core stop precedes one new apply; the final status is Converged.
- Existing 1,000+ Tauri library tests cover stop intent, Service and Local handoff, source version races, failure/retry, pending product publication and release-owed maintenance. Full `chimera-core`, Tauri, rustfmt, Clippy and diff checks are release gates.
- All test ports are mocks. Real installed macOS, Linux and Windows Core/Service/TUN E2E remains unverified.

## Remaining upstream alignment

The full `plan(Facts)` loop must still take over S2 host selection, independent recovery gating, explicitly stopped runtime handling, owner transitions, source selection and postapply maintenance rather than delegating to multiple compatibility conditions. The bridge is intentionally constrained to a **proven, established, stable host**; migrating the other cells without the Core-ownership proof would be unsafe. `ApplicationWorkflow` and `CoreLifecycle` still share compatibility coordination, although Core itself has a single mutating owner. A subsequent focused batch should integrate S2 owner-planning with the current host-lease API.

No public IPC, schema, UI, gitlink or new worktree is changed. Signed commits and a normal fast-forward push are required; revert the scoped code commit first if necessary.
