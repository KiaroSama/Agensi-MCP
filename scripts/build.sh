#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
command -v cargo >/dev/null || { printf '%s\n' 'Rust 1.99+ is required; no automatic global installation.' >&2; exit 1; }
export CARGO_HOME="$root/.cache/cargo"
export CARGO_TARGET_DIR="$root/target"
export CARGO_BUILD_JOBS=4
cd -- "$root"
cargo build --locked --release
mkdir -p -- "$root/bin"
cp -- "$root/target/release/agensi-mcp" "$root/bin/agensi-mcp"
printf '%s\n' 'Native binary built in bin/agensi-mcp.'
