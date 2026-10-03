# Hotkey binding unit-test contract

These contracts apply to `backend/tauri/src/client/hotkey/tests.rs` and the
frontend parser test `frontend/chimera/src/utils/parse-hotkey.test.ts`.

## Persisted accelerator parsing

- Contract ID: `hotkey-bindings-parse`
- Boundary: Rust unit test and pure frontend parser test.
- Initial state: a fresh list of persisted `action,accelerator` strings; the
  platform parser test uses the locked `global-hotkey` parser through
  `PlatformAcceleratorValidator`.
- Operation: parse the stored strings, or normalize one keyboard event's
  `key` and `code` values.
- Authoritative result: the resulting `HotkeyBindings` map, typed parse error,
  or exact key token returned by `parseHotkey`.
- Regression sensitivity: malformed entries, unknown action names, empty key
  segments, missing modifiers, rejected platform names, unrecognized `CMD`, a
  `PLUS` token that is not canonicalized, or keypad plus being recorded as the
  main keyboard plus will fail the expected assertions.
- Simulated boundaries: no UI, IPC, configuration file, or OS registration is
  exercised. The actor parser tests use an identity validator; the dedicated
  canonicalization tests use the actual locked parser.
- Persistence, page reload, and cold start: not applicable to these pure
  parsing contracts.

## Binding diffs and actor reconciliation

- Contract ID: `hotkey-bindings-reconcile`
- Boundary: Rust unit tests over the production `HotkeyClient` and actor with a
  recording fake `ShortcutRegistrar` and fake action sink.
- Initial state: a newly spawned owner with no confirmed registrations, or a
  known set installed by a prior reconcile in the same test.
- Operation: submit one desired binding set with a specified revision; the
  partial-failure case changes the fake registrar's refusal and submits the
  next revision once.
- Authoritative result: the returned `EffectStatus`, owner status snapshot, and
  independently recorded registrar call sequence / registered map.
- Regression sensitivity: the tests fail if registration precedes releases,
  invalid desired input tears down a valid old binding, a partial success is
  forgotten or all keys are needlessly retried, a stale revision changes OS
  state, shutdown accepts a late reconcile, or a pressed action reaches the
  wrong sink.
- Wait budget: actor RPCs use the production bounded call path; tests do not
  sleep or poll for completion.
- Simulated boundaries: the fake registrar does not call the Tauri plugin and
  cannot prove macOS registration, permission behavior, shortcut conflicts,
  keyboard-layout behavior, or host cleanup.
- Resources and cleanup: all registered state belongs to the fake. Each test
  runs with its own actor/runtime state and has no host-level side effects.

## Execution and CI

Run the focused contracts with:

```sh
cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::hotkey::tests
pnpm test:hotkeys
```

The same commands are registered in the Ubuntu job of `.github/workflows/ci.yaml`.
They are unit tests only; a dedicated macOS desktop run is still required to
claim real global-shortcut registration or callback behavior.
