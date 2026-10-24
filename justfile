alias b := build
alias c := check
alias f := fmt
alias t := test
alias p := pre-push
alias d := doc
alias vs := verify-standalone
alias vsa := verify-standalone-all

_default:
  @just --list

# Build the project
build:
   cargo build

# Check code: formatting, compilation, linting, and commit signature
check:
   cargo +nightly fmt --all -- --check
   cargo check --workspace --all-features
   cargo clippy --all-features --all-targets -- -D warnings
   @[ "$(git log --pretty='format:%G?' -1 HEAD)" = "N" ] && \
       echo "\n⚠️  Unsigned commit: BDK requires that commits be signed." || \
       true

# Format all code
fmt:
   cargo +nightly fmt

# Run all tests for all crates with all features enabled.
# Daemon tests (MWEB / bitcoind_rpc emitter) skip without LITECOIND_EXE.
test:
   @just _test-chain
   @just _test-core
   @just _test-electrum
   @just _test-esplora
   @just _test-file_store
   @just _test-mweb
   @just _test-bitcoind_rpc
   @just _test-testenv

_test-chain:
    cargo test -p bdk_chain --all-features

_test-core:
    cargo test -p bdk_core --all-features

_test-electrum:
    cargo test -p bdk_electrum --all-features

_test-esplora:
    cargo test -p bdk_esplora --all-features

_test-file_store:
    cargo test -p bdk_file_store --all-features

_test-mweb:
    cargo test -p bdk_mweb --all-features

_test-bitcoind_rpc:
    cargo test -p bdk_bitcoind_rpc --all-features

_test-testenv:
    cargo test -p bdk_testenv --all-features

# Live sync against litecoinspace / Electrum-LTC (ignored by default; needs network).
test-live:
    cargo test -p bdk_esplora --test live_litecoin --features blocking-https -- --ignored --nocapture
    cargo test -p bdk_electrum --test live_litecoin -- --ignored --nocapture --test-threads=1

# Regtest against locally provided litecoind + electrs-ltc.
# Requires LITECOIND_EXE and ELECTRS_LTC_EXE. Esplora daemon tests stay gated (no regtest Esplora).
test-regtest:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${LITECOIND_EXE:-}" || -z "${ELECTRS_LTC_EXE:-}" ]]; then
      echo "Set LITECOIND_EXE and ELECTRS_LTC_EXE to absolute binary paths."
      echo "electrs-ltc has no prebuilt releases; build from https://github.com/rust-litecoin/electrs-ltc"
      exit 1
    fi
    cargo test -p bdk_chain --test test_indexed_tx_graph relevant_conflicts -- --exact --nocapture
    cargo test -p bdk_electrum --lib \
      bdk_electrum_client::test:: \
      -- --nocapture --test-threads=1

# Briefly fuzz every bdk_mweb target, or one: `just fuzz rewind_output`.
# Needs honggfuzz system deps (libunwind, binutils). RUN_TIME=secs per target.
fuzz target="":
    cd crates/mweb/fuzz && ./fuzz.sh {{target}}

# Regenerate the checked-in fuzz seed corpus (crates/mweb/fuzz/hfuzz_input/).
fuzz-corpus:
    cd crates/mweb/fuzz && cargo run --bin gen_corpus

# Run pre-push suite: format, check, and test
pre-push: fmt check test

# Check documentation for all workspace packages
doc:
   RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps

# A failure usually means the crate uses something from a sibling workspace
# crate that isn't released on crates.io yet
[doc("Verify a crate builds as published, against released workspace crates")]
[positional-arguments]
verify-standalone crate *args:
    #!/usr/bin/env bash
    set -euo pipefail

    command -v jq >/dev/null 2>&1 || { echo "Error: jq is required but not installed" >&2; exit 1; }

    # Positional args avoid word-splitting/quoting issues with `{{{{args}}`
    CRATE="$1"
    shift

    echo "Verifying $CRATE can build standalone..."

    # Package the crate; extra args are passed to `cargo build` only
    cargo package -p "$CRATE" --no-verify --allow-dirty

    # Find the packaged tarball (respects CARGO_TARGET_DIR / build.target-dir)
    METADATA=$(cargo metadata --format-version 1 --no-deps)
    TARGET_DIR=$(jq -r '.target_directory' <<< "$METADATA")
    CRATE_VERSION=$(jq -r --arg c "$CRATE" '.packages[] | select(.name == $c) | .version' <<< "$METADATA")
    TARBALL="$TARGET_DIR/package/${CRATE}-${CRATE_VERSION}.crate"

    if [ ! -f "$TARBALL" ]; then
        echo "Error: Could not find packaged tarball at $TARBALL"
        exit 1
    fi

    # Reuse a persistent cache across runs/crates instead of re-downloading
    # the registry index and every dependency from scratch each invocation
    CACHE_DIR="$TARGET_DIR/verify-standalone-cache"
    mkdir -p "$CACHE_DIR/cargo-home" "$CACHE_DIR/target"

    # Create a temporary directory for unpacking
    TEMP_DIR=$(mktemp -d)
    trap "rm -rf $TEMP_DIR" EXIT

    # Unpack the tarball
    tar -xzf "$TARBALL" -C "$TEMP_DIR"

    # Build outside the workspace so sibling crates resolve from crates.io
    cd "$TEMP_DIR/${CRATE}-${CRATE_VERSION}"

    # Reuse the registry cache and build artifacts across invocations for speed
    export CARGO_HOME="$CACHE_DIR/cargo-home"
    export CARGO_TARGET_DIR="$CACHE_DIR/target"

    echo "Building $CRATE in isolation..."
    # --locked: use the packaged Cargo.lock as-is
    cargo build --locked "$@"

    echo "✅ $CRATE builds successfully in isolation!"

# Verify all publishable crates can be built standalone (with --all-features)
[positional-arguments]
verify-standalone-all *args:
    #!/usr/bin/env bash
    set -euo pipefail

    command -v jq >/dev/null 2>&1 || { echo "Error: jq is required but not installed" >&2; exit 1; }

    # Get all publishable workspace crates (examples should set `publish = false`)
    CRATES=$(cargo metadata --format-version 1 --no-deps | jq -r '.packages[] | select(.publish != []) | .name' | sort)

    if [ -z "$CRATES" ]; then
        echo "Error: no publishable crates found via cargo metadata" >&2
        exit 1
    fi

    echo "Verifying all publishable crates can build standalone..."
    echo "Crates to verify:"
    echo "$CRATES" | sed 's/^/  - /'
    echo

    for crate in $CRATES; do
        echo "========================================="
        echo "Verifying $crate..."
        echo "========================================="

        if ! "{{just_executable()}}" verify-standalone "$crate" --all-features "$@"; then
            echo "❌ $crate: FAILED"
            echo "========================================="
            echo "❌ VERIFICATION FAILED for: $crate"
            echo "========================================="
            exit 1
        fi
        echo "✅ $crate: SUCCESS"
        echo
    done

    echo "========================================="
    echo "✅ All crates build successfully in isolation!"
    echo "========================================="
