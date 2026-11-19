# Profiles Source Commit → single-runtime-owner reconciliation

## Fixed upstream baseline

- Chimera starting point: `3bdc8b908fb85db7a984c7fb7462130aca3dc245`.
- `ref` submodule: `c4b35e04870a9caa53f21d10af6fcfde4a6f4c01`; do not advance the gitlink in this migration.
- Scope: the Profiles source actor, runtime gate notification, and CoreLifecycle's one runtime mailbox. No new Core owner, RPC surface, config schema, or UI control is added.

## Upstream mapping

| `ref`                                                                  | Chimera adaptation                                                                                                                   |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `state/profiles/actor.rs::persist_candidate`                           | `state/profiles/actor.rs::persist_candidate`: independent check then versioned source-only commit                                    |
| `state/profiles/actor.rs::commit_with_resources`                       | `state/profiles/actor.rs::commit_with_resources`: validate/freeze, prepare, source-owned local-write + recovery                      |
| `state/mutation/ports.rs::RuntimeGate::committed`                      | `state/mutation.rs::source_committed` → `ApplicationWorkflowClient::profiles_runtime_committed`                                      |
| `client/reconciler/mod.rs::Message::SourceCommitted`                   | `core_lifecycle/mod.rs::Message::SourceCommitted`: one serialized runtime owner queues latest-source reconciliation                  |
| `client/reconciler/workflow.rs::reconcile_round` / committed snapshots | `core_lifecycle/workflow.rs::reconcile` + `ProfileContentReader`: one committed document/file capture and before/after version fence |

The full upstream actor is not copied wholesale: Chimera's existing `CoreLifecycle` still owns the one runtime mailbox and the compatibility ApplicationWorkflow. This intentionally avoids running two concurrent Core controllers while the wider Core/Service architecture is being ported.

## Semantics and failure boundaries

1. **Pre-commit**: runtime-affecting changes must pass CandidateCheck. Rejection/unavailable validation leaves the old source state untouched. Content-dependent checks use the staged bytes.
2. **Source-owned persistence**: no production Profiles write calls `replace_if_version_with_participant`. Resource promotion, durable document publication and compensation remain in the persistence owner while the shared content fence is held.
3. **Post-commit**: source publishes changed-file notifications and a monotonic source version. Only runtime-affecting operations request reconciliation. The runtime actor fetches the latest committed target; source operations return without claiming Core apply has completed.
4. **Supersession**: CoreLifecycle compares the Profiles source version around the committed target capture and Core apply. If it advances, the round reports a typed supersession and schedules another latest-source round; the old target is never reported as converged.
5. **Core refusal**: the already committed Profiles document remains durable. Runtime apply failure appears in the existing lifecycle completion status. Do not treat a source commit receipt as proof of runtime convergence.

## Regression coverage

- Ref-style candidate rejection before resource preparation, persisted document unchanged.
- Inactive managed file does not request Core apply; active file or explicit activation does.
- An older SourceCommitted notification applies the latest committed Profiles document, not the old message payload.
- A newer source committed while Core is applying the previous generation results in a second round and an explicit superseded-round error.
- Core apply failure after source commit does not roll back the document.
- Existing rustfmt, Clippy, core and Tauri library tests remain required.

These Rust tests use injected Core/Service adapters. They do **not** prove desktop UI or real Core and Service integration, macOS/Windows TUN behavior, crash recovery or cold-start E2E.

## Remaining architecture alignment

- Replace the compatibility CoreLifecycle/ApplicationWorkflow source notification implementation with the upstream `RuntimeReconciler` actor and status/conclusion model when its Core/Service ownership boundary is migrated.
- Align explicit stop/start intent, Service handoff, retry budget, reconciliation status/receipt projection, and effects/maintenance notifications with ref across Application, Clash and Profiles domains.
- Retire the legacy transaction participant fixture/API after its historical regression tests are replaced with committed-target tests. The production Profiles actor no longer uses it.
- Follow with real desktop runtime failure/recovery and end-to-end activation tests before calling the complete ref architecture migrated.

## Rollback and delivery

No schema migration or submodule change. Revert the source-commit implementation commit and the reconciler adaptation commit in reverse order if regression evidence requires rollback. Record actual tests, signatures, and remote `master` verification after delivery.
