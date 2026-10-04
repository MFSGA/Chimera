# Application effects executor unit-test contract

- Contract ID: `application-effects-executor-dispatch`
- Boundary: Rust unit tests over the production executor, effects actor,
  effects plan, and `HotkeyClient`; all operating-system effect adapters are
  fakes.
- Initial state: a complete immutable `ApplicationEffectPlan`, with a fresh
  hotkey owner and a recording fake adapter.
- Operation: submit a plan once at one revision, publish one committed domain
  slice, or submit one effect whose dependency is intentionally unavailable.
- Authoritative result: one `EffectStatus` per planned effect, the planned
  order observed at the adapter boundary, the merged desired slices, and the
  fake's recorded calls. A Profile commit must dispatch only a partial tray
  refresh.
- Regression sensitivity: omitted effect arms, wrong dispatch order, host calls
  made before the mixed port is resolved, an enabled widget reported healthy
  without a runtime, or a PAC setting preventing proxy disable all fail an
  assertion.
- Simulated boundaries: no Tauri window/tray, autostart registration, OS proxy,
  PAC service, logger thread, widget process, or global shortcut plugin is
  invoked. The hotkey owner is real but receives no non-empty registration.
  The actor's recording effect port observes plans without applying them to the
  host.
- Persistence, page reload, and cold start: not applicable to these executor
  unit tests.
- Resources and cleanup: actor state and fakes are local to each test; the
  suite makes no host-level changes.

Run from the repository root:

```sh
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::executor::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::adapters::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::effects::actor::tests -- --test-threads=1
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::application_workflow::tcc::tests::committed_profile_mutation_settles_after_runtime_apply -- --test-threads=1
```

The Ubuntu CI job runs the effects unit tests with the `client::effects::`
filter and the Profile TCC unit tests. These tests prove executor dispatch and
the Profile commit notification only. They do not prove the legacy sysopt
adapter's behavior on a real desktop or complete typed-commit notifications.
The actor tests cover Profile notifications and slice merging; they do not imply
application, Clash, or runtime-bound producers are wired.
