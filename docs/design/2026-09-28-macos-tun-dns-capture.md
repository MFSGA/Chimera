# macOS TUN DNS capture iteration

## Goal and reference

Enable the system DNS override only when Chimera is running a full-route TUN with DNS interception on UDP/TCP 53 and fake-IP mode. Use a non-routable sentinel as the system resolver so a query that bypasses the TUN fails closed rather than reaching a public resolver. DIRECT-specific DNS selection is deferred.

Reference checkout: `ref/` at `5331747c06a5f42eeabb3e225a1e77a83f480549`; its worktree was clean. The reference maps `ref/backend/nyanpasu-runtime/crates/nyanpasu-core-manager/src/dns.rs` to Chimera's two `chimera-core-manager/src/dns.rs` copies, and `ref/backend/tauri/src/core/actor_v2/local_host.rs` to `backend/tauri/src/core/actor_v2/local_host.rs`.

## Current slice

- `MacosDnsController::desired` now requires `dns.enable`, `dns.enhanced-mode: fake-ip`, `tun.enable`, route-all, and catch-all UDP/TCP 53 interception. It selects `192.0.2.1`, a documentation-only address with no public DNS endpoint.
- The local Tauri host and Chimera service manager inject the controller. The existing manager lifecycle persists ownership before applying the dynamic-store change, restores it before stopping the core, and reconciles an orphan record on startup.
- The manager now applies and reads back the DNS override before launching a runtime whose effective config requests this interception. If activation fails or times out, it rejects that launch and attempts to restore the resolver. On macOS, it also rejects a protected plan if its host DNS controller was not registered. Regression tests in both manager copies cover both failure cases.
- `stop`, `shutdown`, and quarantine recovery now stop before tearing down/finalizing when an owned DNS override cannot be restored and read back. The durable ownership record remains for a retry. A successful platform restore followed by a record-clear failure remains a warning, because the resolver itself was already restored.
- If the control executor shutdown fails or times out, the service now waits for that transaction to exit and retries manager shutdown with capped exponential backoff. It does not tear down the service while the manager still cannot stop the core; the durable DNS ownership record remains available to each retry. A persistent platform failure can therefore keep service shutdown pending until DNS restore and core stop succeed.
- The service test target previously did not compile because its tests used undeclared `tempfile` and referenced the removed `ClashCoreType::Meow`; the test dependency is declared and that stale fixture now exercises the supported `ChimeraClient` mapping.
- Public manager entry points (`start`, `apply_config`, `switch`, `restart`, and `reconcile`) now run the DNS converge tail and return an explicit apply error when host DNS apply, restore, read-back, or ownership persistence is uncertain. The runtime transition may already have completed; callers must read current core status before deciding whether to retry the same revision.
- When a target plan has no DNS override intent but an override is still owned, start/replacement paths and in-place config updates restore and read back the previous resolver before launching or committing that target. Graceful switches do so after the candidate is healthy and before retiring the current plan. Restore failure retains the record and blocks that unprotected target transition.
- The controller writes a dedicated `State:/Network/Service/chimera-dns/DNS` dynamic-store key. It does not write a physical service's persistent DNS settings with `networksetup`.
- Added a standalone Swift package for the `NEDNSProxyProvider` flow relay. It accepts only a `127.0.0.1` bridge and a valid local port, relays intercepted UDP/TCP DNS flows to that endpoint, and closes stalled flows after bounded timeouts. The Xcode target builds an unsigned System Extension; the Rust listener exists, but provider activation is not wired up.
- The Chimera Client `DnsRunner` now opens an additional managed UDP/TCP listener on `127.0.0.1:1053` on macOS only when TUN DNS hijacking and the DNS service are both enabled. It coexists with configured `dns.listen`; an existing IPv4 loopback or wildcard listener on the bridge port is reused for its transport, while an occupied bridge port makes core readiness fail. This creates the provider's local bridge endpoint, but no system provider is activated yet.
- Chimera Client now supports an optional `dns.fake-ip-range6` pool, following the local Mihomo reference's dual-pool configuration. A and AAAA mappings for one hostname use separate address-family entries; when no IPv6 pool is configured, fake-eligible AAAA and other non-A queries are answered locally to avoid forwarding their plaintext QNAME. When both DNS IPv6 and an IPv6 pool are enabled, the config converter requires TUN IPv6, rejects overlap with the TUN IPv6 gateway subnet, and adds the pool route when route-all is off. Targeted pool, DNS-handler, config, and route tests pass; actual utun capture and physical-interface leak checks remain unverified.
- Added an Xcode `system-extension` target, provider Info.plist, extension entitlements, system-extension entry point, and a repeatable build script. Xcode 27's macOS 27 SDK requires deployment target 12.0 or newer, so the extension target is 12.0. The target builds an arm64+x86_64 unsigned bundle; its `NEMachServiceName` uses the Chimera app-group prefix, which needs the same registered app group and Team ID in signed builds. `CHIMERA_DEVELOPMENT_TEAM` can supply the Xcode team setting. The host app still needs matching app-group, Network Extension, and system-extension install entitlements plus a provisioning profile; the extension remains unsigned and inactive, and another active DNS proxy still needs safe conflict handling.
- Tauri's macOS app build now builds the extension before Rust/frontend packaging and maps the generated product into `Contents/Library/SystemExtensions/chimera-dns-proxy.systemextension`. An unsigned `.app` bundle confirmed the extension lands at that path with both architectures. This is a packaging path only: the extension and host app are not yet signed with matching entitlements/profiles, and the provider is not activated.

