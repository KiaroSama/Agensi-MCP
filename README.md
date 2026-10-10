# Agensi MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml)
[![GPLv3](https://img.shields.io/badge/License-GPLv3-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.99%2B-dea584)](Cargo.toml)
[![PowerShell](https://img.shields.io/badge/PowerShell-7-5391fe)](run.ps1)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078d4)](#start)
[![Linux core](https://img.shields.io/badge/Core-Linux-64748b)](.github/workflows/ci.yml)
[![MCP](https://img.shields.io/badge/Protocol-MCP-7c3aed)](https://modelcontextprotocol.io/)
[![Streamable HTTP](https://img.shields.io/badge/Transport-Streamable_HTTP-2563eb)](https://agensi.io/mcp)
[![rmcp](https://img.shields.io/badge/SDK-rmcp-64748b)](https://github.com/modelcontextprotocol/rust-sdk)
[![Reqwest](https://img.shields.io/badge/HTTP-reqwest-334155)](https://github.com/seanmonstar/reqwest)
[![SOCKS](https://img.shields.io/badge/Proxy-SOCKS-0f766e)](#routing)
[![Docs](https://img.shields.io/badge/Docs-README-0284c7)](#operator-guidance)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

Native Rust connection manager and stdio↔Streamable HTTP Agensi bridge, with named fail-closed
proxies, manual OAuth and original multi-client operating guidance. No Node runtime is invoked.
Agensi's server stays at https://mcp.agensi.io/mcp; this independent project is not an official Agensi
product, marketplace listing or account authorization.

## Start

Requirements: existing Rust1.99+ for building, PowerShell7 for the Windows launcher. Build dependencies,
Cargo cache and target output stay under this project. No global runtime is installed automatically.

```bash
pwsh -NoProfile -File scripts/build.ps1
```

```bash
pwsh -NoProfile -File run.ps1
```

1. Import proxies with option1 (default local list beside launcher).
2. Select a named profile with option2. Initial mode is disconnected; option4 explicitly enables direct.
3. Use option7 and copy the complete real authorization URL into an external browser on this machine.
4. Keep listener open until approval; reconnect your MCP client. Option6 registers the native executable.

Main menu shows only connection/selection/count; saved profiles appear in selection submenu. `exit`
quits. A registration conflict is refused; review existing settings rather than silently replacing them.

## Desktop entry

After build, merge this example into `mcpServers`, replacing the example root with your actual folder.
Other server entries must remain unchanged.

```json
{
  "mcpServers": {
    "agensi": {
      "command": "<absolute-project-root>/bin/agensi-mcp.exe",
      "args": ["bridge"]
    }
  }
}
```

On Windows JSON paths can use `/` or escaped `\\`. The native binary resolves its installation root
from its own ancestors; caller CWD and spaces do not change route/auth/log storage.

## Routing

HTTP/HTTPS/SOCKS4/SOCKS4a/SOCKS5/SOCKS5h. Ordinary4/5 use local destination DNS;4a/5h use proxy-side
DNS in stored profile names, but the application now normalizes both to SOCKS4a/SOCKS5h at runtime:
destination DNS always occurs through the selected proxy. Stored host, port, credentials and profile
selection remain unchanged. Proxy-host DNS can still be local. TLS certificates remain verified.

Proxy-side resolution prevents local destination DNS, but the provider may use a resolver in another
country or an anycast address. Matching proxy egress country and resolver country cannot be guaranteed
by this application. It adds no timezone metadata/header; UTC log timestamps are not sent upstream.
Browser login/checkout routing and browser timezone remain outside this bridge.

No direct/alternate fallback or environment NO_PROXY bypass. Invalid/unreachable selected proxy blocks
traffic. A route change invalidates old relay within the200ms watcher; reconnect clients. Unknown
charged/upload outcomes are reconciled before retrying, never automatically replayed.

Reqwest handles standard routes. A narrow native authenticated loopback adapter preserves SOCKS4
user IDs that reqwest's upstream connector otherwise ignores. It always tunnels to the selected proxy,
never the destination directly; owns bounded sessions and closes when dropped. SOCKS4 has no password
or IPv6 destination. Browser OAuth/checkout routing is separate; this is not an OS-wide firewall.

## Local HTTP endpoint

Stdio remains available. To serve local MCP HTTP at the configured port (default1594):

```bash
./bin/agensi-mcp.exe http
```

Endpoint: `http://127.0.0.1:1594/mcp`. Change the saved port, then restart the listener:

```bash
./bin/agensi-mcp.exe http-port 2594
```

The `local_http_port` setting accepts1..65535. This is not the OAuth callback, upstream HTTPS443 or
selected proxy port. Host/Origin must match the exact local listener; no external bind, arbitrary
forwarding destination, wildcard CORS or redirects. The owner selected no local token: local processes
are trusted and can use the saved Agensi session. Never expose or forward this port publicly.

## Cache and state paths

The executable derives the project root from its own installed location; no Desktop `env` entry is
needed to choose its runtime paths. Saved OAuth session: `.state/auth/rust-session.json`; refresh lock
alongside it. Proxy/settings: `config.local.json`; logs: `logs/`. No native runtime download cache is
needed to run a built binary. Cargo's build/download cache is `.cache/cargo` only during builds, with
build outputs in `target`; build scripts set those paths explicitly. These paths are independent of
Claude Desktop's own application cache, which this project does not control.

## Proxy import and management

UTF-8 six-field rows with final display name; example values only:

```text
socks5://127.0.0.1:1080:example-user:example-password:Example Proxy
```

Import is additive/atomic, skips identical duplicates, rejects conflicting names without altering route.
Malformed rows report only sanitized line numbers. Limits100 saved profiles/1MiB list; field values
cannot contain colons. Proxy passwords never belong in public source or chat.

```bash
./bin/agensi-mcp.exe status
```

```bash
./bin/agensi-mcp.exe select "Example Proxy"
```

## Manual OAuth

```bash
./bin/agensi-mcp.exe signin
```

Current official SDK handles discovery/scopes/PKCE/state/issuer/token refresh. Human opens the genuine
full URL; no automatic browser. Five-minute callback budget and stable local callback identity. Safe
issuer/scope/expiry-compatible public-client legacy sessions can be imported without extending receipt
time; original data is preserved. Incompatible caches require human renewed login, not weakened checks.
Stored credentials are project-local, not encrypted vault storage; restrict directory access.

Public connectivity, account read, creator/payout readiness and submitted/approved/live listing are
separate evidence layers. Read-only `probe` lists tools and checks an account-scoped response; it does
not buy, upload or verify creator eligibility. Never paste tokens/codes/callback URLs in chat.

## Inline skill files

After specific approved free/owned one-time install, save returned path/content array or `files` object
as local JSON, then:

```bash
./bin/agensi-mcp.exe save-skill example ./returned-files.json
```

Exact UTF-8, root SKILL.md, max500files/20MiB; traversal, ADS/device/linked ancestors, case duplicates
and existing packages refused. Staging/atomic rename preserves originals. No script execution or global
activation. Cloud-only subscriptions stay cloud-only; cost/data/trial/install/creator actions need
separate scope/approval. Submission preview itself transfers files externally.

## Operator guidance

The project root is the plugin root, matching the Revayat skill layout: `.claude-plugin/`,
`.codex-plugin/`, `.cursor-plugin/` and [skills/agensi-operator](skills/agensi-operator/SKILL.md)
are siblings. Root `plugin.json` is the minimal native Antigravity format; the other clients use
same-name compatibility manifests. No duplicate nested package is needed. No plugin MCP/autostart,
hooks or tool preapproval. File presence is not native loading evidence.

Claude Code installation:

```text
/plugin install agensi-operator --marketplace KiaroSama/Agensi-MCP
```

Session-only local loading:

```bash
claude --plugin-dir .
```

Codex standalone discovery uses the original skill copied to the target project's `.agents/skills/agensi-operator`;
Cursor uses `.cursor/skills/agensi-operator` or one supported compatible location. Antigravity native
project plugins use `.agents/plugins/agensi-operator`, or standalone `.agents/skills/agensi-operator`.
Do not create duplicate discovery copies. Native global installation is a separate approved action.

[Paired-output experiment](docs/operator-evaluation.md): competent baseline also meets core criteria;
no proven statistical effectiveness lift or real payment validation. Installs do not establish sales,
conversion, revenue or causality. No private/acquired marketplace instructions redistributed.

## Logging

New perrun UTF-8 `logs/agensi-native_YYYY-MM-DD_HH-mm-ss_UTC_<pid>.log`. Entries:
`[YYYY-MM-DD HH:mm:ss UTC] [LEVEL] [COMPONENT] Message`. Safe startup/failure/shutdown metadata only;
no protocol bodies, proxy credentials/tokens/auth URLs. File-init failure uses stderr. Vendor debug
logging stays disabled because third-party protocol logs may expose credential bytes. Retention manual;
share reviewed sanitized excerpts only. Preserve real operational logs during cleanup.

## Tests and CI

```bash
cargo test --locked --all-targets -- --test-threads=2
```

Use project CARGO_HOME/target environment as in build script. Windows launcher test requires built
`bin/agensi-mcp.exe`; Linux tests cover portable core, not Windows GUI/client integration.

CI Windows2025 and Ubuntu24.04: pinned Rust toolchain, fmt, clippy warnings-as-errors, locked build,
source-oracle management/callback parity, real loopback proxy/DNS/auth handshakes, no-bypass/redirect,
SOCKS4userid lifetime, SDK bidirectional protocol/custom/cancel/backpressure/route change, OAuthPKCE
callback/session, TLS untrusted-certificate rejection, native entry/menu and public-package checks.
Testcases have local deadlines; jobs have outer time budgets and constrained compile/test concurrency.
No tests purchase, publish or contact Agensi. Source parity and known limitations:
[Migration contracts](docs/migration-contracts.md). Positive custom-CA HTTPS-proxy parity and full
native four-client loading remain separate unverified scenarios; live read-only probe alone is not all
production behavior. Rust CodeQL and dependency metadata checks run alongside core CI.

## License

Copyright2026 Kiaro Sama. Original project code/guidance: [GNU GPLv3 only](LICENSE),
SPDX `GPL-3.0-only`. Dependencies and referenced sources retain their licenses.

## Donate

If this project helps you, donations are appreciated.

| Currency | Network | Address |
| --- | --- | --- |
| Bitcoin (BTC) | Bitcoin | `bc1qmth5m03pu5hujw5xw5jmywam3jj3sqwqupesdt` |
| USDT, BNB, USDC, etc. | BEP20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| USDT, TRX, USDC, etc. | TRC20 | `TWBA3xFTqgZAeAYMxqo85xWnzvty3DcAhw` |
| Ethereum (ETH) | ERC20 | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
| TON | TON | `UQCN8Umo_OfOWqImZetQsrNStPcmLkMAKajFyiCOhso23NDb` |
| Litecoin (LTC) | LTC | `ltc1qntqnnrunadurnw4cshv3qgspywrueyyeyngwuy` |
| Solana (SOL) | Solana | `7B2wkczUjmkDhETwQuknBL8sUsbuV7nErxc317TmQuwR` |
| Polygon (POL) | Polygon | `0x0Bd0BA443a8B9cf15922bf7f0Bb0a4b495fD06Ef` |
