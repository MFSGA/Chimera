# C: ApplicationWorkflow / CoreLifecycle ref-first migration

Status: **partial implementation, not yet integrated as upstream's full workflow actor**.

## Baseline and boundaries

- Chimera checkout at start: `master@1f06b55ceb4b68048305ca85465cd97c991d8e71`, clean.
- Pinned read-only ref: `59724e85c34abb6995c22d101c876cc06dc16576`, clean.
- Runtime gitlink: `54294d1fdda6f4f574a27e90374a79dd6558dcf2`, unchanged.
- The reported five dirty Startup/Lifecycle files are **not present in this Windows checkout**. No files were imported from another host.
- C-owned implementation scope: `backend/tauri/src/client/application_workflow/**` and `backend/tauri/src/client/core_lifecycle/**`. This document is an integration handoff, not a change to the normative alignment guide.
- Explicit non-goals: A's CoreActor internals, B's ServiceActor internals, G's `setup.rs`/`lib.rs` composition, shared actor_v2 exports, frontend, ref and runtime gitlinks, host networking, user-data migration, releases and history rewrites.
- Risk: L2 for runtime-operation isolation and failure/recovery admission. A/B and the integration owner must review ownership and the eventual public APIs.

## Ref source → Chimera implementation mapping

| Reference source (`ref/backend/nyanpasu-core/src/client/`)                                         | C working area                                                                  | Current finding                                                                                                                                                                                                                                                                             |
| -------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `application_workflow/mod.rs`                                                                      | `backend/tauri/src/client/application_workflow/mod.rs`; `core_lifecycle/mod.rs` | ref owns **one** actor executing source decisions, runtime actions, service follow-up and shutdown. Chimera currently uses the Tauri lifecycle actor and mutation connection with a separate client wrapper. Full replacement pending shared A/B contracts and D/E client/state dependency. |
| `application_workflow/attempt.rs`                                                                  | `backend/tauri/src/client/application_workflow/attempt.rs`                      | Implemented the ref positive-clearance check against unsettled lower owner, plus ref lifecycle initial stage and diagnostics.                                                                                                                                                               |
| `application_workflow/policy.rs`                                                                   | `backend/tauri/src/client/application_workflow/policy.rs`                       | Copied the upstream policy test cases with only an argument adapter for Chimera's extra peripheral-effects input. Retained `SaveOnly` and `SaveThenNotify` until their effect owners move.                                                                                                  |
| `application_workflow/{tcc,mutation,impact,inputs,participant,profiles,ports,startup,workflow}.rs` | Corresponding files under `backend/tauri/src/client/application_workflow/`      | Existing implementation includes TCC, staged candidate application, deferred retry, startup report and recovery. **Not yet a full upstream source copy**.                                                                                                                                   |
| `application_workflow/{preparation,adapters}.rs`                                                   | No corresponding standalone C modules                                           | Upstream preparation consumes read-only `StateSnapshot`, `SessionPortResolver`, `RuntimeBuildPort`, `RuntimeIntentBuilder`. Integrating these now would require D/E's typed state and runtime port contracts. Do not duplicate builder or mutable config in Tauri for a temporary copy.     |
| `core_lifecycle/{mod,workflow,ports,apply}.rs`                                                     | `backend/tauri/src/client/core_lifecycle/{mod,workflow,ports,adapters}.rs`      | Tauri still owns its actor and compatibility `CoreLifecyclePort`. Upstream `apply.rs` uses an instance-fenced source API for post-apply connection interruption; needs A's final API lease/Facade contract before moving over wholesale.                                                    |

## Completed C-owned code

1. `application_workflow/attempt.rs`: `LiveAttempt::lifecycle` now enters the `TryingCritical` stage as ref does, instead of looking like a preparation that never began.
2. `application_workflow/attempt.rs` and `workflow.rs`: `conclude_attempt` consults `CoreLifecycleWorkflow::outcome_uncertain()` even when its caller supplied no error. An unsettled lower owner retains the write-ahead attempt and explicit recovery requirement. The original failure reason takes precedence over the lower-owner generic reason; the code logs the operation ID and stage.
3. `application_workflow/policy.rs`: copied the ref policy tests for explicit switches, user-stopped intent, non-deferrable host/core swaps, all required deferral facts, unknown outcome and unconfirmed baseline; added Chimera's source-only policy case.
4. `core_lifecycle/mod.rs` (test-only): added a mock-bound `CoreLifecycleWorkflow` integration test proving that an unsettled lower owner blocks clearance of the actual `ApplicationWorkflow` attempt, and that confirmed lower settlement enables clearance.

