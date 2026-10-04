# macOS TUN authorization

## Current behavior

- Reference commit: `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`; `ref/` was clean.
- Reference mapping: `ref/backend/tauri/src/core/manager.rs::grant_permission` →
  `backend/tauri/src/core/manager.rs::grant_permission`; reference
  `frontend/nyanpasu/src/hooks/use-proxy-settings.ts::useTunMode` →
  `frontend/chimera/src/features/system-proxy/use-proxy-settings.ts::useTunModeAction`.
- The reference toggle writes `enable_tun_mode` and its runtime executor emits
  the TUN config. Its macOS `grant_permission` function has no call site in the
  pinned reference tree. That leaves local-core authorization uncovered by the
  current reference call path; this is a recorded reference defect, not a
  behavior Chimera should copy.
- Chimera's main settings hook writes through `useSetting` →
  `patch_verge_config` → `ChimeraClient::patch_verge` →
  `patch_legacy_uncoordinated`. The legacy UI has no separate TUN toggle;
  hotkeys and agent TUN actions also call `patch_verge`, so the same preflight
  protects these existing entry points.
- On macOS, enabling TUN for a local core requests administrator authorization
  before any typed config or legacy mirror is committed. A connected Service
  Mode host handles the core operation itself and skips local-core
  authorization. Disabling TUN and unrelated patches do not prompt.
- The manager resolves and canonicalizes the selected core path, requests
  `root:admin` ownership and the set-user-ID bit through `osascript`, then
  verifies those permissions. Paths are shell-quoted; a dismissed or failed
  authorization returns an error before the configuration transaction starts.
- Difference category: reference defect correction. Re-evaluate this exception
  if ref connects its existing permission helper to the TUN-enable transaction.

## Verification contract

- Contract ID: `macos-tun-authorization-preflight`.
- Boundary: Rust unit tests for the authorization decision and command quoting;
  real administrator approval, route changes, and TUN traffic require a
  manual macOS system run.
- Initial state: local core is not owned by `root:admin` with setuid; TUN is
  disabled. The tested core/config must be a dedicated development instance.
- Operation: enable TUN through the settings UI (or another shared `patch_verge`
  caller).
- Success result: macOS authorization runs before commit; after approval the
  selected core has the required permissions, TUN config is committed, and the
  core reports TUN enabled.
- Failure result: dismiss or reject authorization; the patch returns an error
  and TUN remains disabled in typed state, the legacy mirror, and persisted
  config. A unit test cannot establish this host-level result.
- Cleanup: disable TUN and verify the route/config state is restored. Do not
  remove another application's routes or alter its core permissions.
- Not verified by unit tests: the actual password dialog, setuid execution on
  the packaged app, route installation/removal, and real network traffic.

The available local unit-test command is:

```sh
cargo test --manifest-path backend/tauri/Cargo.toml macos_tun_permission --lib
cargo test --manifest-path backend/tauri/Cargo.toml macos_tun_authorization --lib
```

## Regression history

- `943a4902 fix(macos): grant core permissions for TUN` added a checked
  administrator authorization helper.
- `5cd37b35 fix(macos): authorize core before enabling TUN` moved the prompt to
  the pre-commit `patch_verge` path and ran it outside the async worker.
- `ab1d0340 fix(macos): simplify TUN DNS flow` removed the TUN permission helper
  and preflight to match ref, whose corresponding helper is currently unused.
  This reintroduced the route-permission failure observed in the dev log.

The host log showed `failed to clean up routes: delete default IPv4 route
failed`; the development core was owned by the current user and had no setuid
bit. Those observations support the permission diagnosis, but only a manual
macOS TUN run can verify the complete fix.
