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
    if [[ "${1:-}" == --debug ]]; then
        cargo build --locked
        printf '%s\n' 'Internal debug intermediate built under target/debug; installed binary unchanged.'
        exit 0
    fi
    export AGENSI_SOURCE_COMMIT="$(git -C "$root" rev-parse HEAD)"
    [[ "$AGENSI_SOURCE_COMMIT" =~ ^[0-9a-f]{40}$ ]] || { printf '%s\n' 'Verified source commit required.' >&2; exit 1; }
    command -v pwsh >/dev/null || { printf '%s\n' 'PowerShell 7.4+ is required for private signing; installed binary unchanged.' >&2; exit 1; }
    pwsh -NoProfile -File "$root/scripts/sign-release.ps1" -Path "$root/target/release/agensi-mcp" -Preflight
    cargo build --locked --release
    platform=linux-x64
    if [[ "$(uname -s)" == Darwin ]]; then
        platform=macos-x64
        [[ "$(uname -m)" != arm64 ]] || platform=macos-arm64
    fi
    version="$(sed -n 's/^version *= *"\([0-9][0-9.]*\)"/\1/p' "$root/Cargo.toml" | head -n 1)"
    candidate="$root/target/release/agensi-mcp"
    rm -f -- "$candidate.sig"
    pwsh -NoProfile -File "$root/scripts/sign-release.ps1" -Path "$candidate" -Platform "$platform" -SourceCommit "$AGENSI_SOURCE_COMMIT" -Version "$version"
    mkdir -p -- "$root/bin"
    stage="$(mktemp -d "$root/bin/verified.XXXXXXXX")"
    trap 'rm -rf -- "$stage"' EXIT
    cp -- "$candidate" "$stage/agensi-mcp"
    cp -- "$candidate.sig" "$stage/agensi-mcp.sig"
    pwsh -NoProfile -File "$root/scripts/verify-release.ps1" -Path "$stage/agensi-mcp" -Platform "$platform" -SourceCommit "$AGENSI_SOURCE_COMMIT" -Version "$version"
    backup="$(mktemp -d "$root/bin/previous.XXXXXXXX")"
    had_binary=0 had_signature=0
    if [[ -f "$root/bin/agensi-mcp" ]]; then cp -- "$root/bin/agensi-mcp" "$backup/agensi-mcp"; had_binary=1; fi
    if [[ -f "$root/bin/agensi-mcp.sig" ]]; then cp -- "$root/bin/agensi-mcp.sig" "$backup/agensi-mcp.sig"; had_signature=1; fi
    if mv -f -- "$stage/agensi-mcp" "$root/bin/agensi-mcp" &&
       mv -f -- "$stage/agensi-mcp.sig" "$root/bin/agensi-mcp.sig" &&
       pwsh -NoProfile -File "$root/scripts/verify-release.ps1" -Path "$root/bin/agensi-mcp" -Platform "$platform" -SourceCommit "$AGENSI_SOURCE_COMMIT" -Version "$version"; then
        printf '%s\n' 'Private-signed binary verified in bin/agensi-mcp; previous pair retained under bin/previous.* for rollback.'
    else
        if [[ "$had_binary" == 1 ]]; then cp -- "$backup/agensi-mcp" "$root/bin/agensi-mcp"; else rm -f -- "$root/bin/agensi-mcp"; fi
        if [[ "$had_signature" == 1 ]]; then cp -- "$backup/agensi-mcp.sig" "$root/bin/agensi-mcp.sig"; else rm -f -- "$root/bin/agensi-mcp.sig"; fi
        printf '%s\n' 'Installation failed; prior pair restored and recovery retained.' >&2
        exit 1
    fi
)
