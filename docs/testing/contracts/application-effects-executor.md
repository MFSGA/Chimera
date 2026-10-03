# Application effects executor unit-test contract

- Contract ID: `application-effects-executor-dispatch`
- Boundary: Rust unit tests over the production executor, effects plan, and
  `HotkeyClient`; all other effect adapters are fakes.
- Initial state: a complete immutable `ApplicationEffectPlan`, with a fresh
  hotkey owner and a recording fake adapter.
- Operation: submit the plan once at one revision, or submit one effect whose
  dependency is intentionally unavailable.
- Authoritative result: one `EffectStatus` per planned effect, the planned
  order observed at the adapter boundary, and the fake's recorded calls.
- Regression sensitivity: omitted effect arms, wrong dispatch order, host calls
  made before the mixed port is resolved, an enabled widget reported healthy
  without a runtime, or a PAC setting preventing proxy disable all fail an
  assertion.
- Simulated boundaries: no Tauri window/tray, autostart registration, OS proxy,
  PAC service, logger thread, widget process, or global shortcut plugin is
  invoked. The hotkey owner is real but receives no non-empty registration.
- Persistence, page reload, and cold start: not applicable to these executor
  unit tests.
- Resources and cleanup: actor state and fakes are local to each test; the
  suite makes no host-level changes.

Run from the repository root:

```sh
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::executor::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::adapters::tests -- --test-threads=1
```

The Ubuntu CI job runs the effects unit tests with the `client::effects::`
filter. These tests prove executor dispatch only. They do not prove the legacy
sysopt adapter's behavior on a real desktop or complete typed-commit
notifications.
