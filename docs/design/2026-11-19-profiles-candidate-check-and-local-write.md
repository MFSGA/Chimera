# Profiles candidate checking and source-owned local writes

## Baselines and scope

- Chimera baseline: `21d889e2` (clean `master` before changes).
- Pinned reference: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01` (unchanged, clean).
- Risk: L2. This batch changes source transaction boundaries and preflight validation. No new UI, RPC command, persistence schema, host network mutation, or runtime owner is introduced.

Ref path/symbol mappings:

| Reference                                                                                                | Chimera                                                                                           |
| -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| `ref/backend/nyanpasu-state-engine/src/manager/persistent_state.rs::replace_if_version_with_local_write` | `backend/chimera-core/src/state/manager/persistent_state.rs::replace_if_version_with_local_write` |
| `ref/backend/nyanpasu-core/src/client/reconciler/check.rs::CandidateCheck`                               | `backend/tauri/src/client/reconciler/check.rs::CandidateCheck`                                    |
| `ref/backend/nyanpasu-core/src/state/mutation/ports.rs::RuntimeGate`                                     | `backend/tauri/src/state/mutation.rs::RuntimeGate` (Profiles-only bridge)                         |
| `ref/backend/nyanpasu-core/src/state/profiles/actor.rs::check_candidate, commit_with_resources`          | `backend/tauri/src/state/profiles/actor.rs::persist_candidate, commit_with_resources`             |

## Implementation

1. Add source-only local-write persistence to the existing Chimera state coordinator. The detached persistence owner retains the serial writer permit and runs local recovery before an aborted transaction is reported, including caller cancellation and document-write failures. The reference uses a simpler local-state publication engine; Chimera retains its existing coordinator to avoid a competing state owner.
2. Source-only managed-file changes (outside the active dependency closure, with no explicit runtime request) now promote their resources and publish the versioned document without a legacy runtime participant. The source content publication fence covers promotion and document publication. Successful commits notify shared Profiles consumers and publish source versions without triggering Core apply.
3. Runtime-affecting mutations assemble a frozen candidate and run an independent `CandidateCheck` using the real Core validator _before_ source publication; materialization preparation is deferred until after this check. Checker refusal/unavailability rejects the mutation, preserving the previously committed document.
4. **Compatibility boundary:** Runtime-affecting mutations still use the old application runtime participant for apply/confirmation after the independent check. This is intentional only until the upstream committed-target `RuntimeReconciler` is ported. Do not interpret passing checks as full runtime architecture alignment.

## Acceptance and regression contracts

- Unit: a local resource and document commit together, stale CAS never runs local write, local-write failure compensates, document write failure compensates, cancellation settles recovery before release.
- Actor integration: an inactive managed file writes without an operation participant; the active file keeps the current apply participant; rejected active candidate carries frozen proposed bytes, never prepares/promotes materialization, and leaves disk document/revision unchanged.
- Required follow-up: replace the runtime participant with `SourceCommitted → RuntimeReconciler Apply` only after porting stopped-by-user intent, startup ownership, retry policy, service handoff and recovery reporting. Then remove the compatibility participant and add real runtime/E2E restart validation.
- Test boundary: existing Rust core/actor tests exercise mocked materialization and runtime ports. They do not establish production Core success, UI controls, system proxy/TUN effects, or cold-start persistence.

## Rollback

Revert the two migration commits in reverse order. No user-facing schema conversion or submodule update is performed; the previous `replace_if_version_with_participant` path remains available for active mutations.

## Verification

Run `cargo fmt --manifest-path backend/Cargo.toml -p chimera-core -p chimera`, core/Tauri library tests, `cargo clippy` for touched crates, and `git diff --check`. Record actual outcomes in the task report; a skipped platform/real-runtime test is not a pass.