## Reference difference and limits

### 2026-10-02: local-host DNS authorization repair

Reference checked at `cc21cbd31dc16c3e3b76c27867dd22aeae3b9cf1`, clean worktree;
runtime submodule `889901cb57b1b1753354c2b7b0a442569601e417`. The mapping remains
the two manager `dns.rs` copies and the Tauri local-host adapter listed above.
Both ref and Chimera invoked `scutil` directly from the unprivileged app host.
Granting setuid to the selected core does not elevate the host's DNS command.
The reported runtime error was `DNS override activation failed`, followed by
`read-back mismatch after apply: None`; the WebSocket resets followed runtime recovery.
The owned key was absent on read-only inspection. Permission denial is the likely
cause; the supplied log omitted scutil stdout, so the original OS failure was not captured.

This bounded defect correction invokes mutating scutil scripts through macOS
administrator authorization when the host is not elevated. Reads stay unprivileged;
an already-applied value and an absent key on restore skip the write. Root service
hosts retain the direct command path. The local host allows 60 seconds for the
authorization and DNS read-back operation. Failed, denied, cancelled, or timed-out
authorization is latched for this controller lifetime to prevent password prompts
from every background recovery; restart the app to retry. Ownership records and
restore/read-back checks remain in place, including uncertain side effects after timeout.

Apple's [scutil source](https://github.com/apple-oss-distributions/configd/blob/main/scutil.tproj/cache.c)
prints failed dynamic-store writes to stdout. Mutating commands now reject nonempty
stdout even on exit code zero; non-dictionary read-back errors are rejected rather
than interpreted as an empty resolver list. Recheck this exception when ref adds
an authorized host DNS boundary. This does not prove resolver selection or leak prevention.

Test contract: `dns::macos::authorization_tests` is a pure command-construction and
output-validation boundary. The permission-failure case supplies `Access denied`
with a successful process exit assumed, and requires a DNS error containing that
diagnostic; the prior status-only handling accepted this output. The quoting case
checks that a script with an apostrophe remains literal data inside the AppleScript
shell command. These tests perform no OS authorization or DNS mutation and cannot
prove real apply, cancellation, timeout recovery, or network behavior. Runnable entry
points are `cargo test --manifest-path backend/Cargo.toml -p chimera-core-manager dns:: --lib`
and the same command with `backend/chimera-runtime/Cargo.toml` for the service copy.
Real desktop verification for main UI, legacy UI, and confirmed agent actions remains
on a dedicated runner: enable TUN, authorize DNS, verify owned key/runtime/network;
disable TUN, authorize removal if needed, verify restoration. Denial and timeout must
retain ownership/recovery evidence and must not repeatedly reopen dialogs.

