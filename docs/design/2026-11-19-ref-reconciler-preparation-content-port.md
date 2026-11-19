# Ref alignment: Reconciler preparation and content-capture port

## Baseline and objective

- Starting Chimera `master`: `0f838d7553f18039791548419e60a1518ed09b0d`.
- Pinned upstream `ref`: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`.
- Pinned `backend/chimera-runtime`: `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Upstream source: `ref/backend/nyanpasu-core/src/client/reconciler/{preparation.rs,ports.rs,inputs.rs,check.rs}`.
- The goal is **incremental real production source alignment**, not an independent Runtime test project or an unconnected copy of upstream types.

## Preparation ownership migrated

Upstream `RuntimePreparation` owns three concerns: (a) capture candidates and committed Profile documents plus frozen managed-file content under a publication fence, (b) resolve port bindings and build an immutable, versioned runtime candidate, and (c) make the immutable candidate available to Core check/apply. Chimera currently has the equivalent concerns split between CoreLifecycle, the legacy runtime builder and existing Core/Service adapters.

This batch moves the **production input capture stage (a)** to `backend/tauri/src/client/reconciler/preparation.rs`, naming it `RuntimePreparation` as in upstream. Its read-only `ProfileContentReader` fence is retained, including committed document/content co-sampling and candidate-only staged bytes. `CoreLifecycleWorkflow::capture_profile_inputs_for` and `capture_committed_profile_inputs_for` now delegate to it rather than owning independent capture implementations. `CandidateCheck::check` likewise uses the shared Reconciler preparation entry for candidate frozen inputs.

The source dependency is injected through `reconciler/ports.rs::RuntimeContentPort`, a bounded adaptation of upstream `RuntimeBuildPort::capture_content`. Its production implementation delegates to Chimera's existing `ProfileContentSource` and `chimera-core::client::runtime_build_port::FrozenProfileContent::capture_content`, preserving the active profile closure, missing-file errors and staged override rules. No extra source actor, filesystem writer or second Core executor is introduced.

## Differences deliberately retained

- The **full upstream `RuntimeBuildPort`** also includes `core_spec`, `build` and `publish`. Those remain in the current CoreLifecycle/runtime builder until the prepared runtime can flow through one validated, serialized Core application with a confirmed receipt. The new port is explicitly named `RuntimeContentPort` to avoid incorrectly claiming complete `RuntimeBuildPort` parity.
- Unlike ref's `RuntimePreparation`, Chimera's compatibility wrapper still accepts an optional committed Profiles snapshot for legacy ports where the fence reader is unavailable. Production with a reader uses the fenced committed path; neither path changes selected Core/Service host, revision allocator or application intent.
- CoreLifecycle still owns runtime mutation and accepted-receipt verification. This batch does not spawn an upstream `RuntimeReconcilerActor` alongside it.
- The current `RuntimeInputs::target_key()` and `into_parts()` interfaces are unchanged; no altered hashing, port allocation, YAML bytes or profile transform behavior is intended.

## Verification

- New Reconciler preparation tests: candidate staged content cannot write the committed source; the committed Profiles/content fence waits for file promotion; missing committed snapshot is rejected.
- Existing `reconciler/check`, `reconciler/inputs`, CoreLifecycle target/receipt, source commit and runtime-product tests must continue to pass.
- Required before push: full `chimera-core` and Tauri library Rust suites, Rustfmt, Clippy and `git diff --check`. Re-run after rebasing onto newest `origin/master`; keep newer remote UI history.
- Real desktop Core/TUN/DNS behavior is not the target of this migration batch; those tests remain regression gates where available, not a reason to replace upstream alignment work.

## Next upstream step

Port the remaining actual `RuntimeBuildPort` methods, candidate port resolution and snapshot/intent creation into `reconciler/preparation.rs`, then `startup.rs` and `apply.rs`; finally replace the compatibility CoreLifecycle mailbox with the upstream single `RuntimeReconcilerActor` at its production composition root. Keep only **one** mutating Core owner throughout transition.
