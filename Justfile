set shell := ["bash", "-euo", "pipefail", "-c"]

dylint: dylint-configured dylint-extra

dylint-configured:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${DYLINT_RUST_BIN:?missing from nix develop}"
    : "${DYLINT_RUSTUP_BIN:?missing from nix develop}"
    export PATH="$DYLINT_RUSTUP_BIN:$DYLINT_RUST_BIN:$PATH"
    unset RUSTC_WRAPPER
    unset RUSTC_WORKSPACE_WRAPPER
    unset CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER
    export CARGO_BUILD_RUSTC_WRAPPER=""
    command -v cargo-dylint >/dev/null || {
        echo "cargo-dylint not found. Run inside nix develop: nix develop --command just dylint-configured" >&2
        exit 127
    }
    command -v dylint-link >/dev/null || {
        echo "dylint-link not found. Run inside nix develop: nix develop --command just dylint-configured" >&2
        exit 127
    }
    DYLINT_RUSTFLAGS="${DYLINT_RUSTFLAGS:-} -D warnings" cargo dylint --all -- --all-targets

dylint-extra:
    #!/usr/bin/env bash
    set -euo pipefail
    : "${DYLINT_RUST_BIN:?missing from nix develop}"
    : "${DYLINT_RUSTUP_BIN:?missing from nix develop}"
    export PATH="$DYLINT_RUSTUP_BIN:$DYLINT_RUST_BIN:$PATH"
    unset RUSTC_WRAPPER
    unset RUSTC_WORKSPACE_WRAPPER
    unset CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER
    export CARGO_BUILD_RUSTC_WRAPPER=""
    command -v cargo-dylint >/dev/null || {
        echo "cargo-dylint not found. Run inside nix develop: nix develop --command just dylint-extra" >&2
        exit 127
    }
    command -v dylint-link >/dev/null || {
        echo "dylint-link not found. Run inside nix develop: nix develop --command just dylint-extra" >&2
        exit 127
    }
    cargo dylint --git https://github.com/trailofbits/dylint --tag v6.0.0 --pattern examples/restriction/assert_eq_arg_misordering -- --all-targets
    cargo dylint --git https://github.com/trailofbits/dylint --tag v6.0.0 --pattern examples/supplementary/nonexistent_path_in_comment -- --all-targets
    cargo dylint --git https://github.com/trailofbits/dylint --tag v6.0.0 --pattern examples/supplementary/unnecessary_conversion_for_trait -- --all-targets
    cargo dylint --git https://github.com/KSXGitHub/perfectionist --rev f78dfcf2aa5a6676670b88bd13a535359ae4f5ad -- --all-targets
