# Runtime field ownership contract

- Contract ID: `runtime-field-ownership`
- Boundary: the `chimera-config` config model and pure Runtime Executor. The
  Tauri builder supplies the persistent guard and TUN settings; no live Core or
  host networking is invoked.
- Covered fields in this slice: top-level `unified-delay` and
  `tcp-concurrent` in `ClashGuardOverrides`, plus nested `tun.auto-route` in
  `TunOverrides` for the Standard and ClashRs TUN flavors. The Chimera Client
  TUN flavor deliberately retains its product-specific `route-all` contract.
- Ownership meanings:
  - `Managed(value)` is a forced application value.
  - `Unmanaged` leaves the Profile/transform value in place.
  - `Forced`, `Removed`, `Default`, `Inherited`, and `Table` describe
    post-processing at the corresponding config key.
- Runtime order: build Profile and transform output; retain the pre-filter
  config; apply the first field filter; restore inherited guard and TUN keys
  from the pre-filter config; apply managed guard and TUN settings; then
  continue with resolved port bindings and the existing built-in/finalizing
  stages. TUN auto-route ownership is applied before builtin transforms, so
  transforms see the restored or managed value.
- Authoritative results:
  - A managed guard value overrides a conflicting Profile value.
  - An unmanaged guard value preserves the Profile/transform value even when
    the first field filter would otherwise drop it.
  - An unmanaged key absent from Profile output is not synthesized.
  - Nested ownership changes only declared keys and preserves unrelated keys.
  - With TUN enabled, managed `auto-route` true/false overwrites a conflicting
    Profile value; unmanaged `auto-route` preserves the Profile/transform
    value even when the first field filter drops the entire `tun` table.
  - An unmanaged `auto-route` absent from Profile output is not synthesized.
  - `tun.enable` is forced true/false in the ownership pass. The disabled
    table uses `create: false`, so a missing `tun` table is not synthesized.
  - With TUN disabled, the ownership setting does not force or synthesize
    `auto-route`; an existing profile value remains intact.
  - The Chimera Client TUN flavor ignores `TunOverrides.auto_route` and retains
    its legacy `route-all`, `dns-hijack`, and `so-mark` behavior.
  - The JSON Patch returned by each ownership pass replays to the exact result,
    including object key order.
- A sparse nested `ClashConfigPatch.overrides` updates only the addressed guard
  field, preserves other guard values, and is classified as a runtime-relevant
  request so the runtime reconcile path is not skipped.
- Compatibility: older `ClashConfig` values without a `tun` section receive
  the default `Managed(true)` setting. Legacy bare boolean `tun.auto-route`
  values deserialize as `Managed(value)`; new writes use the tagged
  `{ kind: managed, value: ... }` / `{ kind: unmanaged }` representation.
  Downgrade compatibility with older binaries that expect bare booleans is not
  claimed.
- The Clash settings page contains a ManagedGuardFieldSelector for
  `unified-delay` and `tcp-concurrent`, wired to the persistent typed
  `ClashConfig.overrides` setting through Chimera's local settings hook. All
  five locale message sets were regenerated and the TypeScript contract passed.
  No UI control for `tun.auto-route` is exposed in this slice.
- Not covered yet: a separate Runtime plan module, separate snapshot nodes
  for inherited/settings/default passes, port removal semantics, live Core
  acceptance, TUN device creation, host routes, DNS/system-proxy changes, and
  Windows/macOS/Linux desktop behavior. Do not describe this slice as complete
  Runtime ownership parity.
- Regression sensitivity: serializing a tagged object into generated Core
  YAML, failing to restore an unmanaged field after filtering, synthesizing an
  absent inherited key, overwriting a Profile value marked unmanaged, changing
  unrelated nested table entries, forcing auto-route while TUN is disabled, or
  applying the setting to the Chimera Client legacy flavor must fail a test.

Run from the repository root:

```sh
cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config --lib
cargo test --locked --manifest-path backend/Cargo.toml -p chimera-config --no-default-features --features nesting --lib
cargo clippy --locked --manifest-path backend/Cargo.toml -p chimera-config --all-targets --all-features
nix develop --command bash -c 'cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib enhance::runtime_builder::tests'
nix develop --command bash -c 'cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib client::application_workflow::impact::tests'
nix develop --command bash -c 'cargo test --locked --manifest-path backend/Cargo.toml -p chimera --lib specta_export::tests::typescript_bindings_are_fresh'
nix develop --command bash -c 'pnpm typecheck'
```

The config crate tests exercise the model and executor boundary. The Tauri
RuntimeBuilder test and generated-binding freshness test exercise the
application integration and TypeScript contract. The UI typecheck verifies the
existing guard selector's typed wiring, but none of these tests starts a real
Core, creates a TUN device, changes host routes/DNS/system-proxy settings, or
proves behavior in a running desktop session.

## Runtime Plan extraction (2026-11-11)

`runtime/executor/plan.rs` is the pure ownership declaration shared by
`apply_guard_with_source` and `apply_tun_overrides_with_source`. Unlike the
reference, absent `port`, `socks-port`, `redir-port` and `tproxy-port` overrides
do **not** remove Profile-provided listeners. An explicitly resolved port is
forced, and Linux `routing-mark` is retained. Chimera Client's legacy TUN
flavor does not inherit the Mihomo-family `auto-route` ownership. These
compatibility differences are covered by focused unit tests.
