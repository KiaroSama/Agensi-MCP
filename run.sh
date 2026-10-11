#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
binary="$root/bin/agensi-mcp"
if [[ ! -x "$binary" ]]; then
  printf '%s\n' 'Native executable missing. Download and extract the matching release first.' >&2
  exit 1
fi
mkdir -p -- "$root/.tmp"
export TEMP="$root/.tmp" TMP="$root/.tmp" TMPDIR="$root/.tmp"
if [[ $# -eq 0 ]]; then
  printf '%s\n' 'Commands: status, doctor, network-check, cleanup [--apply], import <file>, select <name>, direct, disconnect, signin, bridge, http, http-port <port>, save-skill <name> <json>' >&2
  exec "$binary" status
fi
exec "$binary" "$@"
