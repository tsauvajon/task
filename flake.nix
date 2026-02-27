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
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "task";
          version = "0.1.0";
          src = ./.;
          cargoLock = {
            lockFile = ./Cargo.lock;
          };
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/task";
        };

        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            rustfmtWrapped
          ];
          env = {
            # Tell cargo to use our wrapped rustfmt rather than the sysroot
            # copy, so that DYLD_LIBRARY_PATH is set only for rustfmt and not
            # for clang/cc.
            RUSTFMT = "${rustfmtWrapped}/bin/rustfmt";
          };
        };
      }
    );
}
