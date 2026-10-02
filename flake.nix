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
          isLinux = pkgs.stdenv.isLinux;
          runtimeLibs = with pkgs; lib.optionals isLinux [
            openssl
            gtk3
            glib
            cairo
            pango
            gdk-pixbuf
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
          ];
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              nodejs_24
              corepack
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
            ] ++ lib.optionals isLinux [ desktop-file-utils wrapGAppsHook4 ];

            buildInputs = runtimeLibs;

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
            RUST_BACKTRACE = "1";

            shellHook = ''
              export COREPACK_HOME="''${COREPACK_HOME:-$HOME/.cache/node/corepack}"
              export DENO_INSTALL_ROOT="''${DENO_INSTALL_ROOT:-$HOME/.deno}"
              export XDG_CACHE_HOME="''${XDG_CACHE_HOME:-$HOME/.cache}"
              export XDG_CONFIG_HOME="''${XDG_CONFIG_HOME:-$HOME/.config}"
              export XDG_DATA_HOME="''${XDG_DATA_HOME:-$HOME/.local/share}"

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
              echo "Rust: run 'rustup toolchain install stable --component rustfmt --component clippy' for checks and 'rustup toolchain install nightly' for the runtime workspace."
              echo "Network: inherited proxy variables are respected; set CHIMERA_HTTP_PROXY and CHIMERA_NO_PROXY to override inside this shell."
            '';
          };
        });
    };
}
