# Local Fail-Closed Connection

A skill/plugin is not an MCP server and does not grant account access. This package declares no remote MCP connection, credentials, hooks or auto-start behavior. Reuse the user's explicitly configured local bridge instead of silently adding a direct remote server.

## Management interface

In the connection-manager project root, `run.ps1` provides proxy import/selection, explicit direct mode, disconnect, client registration and manual sign-in. `./bin/agensi-mcp.exe status` returns sanitized route metadata. Read current README/source before invoking commands; do not hardcode a machine's installation directory.

Change a profile only when requested: `./bin/agensi-mcp.exe select "<saved-name>"`. Reconnect MCP clients after a route change. Do not read credential-bearing proxy/config/auth files into chat or publish them.

## Routing

Startup defaults disconnected. Selected proxy failure blocks traffic; no direct/alternate-profile fallback or environment NO_PROXY bypass. Route changes invalidate old connections. HTTP/HTTPS/SOCKS4/SOCKS5 are supported; SOCKS4a/5h select proxy-side destination DNS, while ordinary4/5 use local destination DNS. Proxy-host DNS can remain local. TLS validation stays enabled.

Browser authorization and checkout use the browser's own route; this bridge is not an OS-wide firewall. Own loopback OAuth coordination is a narrow exception, not permission for upstream bypass. Don't replay uncertain billable/upload operations after disconnect.

## Manual authentication

Run `pwsh -NoProfile -File ./run.ps1 -SignIn` only for an authorized human-login task. Display the genuine complete authorization URL, keep local callback alive, and let the user approve in their external browser. Never ask for tokens, codes or callback URLs in chat. Both login and bridge must use identical discovered resource scopes/provider metadata and shared session identity; do not weaken issuer/state validation to repair cache errors.

## Evidence layers

- Public tool discovery proves connectivity, not account login.
- Saved usable token proves local session persistence at that time.
- Successful account-scoped read proves that read works, not payout readiness.
- Creator readiness comes from a successful current service response, not a screenshot.
- Submitted, approved and live are distinct listing states.

## Failure handling

Inspect sanitized status and newest relevant operational log. Classify disconnected/selected-proxy failure, callback-port conflict, scope/cache mismatch, transport failure or account-service error. Preserve selected route and stop dependent actions. Don't switch routes, delete auth state, disable TLS or kill unrelated processes as a diagnostic shortcut.

For unknown charged/submitted outcome, reconcile ownership/listing activity before another attempt. Retry read-only checks within bounded budgets; require the exact human/configuration step that evidence calls for, not secret values. Kill only task-owned temporary processes.

Sources: connection-manager README and source; [Agensi MCP setup](https://agensi.io/docs/mcp-setup), [official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk). Public source describes behavior, not a guarantee that every client or live service environment has been tested.
