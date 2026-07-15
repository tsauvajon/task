{
  description = "Rust rewrite of task workflow CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        dylintRustOverlay = (builtins.getFlake "github:oxalica/rust-overlay/017351829a9356423afd2cca0dde9b63346c8ab3").overlays.default;
        dylintPkgs = import nixpkgs {
          inherit system;
          overlays = [ dylintRustOverlay ];
        };
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [
            "clippy"
            "llvm-tools-preview"
            "rustfmt"
          ];
        };
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };
        dylintToolchain = "nightly-2026-04-16";
        dylintRust = dylintPkgs.rust-bin.nightly."2026-04-16".default.override {
          extensions = [
            "llvm-tools-preview"
            "rustc-dev"
            "rust-src"
          ];
        };
        dylintLinkInputs = [ pkgs.zlib ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
        dylintLinkLibraryPath = pkgs.lib.makeLibraryPath dylintLinkInputs;
        dylintToolchainWithTarget = "${dylintToolchain}-${pkgs.stdenv.hostPlatform.rust.rustcTarget}";
        rustupShim = pkgs.symlinkJoin {
          name = "dylint-tool-shims";
          paths = [
            (pkgs.writeShellScriptBin "rustup" ''
              set -euo pipefail

              case "$*" in
                "+stable which cargo"|"which cargo")
                  printf '%s\n' "${dylintRust}/bin/cargo"
                  ;;
                "which rustc")
                  printf '%s\n' "${dylintRust}/bin/rustc"
                  ;;
                "show active-toolchain")
                  printf '%s (overridden by Nix Dylint shim)\n' "${dylintToolchainWithTarget}"
                  ;;
                *)
                  printf 'rustup shim only supports Dylint toolchain queries, got: rustup %s\n' "$*" >&2
                  exit 1
                  ;;
              esac
            '')
            (pkgs.writeShellScriptBin "cargo" ''
              export RUSTUP_TOOLCHAIN="${dylintToolchainWithTarget}"
              export LIBRARY_PATH="${dylintLinkLibraryPath}:''${LIBRARY_PATH:-}"
              exec "${dylintRust}/bin/cargo" "$@"
            '')
          ];
        };
        dylintTools = pkgs.rustPlatform.buildRustPackage rec {
          pname = "dylint-tools";
          version = "6.0.0";

          src = pkgs.fetchFromGitHub {
            owner = "trailofbits";
            repo = "dylint";
            rev = "v${version}";
            hash = "sha256-hoavNSVwaPpA+EtvRw2ukQ2KKg1d9AF7oNCy0mnxKdo=";
          };

          cargoHash = "sha256-WiXf8twRfU7w1b8o0EeZJdCLuXKier41z4ZnzoEUmDQ=";
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.openssl ];
          # Avoid baking Nix's temporary build source path into cargo-dylint.
          DOCS_RS = "1";
          OPENSSL_INCLUDE_DIR = "${pkgs.lib.getDev pkgs.openssl}/include";
          OPENSSL_LIB_DIR = "${pkgs.lib.getLib pkgs.openssl}/lib";
          cargoBuildFlags = [
            "-p"
            "cargo-dylint"
            "-p"
            "dylint-link"
          ];
          doCheck = false;

          installPhase = ''
            runHook preInstall
            targetRoot="''${CARGO_TARGET_DIR:-target}"
            targetDir=""
            for candidate in "$targetRoot"/*/release/cargo-dylint "$targetRoot"/release/cargo-dylint; do
              if [ -x "$candidate" ]; then
                targetDir="$(dirname "$candidate")"
                break
              fi
            done
            if [ -z "$targetDir" ]; then
              echo "cargo-dylint binary was not found under $targetRoot" >&2
              exit 1
            fi
            install -Dm755 "$targetDir/cargo-dylint" -t "$out/bin"
            install -Dm755 "$targetDir/dylint-link" -t "$out/bin"
            runHook postInstall
          '';
        };
        # On macOS, rustfmt is a symlink whose @rpath resolves relative to the
        # symlink target (rustfmt-preview/bin/../lib) rather than the
        # rust-default closure that actually holds librustc_driver-*.dylib.
        # Rather than setting DYLD_LIBRARY_PATH globally (which poisons the
        # Nix-provided clang with an incompatible LLVM, breaking any C
        # compilation), we create a thin wrapper that injects the path only for
        # rustfmt.
        rustfmtWrapped = pkgs.writeShellScriptBin "rustfmt" ''
          export DYLD_LIBRARY_PATH="${rustToolchain}/lib"
          exec "${rustToolchain}/bin/rustfmt" "$@"
        '';
        dylintApp = pkgs.writeShellScriptBin "task-dylint" ''
          set -euo pipefail
          export DYLINT_RUST_BIN="${dylintRust}/bin"
          export DYLINT_RUSTUP_BIN="${rustupShim}/bin"
          export LIBRARY_PATH="${dylintLinkLibraryPath}:''${LIBRARY_PATH:-}"
          export PATH="${dylintTools}/bin:${rustupShim}/bin:${dylintRust}/bin:$PATH"
          exec ${pkgs.just}/bin/just dylint "$@"
        '';
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "task";
          version = "7.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
          # The integration suite touches real git repositories /
          # worktree state under $HOME, which the Nix sandbox does
          # not expose. Skip the test phase here; CI runs it
          # separately with a real workspace.
          doCheck = false;
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/task";
        };

        apps.dylint = {
          type = "app";
          program = "${dylintApp}/bin/task-dylint";
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            rustfmtWrapped
            dylintTools
            pkgs.just
          ];
          env = {
            # Tell cargo to use our wrapped rustfmt rather than the sysroot
            # copy, so that DYLD_LIBRARY_PATH is set only for rustfmt and not
            # for clang/cc.
            RUSTFMT = "${rustfmtWrapped}/bin/rustfmt";
          };
          shellHook = ''
            export DYLINT_RUST_BIN="${dylintRust}/bin"
            export DYLINT_RUSTUP_BIN="${rustupShim}/bin"
            export LIBRARY_PATH="${dylintLinkLibraryPath}:''${LIBRARY_PATH:-}"
          '';
        };
      }
    )
    // {
      # Home Manager module exposing `programs.task`.
      homeManagerModules.default = import ./hm-module.nix self;
    };
}
