# Ref CoreFacade production API entry — staged integration

## Reference and ownership

- Baseline: `ref` gitlink `59724e85c34abb6995c22d101c876cc06dc16576`.
- A/B: canonical shared `chimera_core::actor_v2::{CoreClient, service_actor::ServiceClient, facade::CoreFacade}` already on `master`.
- C: `7b579b8e` routes readiness changes through the application workflow; this phase does not rewrite C's host transitions.
- This integration uses **only the read-only methods** of the copied ref `CoreFacade`: `core_status`, `refresh_status`, `service_status`, and `api_client_if_running`. No independent host, retry or pending-action owner is introduced in production for these reads; the actor handles are the same ones already given to the client by `setup()`.

## Production path

`setup()` creates the typed CoreActor and ServiceActor, then constructs `ChimeraClient` from those exact handles. `ChimeraClient::with_parts_and_typed_config` creates its `ref_facade` once if both handles are provided. The production path is therefore:

```
Tauri IPC / UI / Agent → ChimeraClient
                         → ref CoreFacade::api_client_if_running()
                         → CoreClient::refresh_status()
                         → CoreClient::api_client() [instance-bound API lease]
```

`routed_core_status`, `refreshed_routed_core_status`, and `service_actor_status` use the same read-only facade. Injected unit-test clients that have only a legacy CoreLifecyclePort or a single actor retain their existing fallback. This is a temporary adapter arrangement, **not** a replacement for production host-reconcile ownership.

## Safety boundaries

- A host that authoritatively reports `Stopped` cannot receive a Clash API capability. The absence of a reliable stopped verdict is _not_ evidence that a core is stopped; the copied ref facade then attempts a properly instance-bound capability check.
- The read-only adapter does not install/start a daemon, reconcile, switch hosts or clear a pending action. All mutations remain behind the existing serialized lifecycle command path during this stage.
- Once the ref `CoreFacade` becomes the mutable lifecycle owner, **reuse that owner** in ApplicationWorkflow rather than constructing a second mutable CoreFacade. Its pending-action evidence cannot be split across independent instances.
- Chimera-only transparent proxy and native Service restart remain unchanged.

## Verification

- Unit test `client::tests::production_ref_facade_fences_stopped_core_api_and_projects_both_actors` uses fake actors to prove the stopped-host refusal, independent Core/Service status, and no privileged service command.
- `cargo test --manifest-path backend/Cargo.toml -p chimera --lib -- --test-threads=4` with the repository's pnpm toolchain is the regression gate.
- Full live Service/Local switching, user stop intent, real TUN, and desktop E2E are **not** established by the read-only facade entry.

## Next required integration

Move `CoreFacade` **as one stateful owner** into the application workflow actor, migrate reconcile/stop/service mutation and recovery call chains from the old Tauri Facade, prove `PendingAction` semantics and host-handoff ordering, then remove legacy compatibility ports only after their live consumers are gone. Preserve Legacy UI and Agent contract tests throughout.