Service-workspace execution was blocked by a preexisting duplicate `[dev-dependencies]`
table in `backend/chimera-runtime/chimera_service/Cargo.toml:106`. No host DNS was
changed during this repair. Application-side results are reported with the delivery.

Actual checks: manager `dns:: --lib` passed 2 pure authorization tests;
manager `dns_ --lib` passed 7 tests, including 5 fixture lifecycle failure/restore
tests (one authorization case overlaps the first run). `cargo check -p chimera`
passed with existing warnings; scoped rustfmt and root/submodule diff checks passed.
These are not desktop, authorization-dialog, or real-network passes.

The reference injects the controller into its local host but currently derives a loopback resolver from `dns.listen`; Chimera needs TUN port-53 interception and therefore points the OS resolver at the sentinel instead. Service-manager injection is an additional Chimera path so Local and Service runtimes share the same lifecycle behavior.

The `scutil` dynamic-store mechanism is still unverified on a real Mac. We have not confirmed that the dedicated resolver is selected before physical-service resolvers or that macOS will not retry another resolver after the sentinel times out. Until those checks pass, this is a fail-closed design intent, not proof that DNS cannot leak. Application DoH/DoT/DoQ, browser secure DNS, and multicast DNS are outside UDP/TCP 53 interception.

The pre-activation path closes the apply-failure gap for cold starts, process replacements, graceful switches, and in-place updates whose target plan requests a host DNS override. A transition to a plan with no DNS intent now restores the old override before launching/committing the target when an ownership record exists; restore failure blocks that transition. The converge tail remains as a final consistency pass. If DNS apply, restore, read-back, or ownership persistence fails after a target has otherwise completed, public manager entry points return an error, but the runtime transition may already be committed; the ownership record remains for retry. Service shutdown now waits for the control transaction and keeps retrying manager shutdown until the core is stopped; a persistent DNS restore failure keeps service teardown pending rather than intentionally dropping the TUN/core.

Apple's supported all-DNS path is `NEDNSProxyProvider`: Apple documents that it receives DNS flows from apps on UDP/TCP port 53. Its macOS deployment row requires a system extension (macOS 10.15+); only one DNS proxy can be active system-wide, and enabling Chimera's disables another app's proxy. Apple advises against using a packet-tunnel provider to intercept all system DNS and points to DNS proxy/DNS settings APIs. Chimera's Xcode target is now embedded at `Contents/Library/SystemExtensions/chimera-dns-proxy.systemextension` in an unsigned app bundle. Xcode 27's current macOS SDK prevents building this target below macOS 12. For direct Developer ID distribution, the host and extension need matching Network Extension and system-extension install entitlements, provisioning profiles, and Team ID; the extension also uses an app-group-scoped Mach service. The current macOS workflow contains no Apple signing/profile/notarization setup. Tauri now embeds the built target at `Contents/Library/SystemExtensions` through `bundle.macOS.files`; signing, activation, and conflict handling still need implementation. See [NEDNSProxyProvider](https://developer.apple.com/documentation/networkextension/nednsproxyprovider), [NEDNSProxyManager](https://developer.apple.com/documentation/networkextension/nednsproxymanager), [TN3120](https://developer.apple.com/documentation/technotes/tn3120-expected-use-cases-for-network-extension-packet-tunnel-providers), [TN3134](https://developer.apple.com/documentation/technotes/tn3134-network-extension-provider-deployment), [Network Extensions entitlement](https://developer.apple.com/documentation/BundleResources/Entitlements/com.apple.developer.networking.networkextension), [System Extensions](https://developer.apple.com/documentation/systemextensions), and [Tauri macOS bundle files](https://v2.tauri.app/distribute/macos-application-bundle/).

