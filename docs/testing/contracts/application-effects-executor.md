# Application effects executor unit-test contract

- Contract ID: `application-effects-executor-dispatch`
- Boundary: Rust unit tests over the production executor, effects actor,
  effects plan, `HotkeyClient`, and `SystemProxyClient`; every effect that can
  change host state uses fake ports.
- Initial state: a complete immutable `ApplicationEffectPlan`, with a fresh
  hotkey owner and a recording fake adapter.
- Operation: submit a plan once at one revision, publish one committed domain
  slice, submit one effect whose dependency is intentionally unavailable, or
  drive the system-proxy actor through reconcile, guard, PAC, shutdown, stale
  proxy repair, and failed-action restoration.
- Authoritative result: one `EffectStatus` per planned effect, the planned
  order observed at the adapter boundary, the merged desired slices, and the
  fake's recorded calls. A Profile commit must dispatch only a partial tray
  refresh.
- Regression sensitivity: omitted effect arms, wrong dispatch order, host calls
  made before an active mixed port is resolved, a controller port being used
  as the system-proxy listener, app configuration being treated as proof that a core is listening, an enabled widget reported healthy without
  a runtime, Agent reporting proxy-enable success after the core stops, an
  unknown Random/AllowFallback-strategy port being treated as an owned endpoint,
  or a PAC setting preventing proxy disable all fail an assertion.
- Simulated boundaries: no Tauri window/tray, real autostart registration, OS
  proxy, PAC service, logger thread, widget process, or global shortcut plugin
  is invoked. The system-proxy tests use fake OS proxy, autostart, and PAC ports;
  their restore assertions prove actor decisions, not actual macOS settings
  restoration. System-proxy actor tests exercise stale endpoint compare-and-set
  and rollback through fake ports. Agent action tests cover proposal/planning,
  final snapshot validation, and error mapping, but do not execute the full
  orchestration or deterministically test rollback when the core stops during
  enable. The hotkey owner is real but receives no non-empty
  registration.
  The recording effect port observes plans without applying them to the host.
- Persistence, page reload, and cold start: not applicable to these executor
  unit tests.
- Resources and cleanup: actor state and fakes are local to each test; the
  suite makes no host-level changes.

Run from the repository root:

```sh
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::executor::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::adapters::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::actor::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::system_proxy::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::actions::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::application::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::ports::tests::effective_random_mixed_port_prefers_the_confirmed_runtime_binding -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::actions::tests::system_proxy_actions_require_a_known_runtime_port -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib features::agent::actions::tests::new_action_verification_requires_desired_and_observed_state -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::application_workflow::tcc::tests::committed_profile_mutation_settles_after_runtime_apply -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests::lifecycle_reconcile_and_shutdown_publish_active_runtime_binding -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::core_lifecycle::tests::stale_proxy_lifecycle -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::tests::system_proxy_mutation_guard_serializes_competing_transactions -- --exact
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::system_proxy::tests::stale_proxy_rollback_does_not_overwrite_a_concurrent_host_change -- --exact
```

The backend CI job runs `pnpm test:backend`, which runs Cargo tests for the
workspace with all features; this includes these executor, system-proxy, and
Agent unit tests. Their fake OS ports prove owner decisions and rollback paths,
not real desktop OS-proxy, PAC, or login-item behavior. A desktop run of the
production setup can touch host settings and requires a dedicated runner when
verifying those effects.
