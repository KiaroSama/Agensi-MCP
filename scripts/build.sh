#!/usr/bin/env bash
# A subshell also restores the caller's directory/environment when this file is sourced.
(
    set -euo pipefail
    root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
    command -v cargo >/dev/null || { printf '%s\n' 'Rust 1.99+ is required; no automatic global installation.' >&2; exit 1; }
    mkdir -p -- "$root/.cache/cargo" "$root/target" "$root/.tmp"
    export CARGO_HOME="$root/.cache/cargo"
    export CARGO_TARGET_DIR="$root/target"
    export CARGO_BUILD_JOBS=4
    export TEMP="$root/.tmp" TMP="$root/.tmp" TMPDIR="$root/.tmp"
    cd -- "$root"
    cargo build --locked --release
    mkdir -p -- "$root/bin"
    cp -- "$root/target/release/agensi-mcp" "$root/bin/agensi-mcp"
    printf '%s\n' 'Native binary built in bin/agensi-mcp.'
)
