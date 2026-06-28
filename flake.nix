{
  description = "Chimera Tauri development environment";
  inputs.nixpkgs.url = "path:/nix/store/pzxxxg9vvzk63122vj38lcmqg9dl6qxk-nixos-26.05.1947.a0374025a863/nixos";
  outputs = { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      runtimeLibs = with pkgs; [
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
      devShells.${system}.default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          nodejs_24 pnpm corepack git
          cargo rustc rustfmt clippy
          clang llvmPackages.libclang cmake ninja gnumake pkg-config protobuf
          desktop-file-utils
          wrapGAppsHook4
        ];
        buildInputs = runtimeLibs;
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
        COREPACK_HOME = "$HOME/.cache/node/corepack";
        shellHook = ''
          export PATH="${pkgs.cargo}/bin:${pkgs.rustc}/bin:${pkgs.rustfmt}/bin:${pkgs.clippy}/bin:$PATH"
          export PNPM_HOME="$HOME/.local/share/pnpm"
          export XDG_CACHE_HOME="$HOME/.cache"
          export XDG_CONFIG_HOME="$HOME/.config"
          export XDG_DATA_HOME="$HOME/.local/share"
        '';
        RUST_BACKTRACE = "1";
      };
    };
}