Bridge check: the managed loopback endpoint is now `127.0.0.1:1053` and starts with the protected TUN DNS configuration. The provider manager still needs to set `resolverHost`/`resolverPort` to that endpoint, verify listener readiness, and only then enable the provider. The endpoint is currently a fixed port; collision fails core readiness instead of exposing the listener on a wildcard address.

## Validation and next step

Validation on 2026-09-28: the current nested service workspace passed `shutdown_retry_keeps_retrying_after_a_transient_failure`, `core_types_map_onto_manager_kinds`, and `dns_restore_failure_aborts_stop_and_keeps_ownership_record`; `cargo check -p chimera-service` and `cargo fmt --check` passed. `cargo test -p clash-lib --lib managed_bridge -- --nocapture` passed all three listener-selection tests; `cargo fmt --all -- --check` passed. `swift test --package-path backend/tauri/macos/dns-proxy` passed all four bridge-configuration tests and compiled the provider relay. `backend/tauri/macos/dns-proxy/build-system-extension.sh` succeeded; `lipo -archs` confirmed arm64+x86_64 and `plutil -lint` accepted the generated Info.plist. `cargo check -p chimera` passed with preexisting warnings. `pnpm tauri build --bundles app --no-sign` built the unsigned app and placed the universal extension under `Contents/Library/SystemExtensions`. This does not prove signing, extension activation, or DNS behavior. No host DNS setting was changed in this worktree.

```sh
cargo test -q --manifest-path backend/Cargo.toml -p chimera-core-manager dns_activation_failure_blocks_runtime_launch
cargo test -q --manifest-path backend/Cargo.toml -p chimera-core-manager missing_macos_dns_controller_blocks_protected_launch
cargo test -q --manifest-path backend/Cargo.toml -p chimera-core-manager dns_restore_failure_aborts_stop_and_keeps_ownership_record
cargo test -q --manifest-path backend/Cargo.toml -p chimera-core-manager dns_restore_failure_is_reported_by_convergence
cargo test -q --manifest-path backend/Cargo.toml -p chimera-core-manager dns_restore_failure_blocks_unprotected_runtime_launch
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager dns_activation_failure_blocks_runtime_launch
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager missing_macos_dns_controller_blocks_protected_launch
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager dns_restore_failure_aborts_stop_and_keeps_ownership_record
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager dns_restore_failure_is_reported_by_convergence
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager dns_restore_failure_blocks_unprotected_runtime_launch
cargo check -q --manifest-path backend/Cargo.toml -p chimera-core-manager
cargo check -q --manifest-path backend/Cargo.toml -p chimera
cargo check -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager
cargo check -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service shutdown_retry_keeps_retrying_after_a_transient_failure
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-service core_types_map_onto_manager_kinds
cargo test -q --manifest-path backend/chimera-runtime/Cargo.toml -p chimera-core-manager dns_restore_failure_aborts_stop_and_keeps_ownership_record
```

The extension build and app-bundle placement are complete. The remaining implementation slice is host-side activation/deactivation with ownership tracking, setting `resolverHost=127.0.0.1` and `resolverPort=1053`, and preventing Chimera from silently replacing another active DNS proxy. Apple's `NEDNSProxyManager.loadFromPreferences` reads the calling app's DNS proxy preferences, while setting `isEnabled = true` disables any other app's DNS proxy; therefore the active system-wide conflict check needs an authoritative source beyond this caller-owned preference object ([load behavior](https://developer.apple.com/documentation/networkextension/nednsproxymanager/loadfrompreferences%28completionhandler%3A%29), [enable behavior](https://developer.apple.com/documentation/networkextension/nednsproxymanager/isenabled?changes=_5)). This Mac currently reports no valid signing identities, so activation cannot be validated here until a matching Team ID, entitlements, and provisioning profile are available. DIRECT-specific DNS remains deferred and does not block this system-level interception work. A dedicated-Mac Phase-0 check can inspect `scutil --dns` before/apply/restore, prove regular A, AAAA, and HTTPS queries enter the intended resolver, capture packets on the physical interface to check for leaked port-53 queries, and verify restart/orphan recovery. Do not treat a compile or a fixture test as that evidence.
