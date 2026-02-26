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
          ];
          env = {
            # On macOS, rustfmt is a symlink whose @rpath resolves relative to
            # the symlink target (rustfmt-preview/bin/../lib) rather than the
            # rust-default closure that actually holds librustc_driver-*.dylib.
            # Setting DYLD_LIBRARY_PATH here lets dyld find the dylib at launch
            # time.  This is a no-op on Linux (which uses LD_LIBRARY_PATH).
            DYLD_LIBRARY_PATH = "${rustToolchain}/lib";
          };
        };
      }
    );
}
