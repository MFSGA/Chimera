# NixOS development and compatibility

Chimera supports Linux x86_64 through Tauri 2 / GTK3 / WebKitGTK 4.1, and
`flake.nix` provides a reproducible developer shell on NixOS. It does not
install or change any host networking services.

## Scope and safety

- Objective: build and run the existing Chimera desktop sources on NixOS using
  the tool versions and native dependencies declared by this repository.
- Non-goals: change Linux proxy/service semantics, automatically enable TUN,
  alter NixOS system configuration, update the pinned `ref/` baseline, or
  claim compatibility with every desktop environment.
- Interfaces: the repository adds `pnpm nixos:check`; existing Tauri commands,
  public APIs, persisted state, and system networking behavior are unchanged.
- Risk: L2 because the Nixpkgs dependency pin changes, although the developer
  checks themselves are read-only. The dependency refresh warrants a full
  desktop build check. Rollback is to revert the flake and script changes;
  no persisted user configuration is migrated.

## Set up

From the Chimera repository root:

```sh
nix develop
pnpm --version
pnpm install --frozen-lockfile
pnpm nixos:check
```

The Nix shell supplies the pinned `nodejs_24`, Corepack-backed `pnpm`, Deno,
Rust build tools, and pkg-config libraries required by WebKitGTK and GTK3.
Corepack chooses the pnpm version declared in `package.json` rather than
requiring a system-wide pnpm installation.

The first `nix develop` after a lock-file update can download a significant
number of binary dependencies. Do not use the globally installed Node or pnpm
as evidence that the flake environment is working.

`pnpm nixos:check` is read-only. It checks the toolchain, relevant native
libraries, and presence of all sidecars required by Tauri's `externalBin`
manifest. It reports, but does not grant, TUN permission or a desktop session.
If it reports missing sidecars, prepare those assets with:

```sh
pnpm prepare:check
pnpm nixos:check
```

The preparation step can download external binaries and resources; it does
not install a system service or enable a proxy. Upstream release availability
may prevent some optional alpha binaries from downloading; a complete Tauri
bundle requires every `externalBin` listed in the manifest.

## Run and verify

From a graphical NixOS login session, after preparing the resources:

```sh
nix develop
pnpm typecheck
pnpm tauri:dev
```

`pnpm web:build` builds the web UI only and is not proof that the desktop
backend, bundled sidecars, service integration, or system proxy works. Use
`pnpm build` for a desktop build; release bundles with updater artifacts may
require signing credentials. Do not substitute test fixtures for actual TUN,
proxy, DNS, or service-mode verification.

## Runtime integration

- **Display:** Start the desktop app within an X11 or Wayland user login with
  the appropriate session environment and D-Bus session. An isolated remote
  shell without `DISPLAY`/`WAYLAND_DISPLAY` cannot verify the GUI.
- **System proxy:** Desktop-environment proxy behavior depends on the user's
  session and available settings backends. Do not assume a successful API call
  proves that every application uses the proxy.
- **TUN and system service:** `/dev/net/tun` existence does not grant routing,
  DNS, or service privileges. Test these only after explicit authorization in
  a safe development environment; never auto-install a service or rewrite host
  network rules as part of the build. The pinned `backend/chimera-runtime`
  service installer copies its binary to `/usr/bin` on Linux and performs
  imperative user/group and service registration. That path is not a native
  NixOS service deployment and is **not** validated or enabled by this flake.
  It needs a separately reviewed declarative integration or upstream service
  change before NixOS service-mode support can be claimed.
- **Foreign ELF binaries:** Prebuilt Linux sidecars may depend on a glibc
  loader not present on another NixOS host. If running a downloaded binary
  fails with `No such file or directory` despite the file existing, inspect
  its ELF interpreter and dependencies; consider NixOS `programs.nix-ld.enable`
  in the host configuration if needed. Do not silently mutate system config.
- **Packaging:** CI Linux `.deb` / `.rpm` / `.AppImage` builds target generic
  Linux distributions. They are not a tested native NixOS package. Run from
  the development shell for local source testing; test desktop bundles on the
  intended host before distributing them as NixOS-compatible.

## Acceptance and verification

1. Nix shell starts with the exact Node engine declared by `package.json` and
   a usable `pnpm` command (including nested npm scripts).
2. Toolchain and Linux native libraries pass `pnpm nixos:check` after assets
   are prepared.
3. Frontend typecheck, production frontend build, and desktop Rust build are
   exercised separately and failures reported without claiming they passed.
4. Real graphical launching and proxy/TUN/service behavior require separate
   host-authorized E2E testing; unavailable coverage must be listed explicitly.

## Verification record (2026-10-30, NixOS 26.05 x86_64)

- `nix flake check --no-build`: passed for `x86_64-linux`.
- `pnpm prepare:check`: passed after initializing the pinned
  `backend/chimera-runtime` submodule; all Linux sidecars present.
- `pnpm nixos:check`: passed with zero errors, one headless-session warning.
- `pnpm typecheck` and `pnpm web:build`: passed.
- `cargo +nightly check --locked --manifest-path backend/tauri/Cargo.toml
--features default-meta`: passed with upstream/source warnings.
- `pnpm tauri build --debug --no-bundle --no-sign`: passed, producing
  `backend/target/debug/chimera` (not a release installer).
- `ldd backend/target/debug/chimera` inside the development shell: no
  missing shared libraries.
- `pnpm check:service-runtime` and `pnpm lint:frontend-boundaries`: passed.

Not executed: GUI E2E, host system proxy/TUN mutation, privileged service
installation, or a signed release bundle. Native NixOS installation/service
support is not established by this build.
