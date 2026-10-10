# Agensi MCP

<div align="center">

[![CI](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml/badge.svg)](https://github.com/KiaroSama/Agensi-MCP/actions/workflows/ci.yml)
[![License GPLv3](https://img.shields.io/badge/License-GPLv3-blue)](LICENSE)
[![Node 24.21+](https://img.shields.io/badge/Node-24.21%2B-43853d)](package.json)
[![PowerShell 7](https://img.shields.io/badge/PowerShell-7-5391fe)](run.ps1)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078d4)](#start)
[![JavaScript ESM](https://img.shields.io/badge/JavaScript-ESM-f7df1e)](src/bridge.mjs)
[![MCP](https://img.shields.io/badge/Protocol-MCP-7c3aed)](https://modelcontextprotocol.io/)
[![Streamable HTTP](https://img.shields.io/badge/Transport-Streamable_HTTP-2563eb)](https://agensi.io/mcp)
[![mcp-remote](https://img.shields.io/badge/Bridge-mcp--remote-64748b)](https://github.com/punkpeye/mcp-remote)
[![Undici](https://img.shields.io/badge/HTTP-Undici-334155)](https://github.com/nodejs/undici)
[![SOCKS](https://img.shields.io/badge/Proxy-SOCKS-0f766e)](#proxy-behavior)
[![Documentation](https://img.shields.io/badge/Docs-README-0284c7)](#operator-skill-and-plugins)
[![Support donations](https://img.shields.io/badge/Support-donations-d04a9a)](#donate)

</div>

Project-local Agensi connection manager with named fail-closed proxies, manual OAuth sign-in and
original operating/skill-optimization guidance. The remote service remains hosted at
https://mcp.agensi.io/mcp. This is an independent integration, not Agensi's server or an official
Agensi product. It does not make purchases or publish marketplace listings automatically.

## Start

Requirements: existing Node.js24.21+ and PowerShell7. Install locked project dependencies:

```bash
npm ci --ignore-scripts
```

Open the menu from the project folder:

```bash
pwsh -NoProfile -File ./run.ps1
```

1. Import a proxy list with option1; Enter uses the local list beside the launcher.
2. Select a profile with option2. Initial mode is **disconnected** until proxy selection or explicit direct mode.
3. Use option7, copy the full genuine sign-in URL into your external browser on this machine and keep the listener open.
4. After a usable session is saved, reconnect your MCP client. Option6 registers the bridge in Desktop and user-scope Code without replacing unrelated entries.

Main menu shows only mode, selected profile and saved count; profiles appear in the selection submenu.
`exit` quits. Registration changes client settings and should only be used when intended.

## Manual sign-in

```bash
pwsh -NoProfile -File ./run.ps1 -SignIn
```

Authorization is human-initiated; no browser auto-opens. Callback waits up to five minutes. Tokens
stay in project-local authentication storage and are reused by the bridge. Login and bridge discover
identical resource scopes and share a stable callback identity. A busy callback port fails explicitly.
Never paste tokens, authorization codes or callback URLs into chat.

Public connectivity does not prove login. Account-scoped reads do not prove creator/payout readiness.
A failed readiness response remains unknown even if a web dashboard says active. Browser login and
checkout use the browser's own routing, not this bridge's proxy policy.

## Import proxies

UTF-8 rows use a final **display name** field:

```text
socks5://127.0.0.1:1080:example-user:example-password:Example Proxy
```

These are examples, not real credentials. The six-field format does not support colons inside values.
Import preserves the current route, skips identical duplicates and cancels atomically on conflicting
names/malformed rows, reporting line numbers without credential values. Limits:100 profiles and1MiB
input. Keep real proxy lists private.

```bash
pwsh -NoProfile -File ./run.ps1 -ImportProxyList ./my-private-proxies.txt
```

## Proxy behavior

HTTP/HTTPS/SOCKS4/SOCKS5, plus SOCKS4a/SOCKS5h, are supported. Ordinary4/5 resolve destination DNS
locally;4a/5h use proxy-side destination DNS. Proxy-host DNS can remain local. SOCKS4 accepts user ID,
not password authentication or IPv6 destination. TLS certificates are validated.

There is **no direct fallback, automatic alternate-profile switch or NO_PROXY bypass**. Invalid or
failed selected proxy blocks connection. Route changes invalidate old connections within the200ms
watcher interval; reconnect clients. Interrupted billable/upload operations are not replayed. Reconcile
unknown ownership/listing outcomes before retrying. Own loopback OAuth coordination is the narrow
direct exception. This is application routing, not an OS firewall or all-Claude/browser proxy.

## Operator skill and plugins

Original GPLv3 guidance is in [packages/agensi-operator](packages/agensi-operator/README.md):
search/select workflows, market-demand hypotheses, matched competitive comparisons, draft optimization,
privacy/payment/license rules and safe tool use. It is not a skill submitted for sale.

- Claude/Codex/Cursor: portable root plus native compatibility manifests.
- Antigravity: [separate native root](packages/agensi-operator-antigravity/README.md), because its schema differs.
- Standalone: copy the contained skill into the host's documented project skill directory.
- No plugin MCP declaration, hooks, auto-install, account credentials or tool preapproval.

Read the package guide before installation; file presence does not prove native discovery. Avoid
multiple copies in overlapping host skill directories. Actual loading on all four clients is unverified.

[Evaluation method/results](docs/operator-evaluation.md) records a real paired fresh-agent output
experiment. The competent baseline also met core requirements; no statistical effectiveness lift,
actual buyer-payment validation or guaranteed sales claim is made. Public installs are not sales.

## Inline marketplace files

After a specifically approved free/owned one-time install, save returned path/content files as a local
JSON array (or object with `files`) and use:

```bash
node src/manage.mjs save-skill example ./returned-files.json
```

The helper validates paths, UTF-8, required SKILL.md, case-insensitive duplicates, device/ADS names,
traversal and linked ancestors; existing packages are refused. It saves project-local files without
executing scripts or activating global skills. Subscription skills are cloud-only. Purchases, trials,
cloud data transfer, media uploads and Creator submission each need their own approval. Even a
submission preview uploads content; submission/review/live are different states.

## Sensitive data and logging

Do not publish or share runtime configuration, proxy credentials, authentication state, acquired
skill contents or operational logs wholesale. Tokens/proxy settings are not encrypted vault storage;
restrict directory access appropriately. The repository distributes original source and guidance only.

Each executable management/bridge run writes a new UTF-8 file in `logs/`, named
`agensi-menu_YYYY-MM-DD_HH-mm-ss_UTC_<pid>.log` or equivalent `agensi-bridge` name. Entries use
`[YYYY-MM-DD HH:mm:ss UTC] [LEVEL] [COMPONENT] Message`. Lifecycle/failures are recorded without
protocol bodies, passwords/tokens or sign-in URLs. Initialization failure falls back to stderr;
handlers close on normal shutdown. Retention is manual. Share reviewed sanitized excerpts only.

## Tests and CI

```bash
npm test
```

Node's built-in runner executes bounded loopback routing/TLS, profile persistence/import, exact inline
files, PowerShell menu, file-URL preload and fake OAuth session/scope regressions. TLS fixtures need
OpenSSL; PowerShell tests need `pwsh`. No test purchases, publishes or contacts Agensi. Clean-checkout
preload testing uses an isolated configuration and never changes the owner's route/token state.

CI on Windows2025 selects Node24.21, installs lockfile with scripts disabled, checks source/package
UTF-8/privacy patterns/reference parity, runs regression tests and audits production dependencies.
Job and test deadlines are bounded. Dependabot covers npm and Actions. Real service auth/payout,
private real-input scenarios, other OS launchers and native four-client integration remain separate
from those automated checks. A TLS-IP fixture can emit Node's SNI deprecation warning; no TLS
validation is disabled to silence it.

## License

Copyright2026 Kiaro Sama. Original project source and operating guidance are licensed under
[GNU General Public License version3 only](LICENSE), SPDX `GPL-3.0-only`. Dependencies and referenced
third-party sources retain their own licenses. No proprietary marketplace instruction sets are included.

Sources: [Agensi setup](https://agensi.io/docs/mcp-setup), [mcp-remote](https://github.com/punkpeye/mcp-remote),
[Claude MCP](https://code.claude.com/docs/en/mcp), [Agent Skills](https://agentskills.io/specification).

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