These changes preserve the current production Client API and do **not** modify the Core/Service Actor APIs.

## Contracts that the integration owner must settle before the full C swap

1. **A / CoreActor:** expose a single owner with ref `CoreClient` projection, revocable API lease, honest `ChangeHost`/handoff result, pending action and explicit recovery. Do not have C invent a second owner or infer successful completion from a timeout.
2. **B / ServiceActor:** provide ref `ServiceClient` status/readiness generation, endpoint supply and bounded recovery. Host switch intent belongs to the application workflow, process control to ServiceActor.
3. **D / Client Core:** choose the canonical home `backend/chimera-core/src/client/application_workflow/` for the final ref copy, and let Tauri retain only host adapters. C's current Tauri files are temporary migration inputs, **not** a permanent competing business layer.
4. **E / Runtime & state:** provide `StateSnapshot` for Application/Clash/Profiles, `RuntimePreparationPort`, `RuntimeBuildPort`, `RuntimeValidatorPort`, `SessionPortResolver`, `RuntimeSnapshotStore`, and typed `RuntimeError`. A candidate must not mutate confirmed state until the source transaction commits.
5. **G / setup:** construct and own exactly one CoreActor, one ServiceActor and one ApplicationWorkflowActor; wire Main/Legacy/Agent to the same client; transfer partial StartupOwners on failure. Only G changes `setup.rs`, `lib.rs` or shared composition.
6. **Integration ordering:** land A+B API contracts, E's preparation/state ports, C's complete workflow actor, D's composed Client, then G's setup and downstream H/I. Rebase each owner branch onto the exact integrated SHA and run new-SHA tests. These are dependency gates, not additional parallel editing invitations.

## Completion gate for remaining C work

- Copy/refactor `application_workflow/{mod,workflow,tcc,preparation,adapters}.rs` and `core_lifecycle/{mod,workflow,apply,ports}.rs` as **complete actor execution chains**, with only approved branding and product extensions.
- Confirm one admission/mailbox owner for Try → decision → Confirm/Cancel; startup runs once; unresolved lower action refuses later writes; unknown results never auto-retry; same source bytes are checked and applied.
- Cover Local ↔ Service host switches, missing/retired API lease, aborted source with failed restore, failed startup, deferred target, shutdown, and concurrent commands in tests.
- Prove actual production wiring from the composed Client rather than an idle copied module; delete replaced Tauri business implementation only after Main/Legacy/Agent consumers are migrated and verified.
- Keep E2E and real host-network results **separate** from unit/mocked integration results.

## Actually executed checks in this checkout

- `cargo test -p chimera --lib client::application_workflow::attempt::tests --locked --offline --quiet`: **6 passed**.
- `cargo test -p chimera --lib client::application_workflow::policy::tests --locked --offline --quiet`: **11 passed**.
- `cargo test -p chimera --lib client::application_workflow --locked --offline --quiet`: **52 passed**.
- `cargo test -p chimera --lib client::core_lifecycle::tests --locked --offline --quiet`: **90 passed** after the integration regression was added.
- `cargo test -p chimera --lib workflow_attempt_cannot_clear_while_the_lower_core_action_is_unsettled --locked --offline --quiet`: **1 passed**.
- `cargo fmt --manifest-path Cargo.toml -p chimera -- --check` (from `backend/`): passed.
- `cargo check -p chimera --locked --offline --quiet`: passed, with existing Rust warnings.
- `git diff --check`: passed before this document was written.
- No desktop E2E, Windows Service host mutation, TUN, startup cold-run, cross-platform CI or remote push was performed. Existing Rust warnings remain, unrelated to proof of this slice.

Rollback: revert only the C-owned attempt/workflow/policy edits and this document. Do not reset `ref`, runtime, another agent's branch or host state.
