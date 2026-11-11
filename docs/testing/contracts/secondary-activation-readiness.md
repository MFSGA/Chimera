# Secondary-instance activation readiness contract

## Scope

Chimera retains the Tauri single-instance plugin because a second launch must
forward activation to the existing Main or Legacy window. Unlike the older
upstream commit, the current `master` already creates config and data roots
inside Tauri `setup`, after the singleton plugin is initialized.

`backend/tauri/src/single_instance_activation.rs` is a compatibility gate:

- Before primary setup is ready, repeated activation requests are coalesced.
- After Client, window state and deep-link listeners exist, one pending window
  activation is delivered; later requests activate immediately.
- A request racing with the ready transition is delivered exactly once.
- E2E feature builds retain their existing no-singleton test setup.
- This does not change persistence, OS proxy, core or Service state.

## Executed evidence on Windows

- `cargo test --locked --offline -p chimera --lib single_instance_activation::tests`
  with the installed `nightly-2026-11-06`: 2/2 passed.
- Full `chimera` library tests: 781 passed, 1 existing ignored, 0 failed.
- `cargo check --locked --offline -p chimera --lib --features e2e`: passed.
- Tauri checks used a temporary process-level `TAURI_CONFIG` override that
  omits missing packaging sidecars; this is not a release-build check.

## Real desktop acceptance still required

1. Cold start one instance, then launch a second before initialization completes.
   No second window or independent config writer should start; the primary
   should receive one activation after readiness.
2. Send two early activations and verify only one queued wake is delivered.
3. Launch again after readiness; the existing Main or Legacy window should focus.
4. Repeat with a deep link and verify the URI reaches the intended UI once.
5. Repeat on Windows, macOS and Linux, including startup failure and restart.

Unit tests do not verify Tauri plugin process ordering, window focus, URI
forwarding, or real OS-level singleton behaviour.
