{
  description = "Exom - Local-first collaborative workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Rust toolchain
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # Build-time native dependencies
        nativeBuildInputs = with pkgs; [
          rustToolchain
          pkg-config
          cmake # Sometimes needed for native deps
        ];

        # Runtime and build-time libraries
        # Slint requires these for GUI rendering
        buildInputs = with pkgs; [
          # Font rendering
          fontconfig
          freetype

          # Keyboard handling
          libxkbcommon

          # Wayland support
          wayland
          wayland-protocols
          libffi

          # X11 support (fallback/hybrid)
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          xorg.libxcb

          # OpenGL rendering
          libGL
          libGLU

          # Clipboard (arboard)
          xorg.libXfixes
          xorg.libXext

          # SQLite (bundled, but headers may help)
          sqlite
        ];

        # Runtime library path for dynamically linked libs
        runtimeLibs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          wayland
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          xorg.libxcb
          xorg.libXfixes
          xorg.libXext
          libGL
          vulkan-loader # Slint can use Vulkan
        ];

        # Common environment variables for building
        buildEnv = {
          # Help pkg-config find libraries
          PKG_CONFIG_PATH = pkgs.lib.makeSearchPath "lib/pkgconfig" buildInputs;

          # Slint backend selection (prefer Wayland, fallback to X11)
          SLINT_BACKEND = "winit";

          # Ensure fontconfig can find fonts
          FONTCONFIG_FILE = "${pkgs.fontconfig.out}/etc/fonts/fonts.conf";
        };

      in {
        # Main package: exom-app
        packages = {
          exom-app = pkgs.rustPlatform.buildRustPackage {
            pname = "exom-app";
            version = "0.1.0";

            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            inherit nativeBuildInputs buildInputs;

            # Build only the app binary
            cargoBuildFlags = [ "-p" "exom-app" ];

            # Set library paths for build
            preBuild = ''
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
            '';

            # Wrap the binary to include runtime library paths
            postInstall = ''
              wrapProgram $out/bin/exom-app \
                --prefix LD_LIBRARY_PATH : "${pkgs.lib.makeLibraryPath runtimeLibs}" \
                --set FONTCONFIG_FILE "${pkgs.fontconfig.out}/etc/fonts/fonts.conf"
            '';

            nativeBuildInputs = nativeBuildInputs ++ [ pkgs.makeWrapper ];

            meta = with pkgs.lib; {
              description = "Local-first collaborative workspace";
              homepage = "https://github.com/Onyx-Digital-Dev/exom";
              license = licenses.mit;
              platforms = platforms.linux;
              mainProgram = "exom-app";
            };
          };

          default = self.packages.${system}.exom-app;
        };

        # App runner
        apps = {
          exom = {
            type = "app";
            program = "${self.packages.${system}.exom-app}/bin/exom-app";
          };

          default = self.apps.${system}.exom;
        };

        # Development shell
        devShells.default = pkgs.mkShell {
          inherit buildInputs;

          nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
            # Additional dev tools
            sqlite # For inspecting DBs
            cargo-watch
            cargo-edit
          ]);

          # Set up environment for development
          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH"
            export FONTCONFIG_FILE="${pkgs.fontconfig.out}/etc/fonts/fonts.conf"
            export RUST_BACKTRACE=1
            export RUST_LOG=info

            echo "Exom development shell"
            echo "  Run: cargo run -p exom-app"
            echo "  Test: cargo test"
            echo ""
          '';
        };
      }
    );
}
