# Full reference `include-all*` migration and compatibility assessment

**Date:** 2026-11-11

**Reference checkout inspected:** `ref@b37e18444e3df012fe8f548e7e0818ed8b4aa227`

**Tracked parent gitlink:** `ref@59724e85c34abb6995c22d101c876cc06dc16576`

## 1. Source migration

The following upstream source files were copied **byte-for-byte** without
changing the expansion algorithm or its test cases:

- `ref/backend/nyanpasu-config/src/runtime/executor/include_all.rs`
  → `backend/chimera-config/src/runtime/executor/include_all.rs`
- `ref/backend/nyanpasu-config/src/runtime/executor/tests/include_all.rs`
  → `backend/chimera-config/src/runtime/executor/tests/include_all.rs`

The reference requires `fancy-regex` for Mihomo/regexp2-style lookaround. The
Chimera config crate now declares `fancy-regex = "0.20.0"` (Cargo's semver
resolver selected **0.20.1**), with the resolved dependency in
`backend/Cargo.lock`. Replacing it with ordinary Rust `regex` would break
lookaround and is **not** a valid adaptation.

The surrounding integration was adapted to the existing Chimera types, not to
change upstream expansion logic:

| Reference responsibility | Chimera integration |
| --- | --- |
| `ClashConfig.expand_include_all` (defaults to `true`) | `clash/config/mod.rs`, with explicit `Default` preserving all prior fields and `serde` default for old files |
| `RuntimePipelineInputs.expand_include_all` | `runtime/executor/mod.rs` |
| RuntimeBuilder maps persisted toggle to executor | `backend/tauri/src/enhance/runtime_builder.rs` |
| Expansion and step logs | `IncludeAllExpansion` snapshot and `StepLog` emitted only when enabled |
| Runtime-input and patch impact | `application_workflow/impact.rs`, `client/clash_config.rs`, and `client/effects/plan.rs` |
| Settings UI | `pages/(main)/main/settings/clash/_modules/expand-include-all-switch.tsx` and Clash route |
| Types and translations | Generated Specta bindings and five existing Chimera locales |

The `Finalizing` node remains the authoritative end-of-pipeline snapshot so
existing Inspect consumers work as before; Chimera additionally sorts top-level
fields after expansion, matching the existing Chimera finalization contract.
Other Runtime ownership, ports, TUN, DNS, and `mmdb` semantics are unchanged.

## 2. Upstream behavior now used

- `include-all: true` includes proxies **and** providers;
  `include-all-proxies` and `include-all-providers` independently select the two.
- Generated proxy names are sorted and appended to explicitly configured
  `proxies`; provider names are sorted into the group's `use` list rather than
  being inserted as synthetic proxy names.
- `filter` supports backtick-separated patterns and lookaround via
  `fancy-regex`. It remains in the group for Core-side processing.
- `exclude-filter` and `exclude-type` remain untouched for Core-side runtime
  filtering, as in the reference. This is **not** an implementation of all
  downstream Mihomo group filtering inside Chimera.
- An empty selected proxy result can create a `COMPATIBLE` or configured
  `empty-fallback` entry, just as the reference does.
- Invalid filters yield a step error log and leave that group unmodified for
  Core-side rejection or handling. Unsupported field types are not silently
  corrected.
- `expand_include_all: false` skips the expansion step entirely and sends
  the raw `include-all*` keys onward to the selected Core. It does **not**
  restore the old Chimera expansion algorithm.

An existing saved `ClashConfig` without this setting deserializes to **true**,
matching the reference. This means enabling the new upstream semantics for
existing users on upgrade is an **intentional behavioral change**.

## 3. Behavioral changes relative to prior Chimera

| Scenario | Prior Chimera | Copied reference |
| --- | --- | --- |
| `include-all: true` with `DIRECT` and two proxies | Generated entries first, then explicit `DIRECT` | Explicit `DIRECT` first, then sorted generated proxies |
| Proxy providers | Provider names appended to `proxies` | Provider names assigned to `use` |
| Proxies/providers list order | Profile/provider declaration order | Sorted names before expansion |
| `include-all-proxies`/`include-all-providers` | Not expanded in Chimera | Expanded independently |
| Regex `filter` and regexp2 lookaround | Not applied by Chimera expansion | Applied when expanding proxies |
| Empty matching proxy set | No upstream-compatible fallback | `COMPATIBLE` or configured `empty-fallback` |
| Disable expansion | No dedicated switch | Raw fields left to the Core |

The existing finalizer regression was updated to require the reference's
`proxies`/`use` separation; this is an explicit semantic change, not a
spurious test update. No legacy fallback was silently installed.

## 4. Assessment: when is a Chimera-specific adaptation necessary?

1. **Before a cross-core release, test actual Core acceptance.** The reference
   expansion is designed around Mihomo. Chimera also supports ClashRs and
   Chimera Client. Whether their parsers honor `use`, `COMPATIBLE`,
   `include-all*`, `filter`, and `empty-fallback` must be checked in isolated
   Core contract tests; no unverified capability claim is made here. If a
   supported Core requires another representation, use an explicit
   **core-specific adapter** with tests, not a silent global change.
2. **Assess upgrade compatibility.** The default of `true` matches the
   reference but changes output for older Profiles using `include-all`.
   The existing on/off setting is not a "legacy expansion" switch. If real
   Profiles require the previous declaration-order behavior, an explicit
   compatibility mode or a migration notice should be designed separately.
3. **Preserve upstream semantics unless tests demonstrate a real defect.** In
   particular, do not deduplicate repeated filter matches or implement
   `exclude-type` on the frontend: the reference intentionally leaves those
   details to the Core. Test regexp runtime limits on hostile configurations
   before enabling user-supplied, untrusted patterns in high-throughput paths.
4. **The legacy/typed bridge does require a Chimera-specific adapter.**
   `CoreFacade::change_core` and legacy runtime generation still reconstruct
   `ClashConfig` from `IVerge`. Without a mirrored setting, a user-selected
   `expand_include_all: false` is lost on that path and becomes `true`.
   An optional `IVerge.expand_include_all` is now mirrored during Typed
   settings updates and read when legacy code rebuilds the runtime. Legacy
   patches also classify this key under Clash settings, not application
   settings. A missing legacy field still means `true`.
5. **No product-specific adaptation is needed for the algorithm itself.** Its
   source file and tests are identical to the reference; Chimera's required
   changes were typed configuration, runtime lifecycle integration, the
   legacy bridge, UI routes, and localized strings.

## 5. Verification boundary

Static/pure tests cover the reference's sorting, provider handling, filter,
lookaround, empty fallback, invalid-pattern logging, and on/off step emission.
Chimera additionally tests default/persistence/patch behavior, runtime
rebuild decisions in the application workflow and effects projection, and the
Typed-to-Legacy mirror plus reverse projection used by older core paths.

**Not run:** live Core acceptance, real subscription provider download,
TUN/DNS/system-proxy changes, or Windows/macOS/Linux desktop E2E. In
particular, the reference implementation's expansion tests are not proof of
compatibility across all Chimera Core flavors.

The parent `ref` gitlink and its existing `backend/Cargo.lock` modification
were not changed during this migration.
