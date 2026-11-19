# Ref alignment: Reconciler owns runtime input and round records

## Base and upstream reference

- Starting local Chimera master: `ea136ae05b26168195a551bb25427c69a86cfeef`.
- Fixed upstream gitlink: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`.
- Fixed `backend/chimera-runtime` gitlink: `9cf8e01acea5c5af2ec074c14d99d2b7f9192ffa`.
- Source of truth: `ref/backend/nyanpasu-core/src/client/reconciler/{mod.rs,inputs.rs,mutation.rs,status.rs,workflow.rs}`.
- The objective is **source architecture parity**, with tests only as a regression gate. This is a production wiring migration, not a spare module copy or a new runtime actor.

## What migrated in this batch

1. **`ref/client/reconciler/inputs.rs` → `backend/tauri/src/client/reconciler/inputs.rs`.** The existing Chimera `RuntimeInputs` (application, Clash, Profiles and frozen managed file contents) is now canonically defined by the Reconciler, exactly as in ref's source tree. Its target digest includes the same existing runtime-impact projections and frozen content. Chimera's current `into_parts` interface stays intact to support the existing mutation and CoreLifecycle adapters. The `FrozenProfileContent` implementation still lives in `chimera-core::client::runtime_build_port` until the Core build port is fully migrated; avoid duplicating its implementation.
2. Production references from **CoreLifecycle**, **Clash candidate checking**, and **ApplicationWorkflow's runtime trait** now resolve this canonical Reconciler type rather than having the mutation workflow own the input identity.
3. `application_workflow/inputs.rs` becomes a compatibility-only re-export, preserving old source clients and in-flight TCC tests without granting the ApplicationWorkflow a second runtime target type.
4. **`ref/client/reconciler/mutation.rs`'s `RoundRecord` boundary** is introduced in `backend/tauri/src/client/reconciler/mutation.rs`. The same single CoreLifecycle actor still publishes and owns the bounded reconciliation history via `reconciler/status.rs`; only type ownership and imports change.
5. `backend/tauri/src/client/reconciler/mod.rs` exports the ref-shaped module boundaries.

No Core operation admission, active receipt, Service ownership, run intent, source commit, DNS, TUN, IPC wire, persistent data, UI or gitlinks changed. In particular, this work does **not** spawn a second RuntimeReconciler actor or change which actor owns Core mutations.

## Review and acceptance

- Existing `client::reconciler::inputs::tests` test the target identity exclusions, runtime input changes, frozen managed file capture and staged content precedence.
- Existing `client::reconciler::status` tests retain the same bounded round history and conclusions after moving `RoundRecord`.
- Full `chimera-core` and Tauri library tests, Rustfmt, Clippy and `git diff --check` must pass before committing.
- Make separate signed commits for inputs and for mutation/status module realignment; put this design note in a separate docs commit.
- Preserve concurrent remote UI changes by rebasing onto current `origin/master`, rerunning full checks, then normal fast-forward push. Never squash or force push.

## Explicit remaining upstream alignment gap

The pinned ref's `client/reconciler/mod.rs` contains the single independent `RuntimeReconcilerActor`, `RuntimeReconcilerState` and `RuntimeReconcilerClient`, with `workflow.rs` orchestrating `preparation.rs → startup.rs → apply.rs`, its own `ports.rs`, and fully owned commit-aware `status.rs`. Chimera has a single **CoreLifecycle actor** plus compatible ApplicationWorkflow and ref decision/planner adapters, but does **not** yet have that upstream actor and workflow architecture. Following this first ownership cleanup, migrate upstream `preparation`, `startup` and `apply` into the Reconciler module with adapters, and replace — not parallel-run — the old actor once the ref command interface and existing Chimera extensions can be covered. Do not claim parity until the entire source path is wired and production startup, source commits, Service transitions and shutdown all use the ported owner.
