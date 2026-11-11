{
  description = "Chimera development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      eachSystem = nixpkgs.lib.genAttrs [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" "aarch64-linux" ];
    in {
      devShells = eachSystem (system:
        let
          pkgs = import nixpkgs { inherit system; };
          isLinux = pkgs.stdenv.hostPlatform.isLinux;
          # Corepack supplies the repository-pinned pnpm version, but it does
          # not create a pnpm executable in the immutable Nix store.
          pnpm = pkgs.writeShellScriptBin "pnpm" ''
            exec ${pkgs.corepack}/bin/corepack pnpm "$@"
          '';
          runtimeLibs = with pkgs; lib.optionals isLinux [
            openssl
            gtk3
            glib
            glib-networking
            cairo
            pango
            gdk-pixbuf
            librsvg
            gsettings-desktop-schemas
            webkitgtk_4_1
            libsoup_3
            libayatana-appindicator
            nss
            nspr
            atk
            at-spi2-atk
            cups
            dbus
            expat
            libdrm
            libgbm
            mesa
            alsa-lib
            libxkbcommon
            libx11
            libxcb
            libxcomposite
            libxdamage
            libxext
            libxfixes
            libxrandr
            xdotool
            gst_all_1.gst-plugins-base
          ];

          # Match Nixpkgs' Electron runtime closure for npm's prebuilt binary.
          # Keep this separate from the broader WebKitGTK/Tauri runtime path.
          electronRuntimeLibs = with pkgs; lib.optionals isLinux [
            alsa-lib
            at-spi2-atk
            cairo
            cups
            dbus
            expat
            gdk-pixbuf
            glib
            gtk3
            gtk4
            nss
            nspr
            libx11
            libxcb
            libxcomposite
            libxdamage
            libxext
            libxfixes
            libxrandr
            libxkbfile
            pango
            pciutils
            stdenv.cc.cc
            systemd
            libnotify
            pipewire
            libsecret
            libpulseaudio
            speechd-minimal
            libdrm
            libgbm
            libxkbcommon
            libxshmfence
            libGL
            vulkan-loader
          ];

          electronLibraryPath = pkgs.lib.makeLibraryPath electronRuntimeLibs;
          nixLdLibraryPath = pkgs.lib.makeLibraryPath (electronRuntimeLibs ++ [ pkgs.stdenv.cc.libc ]);
          electronGlLibraryPath = pkgs.lib.makeLibraryPath [ pkgs.libGL pkgs.pciutils pkgs.vulkan-loader ];

          linuxShellHook = pkgs.lib.optionalString isLinux ''
            # Keep nix-ld on the same glibc ABI as this pinned dev shell.
            # Electron itself is patched below to use this loader directly.
            export NIX_LD="${pkgs.stdenv.cc.bintools.dynamicLinker}"
            export NIX_LD_LIBRARY_PATH="${nixLdLibraryPath}"

            # Keep GTK schemas, GIO TLS modules and GStreamer plugins
            # discoverable when WebKitGTK runs inside the Nix shell.
            if [ -n "$GSETTINGS_SCHEMAS_PATH" ]; then
              if [ -n "$XDG_DATA_DIRS" ]; then
                export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH:$XDG_DATA_DIRS"
              else
                export XDG_DATA_DIRS="$GSETTINGS_SCHEMAS_PATH:/run/current-system/sw/share:/usr/share"
              fi
            fi
            if [ -n "$GIO_EXTRA_MODULES" ]; then
              export GIO_EXTRA_MODULES="${pkgs.glib-networking}/lib/gio/modules:$GIO_EXTRA_MODULES"
            else
              export GIO_EXTRA_MODULES="${pkgs.glib-networking}/lib/gio/modules"
            fi
            _GST_PLUGIN_PATH="${pkgs.gst_all_1.gst-plugins-base}/lib/gstreamer-1.0"
            case ":$GST_PLUGIN_SYSTEM_PATH_1_0:" in
              *":$_GST_PLUGIN_PATH:"*) ;;
              *)
                if [ -n "$GST_PLUGIN_SYSTEM_PATH_1_0" ]; then
                  export GST_PLUGIN_SYSTEM_PATH_1_0="$_GST_PLUGIN_PATH:$GST_PLUGIN_SYSTEM_PATH_1_0"
                else
                  export GST_PLUGIN_SYSTEM_PATH_1_0="$_GST_PLUGIN_PATH"
                fi
                ;;
            esac
            unset _GST_PLUGIN_PATH

            # DevSpace and other non-login launchers may omit the active
            # GNOME session variables. Import only display keys from the
            # systemd user manager; never eval its complete environment.
            if command -v systemctl >/dev/null 2>&1 && [ -n "$XDG_RUNTIME_DIR" ]; then
              _session_environment="$(systemctl --user show-environment 2>/dev/null || true)"
              for _name in DISPLAY WAYLAND_DISPLAY XAUTHORITY XDG_SESSION_TYPE; do
                case "$_name" in
                  DISPLAY) _current="$DISPLAY" ;;
                  WAYLAND_DISPLAY) _current="$WAYLAND_DISPLAY" ;;
                  XAUTHORITY) _current="$XAUTHORITY" ;;
                  XDG_SESSION_TYPE) _current="$XDG_SESSION_TYPE" ;;
                esac
                if [ -z "$_current" ]; then
                  _value="$(printf '%s\n' "$_session_environment" | sed -n "s/^$_name=//p" | tail -n 1)"
                  if [ -n "$_value" ]; then
                    export "$_name=$_value"
                  fi
                fi
                unset _current
              done
              unset _session_environment _name _value
            fi

            # npm ships Electron as a generic Ubuntu ELF. Recreate the relevant
            # Nixpkgs Electron fixups in a versioned cache, preserving the exact
            # npm Electron version and the upstream package scripts.
            if [ -z "''${ELECTRON_OVERRIDE_DIST_PATH:-}" ] &&
              command -v patchelf >/dev/null 2>&1; then
              _electron_binary="$(node -e '
                const { createRequire } = require("node:module");
                const packageJson = require.resolve("react-devtools/package.json");
                process.stdout.write(createRequire(packageJson)("electron"));
              ' 2>/dev/null || true)"

              if [ -n "$_electron_binary" ] && [ -x "$_electron_binary" ]; then
                _electron_source="$(dirname "$_electron_binary")"
                _electron_version="$(cat "$_electron_source/version" 2>/dev/null || printf unknown)"
                _electron_cache="''${XDG_CACHE_HOME:-$HOME/.cache}/chimera"
                mkdir -p "$_electron_cache"
                _electron_binary_hash="$(sha256sum "$_electron_binary" | cut -c1-16)"
                _electron_cache_key="$(printf '%s\n%s\n%s' "$_electron_binary_hash" "${pkgs.stdenv.cc.bintools.dynamicLinker}" "${electronLibraryPath}" | sha256sum | cut -c1-16)"
                _electron_dist="$_electron_cache/electron-$_electron_version-$_electron_cache_key-nix"

                if [ ! -f "$_electron_dist/.chimera-nixos-patched" ]; then
                  rm -rf "$_electron_dist"
                  mkdir -p "$_electron_dist"
                  _electron_patch_ok=1

                  for _electron_entry in "$_electron_source"/*; do
                    [ -e "$_electron_entry" ] || [ -L "$_electron_entry" ] || continue
                    _electron_name="''${_electron_entry##*/}"
                    case "$_electron_name" in
                      electron|chrome_crashpad_handler|chrome-sandbox|lib*GL*)
                        cp --reflink=auto -a "$_electron_entry" "$_electron_dist/$_electron_name" || _electron_patch_ok=0
                        ;;
                      libvulkan.so.1) ;;
                      *)
                        ln -s "$_electron_entry" "$_electron_dist/$_electron_name" || _electron_patch_ok=0
                        ;;
                    esac
                  done

                  if [ -e "$_electron_source/libvulkan.so.1" ] || [ -L "$_electron_source/libvulkan.so.1" ]; then
                    ln -s "${pkgs.lib.getLib pkgs.vulkan-loader}/lib/libvulkan.so.1" "$_electron_dist/libvulkan.so.1" || _electron_patch_ok=0
                  fi

                  if [ "$_electron_patch_ok" -eq 1 ]; then
                    patchelf --set-interpreter "${pkgs.stdenv.cc.bintools.dynamicLinker}" --set-rpath "${electronLibraryPath}:$_electron_dist" "$_electron_dist/electron" "$_electron_dist/chrome_crashpad_handler" || _electron_patch_ok=0
                    patchelf --set-interpreter "${pkgs.stdenv.cc.bintools.dynamicLinker}" --set-rpath "${pkgs.stdenv.cc.libc}/lib" "$_electron_dist/chrome-sandbox" || _electron_patch_ok=0

                    for _electron_gl in "$_electron_dist"/lib*GL*; do
                      [ -e "$_electron_gl" ] || continue
                      patchelf --set-rpath "${electronGlLibraryPath}" "$_electron_gl" || _electron_patch_ok=0
                    done
                  fi

                  if [ "$_electron_patch_ok" -eq 1 ]; then
                    printf '%s\n' "$_electron_cache_key" > "$_electron_dist/.chimera-nixos-patched"
                  else
                    rm -rf "$_electron_dist"
                    echo "Chimera: warning: failed to prepare npm Electron for NixOS."
                  fi
                fi

                if [ -f "$_electron_dist/.chimera-nixos-patched" ]; then
                  export ELECTRON_OVERRIDE_DIST_PATH="$_electron_dist"
                  export CHROME_DEVEL_SANDBOX="$_electron_dist/chrome-sandbox"
                  export GDK_PIXBUF_MODULE_FILE="${pkgs.librsvg}/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
                  echo "Chimera: using NixOS-compatible Electron $_electron_version."
                fi
              fi

              unset _electron_binary _electron_source _electron_version _electron_cache _electron_binary_hash _electron_cache_key _electron_entry _electron_name _electron_gl _electron_patch_ok
            fi
          '';
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              nodejs_24
              corepack
              pnpm
              deno
              git
              rustup
              clang
              llvmPackages.libclang
              cmake
              ninja
              gnumake
              pkg-config
              protobuf
              file
              curl
              wget
            ] ++ lib.optionals isLinux [ desktop-file-utils wrapGAppsHook4 xdotool patchelf ];

            buildInputs = runtimeLibs;

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
            RUST_BACKTRACE = "1";

            shellHook = ''
              export XDG_CACHE_HOME="''${XDG_CACHE_HOME:-$HOME/.cache}"
              export XDG_CONFIG_HOME="''${XDG_CONFIG_HOME:-$HOME/.config}"
              export XDG_DATA_HOME="''${XDG_DATA_HOME:-$HOME/.local/share}"
              export COREPACK_HOME="''${COREPACK_HOME:-$XDG_CACHE_HOME/node/corepack}"
              export COREPACK_ENABLE_DOWNLOAD_PROMPT=0
              export DENO_INSTALL_ROOT="''${DENO_INSTALL_ROOT:-$HOME/.deno}"

              ${linuxShellHook}

              if [ "$(uname -s)" = "Darwin" ]; then
                for NODE_BIN in /opt/homebrew/opt/node@24/bin /usr/local/opt/node@24/bin; do
                  if [ -x "$NODE_BIN/node" ] && [ "$($NODE_BIN/node --version)" = "v24.21.0" ]; then
                    export PATH="$NODE_BIN:$HOME/.local/bin:$PATH"
                    break
                  fi
                done
              fi

              # Keep proxy configuration opt-in: Nix, Cargo, Deno and Corepack
              # already honor the standard proxy environment variables.
              if [ -n "''${CHIMERA_HTTP_PROXY:-}" ]; then
                export http_proxy="$CHIMERA_HTTP_PROXY"
                export https_proxy="$CHIMERA_HTTP_PROXY"
                export HTTP_PROXY="$CHIMERA_HTTP_PROXY"
                export HTTPS_PROXY="$CHIMERA_HTTP_PROXY"
              fi
              if [ -n "''${CHIMERA_NO_PROXY:-}" ]; then
                export no_proxy="$CHIMERA_NO_PROXY"
                export NO_PROXY="$CHIMERA_NO_PROXY"
              fi

              if [ "$(uname -s)" = "Darwin" ]; then
                echo "Chimera: macOS SDK/frameworks come from Xcode Command Line Tools."
                if ! xcrun --find clang >/dev/null 2>&1; then
                  echo "Install Xcode Command Line Tools with: xcode-select --install"
                fi
              fi

              echo "Chimera dev shell ($system): Node $(node --version); pnpm via corepack (packageManager pin); Deno $(deno --version | head -n 1)"
              echo "Rust: rustup follows rust-toolchain.toml (nightly-2026-11-09); rustfmt and clippy are available through the pinned toolchain."
              echo "Network: inherited proxy variables are respected; set CHIMERA_HTTP_PROXY and CHIMERA_NO_PROXY to override inside this shell."
            '';
          };
        });
    };
}
