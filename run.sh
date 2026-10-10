#!/usr/bin/env bash
set -euo pipefail
root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
binary="$root/bin/agensi-mcp"
if [[ ! -x "$binary" ]]; then
  printf '%s\n' 'Native Rust binary missing. Run scripts/build.sh first.' >&2
  exit 1
fi
if [[ $# -eq 0 ]]; then
  printf '%s\n' 'Commands: status, import <file>, select <name>, direct, disconnect, signin, bridge, save-skill <name> <json>' >&2
  exec "$binary" status
fi
exec "$binary" "$@"
